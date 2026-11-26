//! The Metadata panel (Encore: Window ▸ Metadata, "Powered By XMP"; M6.261, M6.396, M6.397): the XMP metadata of the
//! selected asset's file (a timeline's: its video's) laid out as Encore CS6 lays it out (docs/encore-behavior.md
//! 9.14). A heading row with the file's name, then every schema of [`SCHEMAS`] (greyed while no asset is selected),
//! each listing every one of its standard properties whether the file has it or not: a name column and the value,
//! an edit box for the properties Encore edits, the text alone for the others, a check box for true or false, a
//! list for the rating, an opener for a structure. A property edited is written back into the file (or its sidecar)
//! when the box is left or Enter is pressed, as Encore does (`asset.setMetadata`). Schemas the panel does not know
//! follow, by their namespace, shown only. The search field keeps the properties whose name or value has the text.

use std::collections::HashMap;

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf, tr_ctx};
use disccraft_engine::metadata::MetadataGroup;
use disccraft_engine::xmp_schemas::{PropertyKind, PropertySpec, SCHEMAS, SchemaSpec};
use egui::{RichText, Ui, vec2};

/// The width of the name column (Encore's is about 170 pixels; longer names are cut with "…").
const NAME_WIDTH: f32 = 170.0;

/// A schema's name as the panel shows it.
fn schema_name(schema: &SchemaSpec) -> &'static str {
    tr_ctx("xmp", schema.name)
}

/// A property's name as the panel shows it (Encore's names are per schema: TIFF's Artist is 作者名, Dynamic Media's
/// アーティスト).
fn property_name(schema: &SchemaSpec, property: &PropertySpec) -> &'static str {
    tr_ctx(&format!("xmp:{}", schema.prefix), property.label)
}

/// A property's value in the file, as read (`None` when it has none), and the fields of a structure (path and
/// value) when it is one.
fn shown_value(
    group: Option<&MetadataGroup>,
    local: &str,
) -> (Option<String>, Vec<(String, String)>) {
    let Some(group) = group else {
        return (None, Vec::new());
    };
    let prefix = format!("{local} / ");
    let simple = group
        .items
        .iter()
        .find(|(name, _)| name == local)
        .map(|(_, value)| value.clone());
    let fields = group
        .items
        .iter()
        .filter_map(|(name, value)| {
            name.strip_prefix(&prefix)
                .map(|path| (path.to_owned(), value.clone()))
        })
        .collect();
    (simple, fields)
}

/// An XMP date (ISO 8601: `2026-11-10T21:44:09+09:00`, or its date alone) as Encore shows one: "11/10/26,
/// 21:44:09" in local time; anything else as it is.
pub(crate) fn display_date(value: &str) -> String {
    let text = value.trim();
    if let Ok(moment) = chrono::DateTime::parse_from_rfc3339(text) {
        return moment
            .with_timezone(&chrono::Local)
            .format("%m/%d/%y, %H:%M:%S")
            .to_string();
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(moment) = chrono::NaiveDateTime::parse_from_str(text, format) {
            return moment.format("%m/%d/%y, %H:%M:%S").to_string();
        }
    }
    if let Ok(day) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return day.format("%m/%d/%y").to_string();
    }
    value.to_owned()
}

/// A rating as the panel shows it: none, rejected, or stars.
pub(crate) fn rating_label(value: &str) -> String {
    match value.trim() {
        "" => tl!("None").to_owned(),
        "-1" => tl!("Rejected").to_owned(),
        stars => match stars.parse::<usize>() {
            Ok(0) => "☆".to_owned(),
            Ok(count @ 1..=5) => "★".repeat(count),
            _ => stars.to_owned(),
        },
    }
}

/// Lays out one name cell, `indent` pixels in, cut to the column.
fn name_cell(ui: &mut Ui, indent: f32, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.allocate_ui_with_layout(
        vec2(NAME_WIDTH, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add_space(indent);
            ui.add(
                egui::Label::new(text)
                    .truncate()
                    .sense(egui::Sense::click()),
            )
        },
    )
    .inner
}

