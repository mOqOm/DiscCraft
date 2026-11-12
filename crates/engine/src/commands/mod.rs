use crate::{CommandSpec, Session};
use serde_json::{Map, Value};
use std::sync::LazyLock;

mod arrange;
mod asset;
mod build;
mod chapter_playlist;
mod disc;
mod edit;
mod file;
mod library;
mod lists;
mod menu;
mod menu_color_set;
mod menu_graphic;
mod menu_guide;
mod menu_options;
mod menu_orient;
mod menu_shadow;
mod menu_text;
mod nav;
mod preview;
pub(crate) use preview::Preview;
mod project;
mod subtitle;
mod timeline;

static ALL: LazyLock<Box<[CommandSpec]>> = LazyLock::new(build_all);

/// Returns all registered commands.
pub fn commands() -> &'static [CommandSpec] {
    &ALL
}

/// Finds a command by id.
pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    commands().iter().find(|spec| spec.id == id)
}

fn build_all() -> Box<[CommandSpec]> {
    let mut all = Vec::new();
    all.extend_from_slice(file::COMMANDS);
    all.extend_from_slice(asset::COMMANDS);
    all.extend_from_slice(timeline::COMMANDS);
    all.extend_from_slice(disc::COMMANDS);
    all.extend_from_slice(menu::COMMANDS);
    all.extend_from_slice(menu_color_set::COMMANDS);
    all.extend_from_slice(menu_options::COMMANDS);
    all.extend_from_slice(menu_text::COMMANDS);
    all.extend_from_slice(menu_shadow::COMMANDS);
    all.extend_from_slice(menu_orient::COMMANDS);
    all.extend_from_slice(menu_guide::COMMANDS);
    all.extend_from_slice(menu_graphic::COMMANDS);
    all.extend_from_slice(arrange::COMMANDS);
    all.extend_from_slice(lists::COMMANDS);
    all.extend_from_slice(chapter_playlist::COMMANDS);
    all.extend_from_slice(library::COMMANDS);
    all.extend_from_slice(nav::COMMANDS);
    all.extend_from_slice(preview::COMMANDS);
    all.extend_from_slice(project::COMMANDS);
    all.extend_from_slice(build::COMMANDS);
    all.extend_from_slice(edit::COMMANDS);
    all.extend_from_slice(subtitle::COMMANDS);
    all.into_boxed_slice()
}

pub(crate) fn always_enabled(_: &Session) -> Result<(), String> {
    Ok(())
}

pub(crate) fn empty_object() -> Value {
    Value::Object(Map::new())
}

pub(crate) fn object(entries: &[(&str, Value)]) -> Value {
    let mut map = Map::new();
    for (key, value) in entries {
        map.insert((*key).to_string(), (*value).clone());
    }
    Value::Object(map)
}

/// The "prohibited" parameter: a list of user operation names ("fastForward"), returned without repeats in UOP
/// order.
pub(crate) fn user_ops_param(
    params: &serde_json::Value,
) -> Result<Vec<disccraft_project::UserOp>, String> {
    let list = params
        .get("prohibited")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "parameter \"prohibited\" must be a list of operation names".to_string())?;
    let mut ops = Vec::with_capacity(list.len());
    for item in list {
        let name = item
            .as_str()
            .ok_or_else(|| "parameter \"prohibited\" must hold names".to_string())?;
        let op = disccraft_project::UserOp::parse(name)
            .ok_or_else(|| format!("unknown user operation {name:?}"))?;
        ops.push(op);
    }
    Ok(disccraft_project::normalized_ops(&ops))
}

#[cfg(test)]
mod tests {
    use super::{commands, find_command};
    use std::collections::HashSet;
    use std::error::Error;

    fn is_command_id(id: &str) -> bool {
        let Some((namespace, command)) = id.split_once('.') else {
            return false;
        };

        if namespace.is_empty() || command.is_empty() || command.contains('.') {
            return false;
        }

        if !namespace.chars().all(|ch| ch.is_ascii_lowercase()) {
            return false;
        }

        let mut chars = command.chars();
        let Some(first) = chars.next() else {
            return false;
        };

        first.is_ascii_lowercase() && chars.all(|ch| ch.is_ascii_alphanumeric())
    }

    #[test]
    fn ids_are_unique() -> Result<(), Box<dyn Error>> {
        let mut seen = HashSet::new();
        for spec in commands() {
            if !seen.insert(spec.id) {
                return Err(format!("duplicate command id: {}", spec.id).into());
            }
        }
        Ok(())
    }

    #[test]
    fn ids_match_namespace_and_camel_case() -> Result<(), Box<dyn Error>> {
        for spec in commands() {
            if !is_command_id(spec.id) {
                return Err(format!("bad command id: {}", spec.id).into());
            }
            if find_command(spec.id).is_none() {
                return Err(format!("find_command failed for {}", spec.id).into());
            }
        }
        assert!(find_command("missing.command").is_none());
        Ok(())
    }

    #[test]
    fn journal_labels_are_non_empty() -> Result<(), Box<dyn Error>> {
        for spec in commands() {
            if spec.journal && spec.label.is_empty() {
                return Err(format!("empty label for {}", spec.id).into());
            }
        }
        Ok(())
    }
}