impl DiscCraftApp {
    /// The Metadata panel (see the module's notes).
    pub(crate) fn metadata_panel(&mut self, ui: &mut Ui) {
        let asset = match self.selection {
            crate::app::Selection::Asset(id) => Some(id),
            crate::app::Selection::Timeline(id) => self
                .session
                .project
                .timeline(id)
                .map(|timeline| timeline.video),
            _ => None,
        };
        let found = asset.and_then(|id| {
            let asset = self.session.project.assets.get(&id)?;
            let path = asset.media_file().into_owned();
            let name = path.file_name().map_or_else(
                || asset.name.clone(),
                |name| name.to_string_lossy().into_owned(),
            );
            Some((id, name, path))
        });
        let _ = ui.add(
            egui::TextEdit::singleline(&mut self.metadata_search)
                .hint_text(tl!("Search"))
                .desired_width(f32::INFINITY),
        );
        // The heading row: the file's name, and the reload button and "XMP" on the right.
        let mut reload = false;
        ui.horizontal(|ui| {
            if let Some((_, name, _)) = &found {
                ui.strong(name);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak("XMP");
                if found.is_some() {
                    reload = ui
                        .small_button(tl!("Reload"))
                        .on_hover_text(tl!("Reads the file's metadata again"))
                        .clicked();
                }
            });
        });
        let Some((id, _, path)) = found else {
            // No asset: every schema's heading, greyed (as Encore shows them).
            ui.add_enabled_ui(false, |ui| {
                for schema in SCHEMAS {
                    egui::CollapsingHeader::new(RichText::new(schema_name(schema)).strong())
                        .id_salt(("xmp_none", schema.prefix))
                        .open(Some(false))
                        .show(ui, |_| {});
                }
            });
            return;
        };
        if reload {
            let _ = self.metadata_cache.remove(&id.0);
            self.metadata_draft = None;
        }
        let metadata =
            std::sync::Arc::clone(self.metadata_cache.entry(id.0).or_insert_with(|| {
                std::sync::Arc::new(disccraft_engine::metadata::file_metadata(&path))
            }));
        // The editable values, read once for the asset shown: (asset, as read, as edited).
        if self
            .metadata_draft
            .as_ref()
            .is_none_or(|(shown, _, _)| *shown != id.0)
        {
            let read: HashMap<String, String> =
                disccraft_engine::metadata_edit::editable_values(&path)
                    .into_iter()
                    .map(|(property, value)| (property.key(), value))
                    .collect();
            self.metadata_draft = Some((id.0, read.clone(), read));
        }
        if let Some(sidecar) = &metadata.sidecar {
            ui.weak(tlf!(
                "From the sidecar file {name}",
                name = sidecar
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
        }
        let wanted = self.metadata_search.trim().to_lowercase();
        let matches = |text: &str| wanted.is_empty() || text.to_lowercase().contains(&wanted);
        let mut commit: Option<(String, String)> = None;
        let Some((_, read, draft)) = self.metadata_draft.as_mut() else {
            return;
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for schema in SCHEMAS {
                    let group = metadata
                        .groups
                        .iter()
                        .find(|group| group.namespace == schema.namespace);
                    let rows: Vec<&PropertySpec> = schema
                        .properties
                        .iter()
                        .filter(|property| {
                            let key = format!("{}:{}", schema.prefix, property.local);
                            let (simple, fields) = shown_value(group, property.local);
                            matches(property_name(schema, property))
                                || draft.get(&key).is_some_and(|value| matches(value))
                                || simple.as_deref().is_some_and(matches)
                                || fields.iter().any(|(_, value)| matches(value))
                        })
                        .collect();
                    if !wanted.is_empty() && rows.is_empty() {
                        continue;
                    }
                    let title = if schema.faint {
                        RichText::new(schema_name(schema)).color(ui.visuals().weak_text_color())
                    } else {
                        RichText::new(schema_name(schema)).strong()
                    };
                    egui::CollapsingHeader::new(title)
                        .id_salt(("xmp", schema.prefix))
                        // A search opens what it finds.
                        .open((!wanted.is_empty()).then_some(true))
                        .show(ui, |ui| {
                            egui::Grid::new(("xmp_grid", schema.prefix))
                                .num_columns(2)
                                .spacing(vec2(6.0, 2.0))
                                .show(ui, |ui| {
                                    for property in rows {
                                        let key = format!("{}:{}", schema.prefix, property.local);
                                        let name = property_name(schema, property);
                                        let (simple, fields) = shown_value(group, property.local);
                                        if let Some(edit) = property_row(
                                            ui,
                                            (schema, property, name),
                                            (
                                                read.get(&key).cloned().unwrap_or_default(),
                                                draft.entry(key.clone()).or_default(),
                                            ),
                                            (simple, fields),
                                        ) {
                                            commit = Some((key, edit));
                                        }
                                    }
                                });
                        });
                }
                // Schemas the panel does not know: by their namespace, shown only.
                for group in metadata.groups.iter().filter(|group| {
                    !SCHEMAS
                        .iter()
                        .any(|schema| schema.namespace == group.namespace)
                }) {
                    let items: Vec<&(String, String)> = group
                        .items
                        .iter()
                        .filter(|(name, value)| matches(name) || matches(value))
                        .collect();
                    if items.is_empty() {
                        continue;
                    }
                    egui::CollapsingHeader::new(RichText::new(&group.name).strong())
                        .id_salt(("xmp_other", &group.namespace))
                        .open((!wanted.is_empty()).then_some(true))
                        .show(ui, |ui| {
                            egui::Grid::new(("xmp_other_grid", &group.namespace))
                                .num_columns(2)
                                .spacing(vec2(6.0, 2.0))
                                .show(ui, |ui| {
                                    for (name, value) in items {
                                        let _ = name_cell(ui, 12.0, name.as_str());
                                        ui.add(egui::Label::new(value).wrap().selectable(true));
                                        ui.end_row();
                                    }
                                });
                        });
                }
            });
        if let Some((key, value)) = commit {
            self.write_metadata_property(id.0, &key, value);
        }
    }

    /// Writes one property into asset `asset`'s file (or its sidecar) and reads the metadata again; on a failure the
    /// edit is taken back.
    fn write_metadata_property(&mut self, asset: u64, key: &str, value: String) {
        let mut fields = serde_json::Map::new();
        let _ = fields.insert(key.to_owned(), serde_json::Value::String(value));
        let result = self.run_query(
            "asset.setMetadata",
            serde_json::json!({ "asset": asset, "fields": fields }),
        );
        if let Some(result) = result {
            self.status =
                if result.get("written").and_then(serde_json::Value::as_str) == Some("file") {
                    tl!("Wrote the metadata into the file").to_owned()
                } else {
                    let name = result
                        .get("path")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|path| std::path::Path::new(path).file_name())
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    tlf!("Wrote the metadata to the sidecar {name}", name = name)
                };
        }
        let _ = self.metadata_cache.remove(&asset);
        self.metadata_draft = None;
    }
}

/// One property's row (and, for an open structure, its fields' rows): `value` is the edited value (`as read` beside
/// it), `shown` what the file has. Returns the new value of an edited property when it is to be written (the box
/// left or Enter pressed with a change, a check box or the rating changed).
fn property_row(
    ui: &mut Ui,
    (schema, property, name): (&SchemaSpec, &PropertySpec, &str),
    (as_read, value): (String, &mut String),
    (simple, fields): (Option<String>, Vec<(String, String)>),
) -> Option<String> {
    let mut commit = None;
    match property.kind {
        PropertyKind::Structure => {
            let id = ui.make_persistent_id(("xmp_open", schema.prefix, property.local));
            let open = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
            let arrow = if open { "▾" } else { "▸" };
            if name_cell(ui, 0.0, format!("{arrow} {name}")).clicked() {
                ui.data_mut(|data| data.insert_temp(id, !open));
            }
            ui.label(simple.unwrap_or_default());
            ui.end_row();
            if open {
                for (path, field) in fields {
                    let _ = name_cell(ui, 24.0, path);
                    ui.add(egui::Label::new(field).wrap().selectable(true));
                    ui.end_row();
                }
            }
            return None;
        }
        _ => {
            let _ = name_cell(ui, 12.0, name).on_hover_text(name);
        }
    }
    match property.kind {
        PropertyKind::Text
        | PropertyKind::LanguageAlternative
        | PropertyKind::Ordered
        | PropertyKind::Unordered => {
            let mut response =
                ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
            if matches!(
                property.kind,
                PropertyKind::Ordered | PropertyKind::Unordered
            ) {
                response = response.on_hover_text(tl!("Separate several with ;"));
            }
            if response.lost_focus() && *value != as_read {
                commit = Some(value.clone());
            }
        }
        PropertyKind::Rating => {
            let before = value.clone();
            egui::ComboBox::from_id_salt(("xmp_rating", schema.prefix))
                .selected_text(rating_label(value))
                .width(ui.available_width().max(60.0))
                .show_ui(ui, |ui| {
                    for choice in ["", "-1", "0", "1", "2", "3", "4", "5"] {
                        let _ = ui.selectable_value(value, choice.to_owned(), rating_label(choice));
                    }
                });
            if *value != before {
                commit = Some(value.clone());
            }
        }
        PropertyKind::Boolean => {
            let mut on = value.eq_ignore_ascii_case("true");
            let unset = value.trim().is_empty();
            if ui
                .add(egui::Checkbox::without_text(&mut on).indeterminate(unset))
                .clicked()
            {
                *value = if on { "True" } else { "False" }.to_owned();
                commit = Some(value.clone());
            }
        }
        PropertyKind::ShownBoolean => {
            let mut on = simple
                .as_deref()
                .is_some_and(|text| text.eq_ignore_ascii_case("true"));
            let unset = simple.is_none();
            let _ = ui.add_enabled(
                false,
                egui::Checkbox::without_text(&mut on).indeterminate(unset),
            );
        }
        PropertyKind::Shown | PropertyKind::Structure => {
            let text = simple.map(|text| display_date(&text)).unwrap_or_default();
            ui.add(egui::Label::new(text).truncate().selectable(true));
        }
    }
    ui.end_row();
    commit
}

#[cfg(test)]
mod tests {
    use super::{display_date, rating_label};
    use disccraft_engine::xmp_schemas::SCHEMAS;

    /// Every schema's name and every property's name has a Japanese name in the catalog (Encore's).
    #[test]
    fn every_schema_and_property_has_its_japanese_name() {
        crate::i18n::set_current(crate::i18n::Language::Ja);
        for schema in SCHEMAS {
            assert!(
                crate::i18n::has_contextual("xmp", schema.name),
                "{}",
                schema.name
            );
            for property in schema.properties {
                assert!(
                    crate::i18n::has_contextual(&format!("xmp:{}", schema.prefix), property.label),
                    "{}: {}",
                    schema.prefix,
                    property.label
                );
            }
        }
        assert_eq!(SCHEMAS.first().map(super::schema_name), Some("Dublin Core"));
        // Looked up per schema (the context `xmp:<prefix>`): Dynamic Media's Artist and TIFF's.
        let named = |prefix: &str| {
            let schema = SCHEMAS
                .iter()
                .find(|schema| schema.prefix == prefix)
                .unwrap();
            let artist = schema
                .properties
                .iter()
                .find(|property| property.local == "Artist" || property.local == "artist")
                .unwrap();
            super::property_name(schema, artist)
        };
        assert_eq!(
            (named("xmpDM"), named("tiff")),
            ("アーティスト", "アーティスト")
        );
    }

    #[test]
    fn dates_and_ratings_are_shown_as_encore_shows_them() {
        assert_eq!(display_date("2026-11-10T21:44:09"), "11/10/26, 21:44:09");
        assert_eq!(display_date("2004-01-01"), "01/01/04");
        assert_eq!(display_date("not a date"), "not a date");
        crate::i18n::set_current(crate::i18n::Language::Ja);
        assert_eq!(rating_label("3"), "★★★");
        assert_eq!(rating_label("0"), "☆");
        assert_eq!(rating_label("-1"), "不採用");
    }
}
