//! The disc label as a DesignCraft document (M6.507): the same label as the PSD of [`super`], made by DesignCraft's
//! command line from a script of its commands, so that its text is type DesignCraft sets (sharp at any printer's
//! resolution) and its shapes are paths, in points.
//!
//! The page is the disc, 120 mm square. On its first layer (named after the disc's face): the printable ring as one
//! path of two circles (the inner one a hole), the picture placed in it (filling it, cut by it: nothing printed on the
//! hole or past the edge), the title in a frame across the upper half and the chapters in a frame in the lower half,
//! in the font DiscCraft sets menus in. Over it, a layer of guides that does not print: the disc's edge, the printable
//! ring and the centre hole. The chapters are packed on lines as for the PSD; when DesignCraft's own setting still
//! leaves some out (its story is overset), the script is run again a half point smaller.
//!
//! The commands and their parameters are those `designcraft-cli describe` gives; the script's form is that of
//! `designcraft-cli script --help` (a command and its JSON parameters a line, `$N.path` for an earlier step's result).

use super::{
    DISC_MM, DiscLabel, HOLE_MM, LINES_POINTS, TITLE_POINTS, label_text, pack_lines, ring_mm,
};
use crate::{MenuError, draw};
use disccraft_project::Menu;
use serde_json::{Value, json};
use std::path::Path;

/// Points a millimetre.
const PT_PER_MM: f32 = 72.0 / 25.4;
/// The resolution the lines are measured at (DiscCraft's own text layout), pixels an inch.
const MEASURE_DPI: f32 = 300.0;
/// The guides' colour: a swatch every DesignCraft document has.
const GUIDE_SWATCH: &str = "C=100 M=0 Y=0 K=0";
/// The guides' stroke, points.
const GUIDE_WEIGHT: f32 = 0.5;

/// What the document came to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Made {
    /// The chapters' size, points (`None`: no chapters).
    pub chapters_points: Option<f32>,
    /// DesignCraft left some chapters out even at the smallest size.
    pub chapters_overset: bool,
}

/// A DesignCraft script, a step a line.
#[derive(Default)]
struct Script {
    steps: Vec<String>,
}

impl Script {
    /// Adds `command` with `params`; its step number.
    fn step(&mut self, command: &str, params: &Value) -> usize {
        self.steps.push(format!("{command} {params}"));
        self.steps.len() - 1
    }

    fn text(&self) -> String {
        let mut text = self.steps.join("\n");
        text.push('\n');
        text
    }
}

/// `$step.path`: a value of an earlier step's result.
fn earlier(step: usize, path: &str) -> Value {
    Value::String(format!("${step}.{path}"))
}

/// `x` rounded to a thousandth of a point (the script stays short and reproducible).
fn pt(x: f32) -> f64 {
    (f64::from(x) * 1000.0).round() / 1000.0
}

/// A square of `radius` points about the page's centre `c`, as `[x0, y0, x1, y1]`.
fn about(c: f32, radius: f32) -> Value {
    json!([
        pt(c - radius),
        pt(c - radius),
        pt(c + radius),
        pt(c + radius)
    ])
}

/// What a label's script sets: the font, the title's size, the chapters' lines and size, and the picture's file.
pub(crate) struct Setting<'a> {
    pub(crate) family: &'a str,
    pub(crate) title_points: f32,
    pub(crate) chapters: Option<(String, f32)>,
    pub(crate) picture: Option<&'a Path>,
}

/// The script that makes `label`'s document, and the step that makes the chapters' frame (its result names the
/// chapters' story).
pub(crate) fn script(label: &DiscLabel, setting: &Setting<'_>) -> (String, Option<usize>) {
    let side = DISC_MM * PT_PER_MM;
    let c = side / 2.0;
    let (inner_mm, outer_mm) = ring_mm(label);
    let (inner, outer) = (inner_mm / 2.0 * PT_PER_MM, outer_mm / 2.0 * PT_PER_MM);
    let mut script = Script::default();
    let _ = script.step(
        "file.new",
        &json!({
            "units": "points", "width": pt(side), "height": pt(side), "pages": 1, "margins": 0, "bleed": 0,
            "intent": "print", "facingPages": false, "title": label.title.trim(),
        }),
    );
    // The face: two circles made one path, the inner one a hole, with the picture in it.
    let outside = script.step(
        "frame.create",
        &json!({"rect": about(c, outer), "shape": "ellipse", "content": "graphic"}),
    );
    let hole = script.step(
        "frame.create",
        &json!({"rect": about(c, inner), "shape": "ellipse", "content": "graphic"}),
    );
    let face = script.step(
        "object.makeCompoundPath",
        &json!({"ids": [earlier(outside, "id"), earlier(hole, "id")]}),
    );
    if let Some(picture) = setting.picture {
        let _ = script.step(
            "file.place",
            &json!({"path": picture.to_string_lossy(), "embed": true, "frame": earlier(face, "id")}),
        );
        let _ = script.step(
            "object.fit",
            &json!({"mode": "fillProportionally", "ids": [earlier(face, "id")]}),
        );
    }
    let _ = script.step("layer.set", &json!({"id": 1, "name": label.names.face}));
    let color = if setting.picture.is_some() {
        "[Paper]"
    } else {
        "[Black]"
    };
    let text = |script: &mut Script, rect: Value, body: &str, points: f32, middle: bool| {
        let frame = script.step(
            "frame.create",
            &json!({"rect": rect, "content": "text", "text": body, "caret": true}),
        );
        let _ = script.step("edit.selectAll", &Value::Null);
        let _ = script.step(
            "type.char",
            &json!({"attrs": {"fontFamily": setting.family, "size": pt(points), "fill": color}}),
        );
        let _ = script.step("type.align", &json!({"align": "center"}));
        let _ = script.step(
            "object.textFrameOptions",
            &json!({
                "verticalJustification": if middle { "center" } else { "top" },
                "inset": 0,
                "ids": [earlier(frame, "id")],
            }),
        );
        frame
    };
    // The title: across the middle of the ring's upper half.
    let title = label.title.trim();
    if !title.is_empty() {
        let (half, middle) = (outer * 0.7, c - (inner + outer) / 2.0);
        let height = setting.title_points * 3.0;
        let _ = text(
            &mut script,
            json!([
                pt(c - half),
                pt(middle - height / 2.0),
                pt(c + half),
                pt(middle + height / 2.0)
            ]),
            title,
            setting.title_points,
            true,
        );
    }
    // The chapters: from just under the hole's ring to where the ring is still 1.3 outer radii across.
    let chapters = setting.chapters.as_ref().map(|(lines, points)| {
        let (half, top, bottom) = (outer * 0.65, c + inner + 2.0 * PT_PER_MM, c + outer * 0.75);
        text(
            &mut script,
            json!([pt(c - half), pt(top), pt(c + half), pt(bottom)]),
            lines,
            *points,
            false,
        )
    });
    // The guides, on a layer that does not print.
    let guides = script.step("layer.new", &json!({"name": label.names.guides}));
    let _ = script.step(
        "layer.set",
        &json!({"id": earlier(guides, "id"), "printable": false}),
    );
    let _ = script.step("layer.activate", &json!({"id": earlier(guides, "id")}));
    for radius in [c - GUIDE_WEIGHT, outer, inner, HOLE_MM / 2.0 * PT_PER_MM] {
        let circle = script.step(
            "frame.create",
            &json!({"rect": about(c, radius), "shape": "ellipse", "content": "unassigned"}),
        );
        let _ = script.step(
            "object.stroke",
            &json!({"swatch": GUIDE_SWATCH, "weight": GUIDE_WEIGHT, "ids": [earlier(circle, "id")]}),
        );
    }
    // M6.508: what is added later (a picture pasted in) goes on the face, which prints, not on the guides: the guides'
    // layer is locked and the face's is the one in use when the document opens.
    let _ = script.step(
        "layer.set",
        &json!({"id": earlier(guides, "id"), "locked": true}),
    );
    let _ = script.step("layer.activate", &json!({"id": 1}));
    let _ = script.step("document.inspect", &Value::Null);
    (script.text(), chapters)
}

/// The largest title size up to [`TITLE_POINTS`] at which `title` is one line across the ring's upper half.
fn title_points(menu: &Menu, title: &str, outer_mm: f32) -> f32 {
    let width = outer_mm / 2.0 * PT_PER_MM * 1.4 * MEASURE_DPI / 72.0;
    let mut points = TITLE_POINTS;
    while points > 8.0 {
        let mut text = label_text(
            title,
            points,
            MEASURE_DPI,
            (0.0, 0.0),
            (width, None),
            [0; 3],
        );
        text.box_width = None;
        if f32::from(draw::text_rect(menu, &text).w) <= width {
            break;
        }
        points -= 0.5;
    }
    points
}

/// The size the chapters start at: the largest at which DiscCraft's own setting fits them in their frame.
fn chapters_points(menu: &Menu, entries: &[&str], (inner_mm, outer_mm): (f32, f32)) -> f32 {
    let scale = MEASURE_DPI / 72.0;
    let (inner, outer) = (inner_mm / 2.0 * PT_PER_MM, outer_mm / 2.0 * PT_PER_MM);
    let width = outer * 1.3 * scale;
    let height = (outer * 0.75 - inner - 2.0 * PT_PER_MM) * scale;
    let mut points = LINES_POINTS.0;
    while points > LINES_POINTS.1 {
        let lines = pack_lines(menu, entries, (points, MEASURE_DPI), width).join("\n");
        let text = label_text(
            &lines,
            points,
            MEASURE_DPI,
            (0.0, 0.0),
            (width, Some(height)),
            [0; 3],
        );
        if !draw::text_overset(menu, &text) {
            break;
        }
        points -= 0.5;
    }
    points
}

/// Whether the story the chapters' frame (the result of step `step`) holds is overset, in the result of a run.
fn overset(result: &Value, step: usize) -> bool {
    let results = result.get("results").and_then(Value::as_array);
    let story = results
        .and_then(|results| results.get(step))
        .and_then(|frame| frame.get("story"))
        .cloned();
    let inspected = results.and_then(|results| results.last());
    inspected
        .and_then(|inspect| inspect.get("stories"))
        .and_then(Value::as_array)
        .is_some_and(|stories| {
            stories.iter().any(|found| {
                story.as_ref().is_some_and(|id| found.get("id") == Some(id))
                    && found.get("overset").and_then(Value::as_bool) == Some(true)
            })
        })
}

/// Runs `script` with DesignCraft's command line `cli`, saving the document at `out`: its result.
fn run(cli: &Path, script: &str, out: &Path) -> Result<Value, MenuError> {
    let file = out.with_extension("designcraft-script.txt");
    std::fs::write(&file, script)
        .map_err(|error| MenuError::Invalid(format!("{}: {error}", file.display())))?;
    let mut command = std::process::Command::new(cli);
    let _ = command.arg("script").arg(&file).arg("--save").arg(out);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console window flashes up beside the GUI.
        let _ = command.creation_flags(0x0800_0000);
    }
    let output = command.output();
    let _ = std::fs::remove_file(&file);
    let output =
        output.map_err(|error| MenuError::Invalid(format!("{}: {error}", cli.display())))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result = serde_json::Deserializer::from_str(&stdout)
        .into_iter::<Value>()
        .next()
        .and_then(Result::ok);
    match result {
        Some(result) if output.status.success() && out.is_file() => Ok(result),
        _ => Err(MenuError::Invalid(format!(
            "DesignCraft: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

/// DesignCraft's command line (`designcraft-cli`): named by `DISCCRAFT_DESIGNCRAFT_CLI`, beside the app named by
/// `DISCCRAFT_DESIGNCRAFT`, in the Craft apps' install folder (`ArtCraft\DesignCraft`), or on the PATH.
pub fn find_cli() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let exe = format!("designcraft-cli{}", std::env::consts::EXE_SUFFIX);
    let named = std::env::var_os("DISCCRAFT_DESIGNCRAFT_CLI").map(PathBuf::from);
    let beside = std::env::var_os("DISCCRAFT_DESIGNCRAFT")
        .map(PathBuf::from)
        .and_then(|app| app.parent().map(|folder| folder.join(&exe)));
    let mut candidates: Vec<PathBuf> = named.into_iter().chain(beside).collect();
    for var in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var).map(PathBuf::from) {
            candidates.push(base.join("ArtCraft").join("DesignCraft").join(&exe));
            candidates.push(
                base.join("Programs")
                    .join("ArtCraft")
                    .join("DesignCraft")
                    .join(&exe),
            );
        }
    }
    candidates.push(PathBuf::from("/Applications/DesignCraft.app/Contents/MacOS").join(&exe));
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths).map(|dir| dir.join(&exe)));
    }
    candidates.into_iter().find(|path| path.is_file())
}

/// Makes `label` as a DesignCraft document at `out` with DesignCraft's command line `cli` (see the module).
///
/// # Errors
///
/// When the picture cannot be written, DesignCraft cannot be run or its script fails.
pub fn make(label: &DiscLabel, cli: &Path, out: &Path) -> Result<Made, MenuError> {
    let family = draw::default_family();
    let menu = Menu::default();
    let ring = ring_mm(label);
    // The picture in square pixels, as it is shown, beside the document while DesignCraft takes it in.
    let picture = match &label.picture {
        Some(picture) => {
            let file = out.with_extension("picture.png");
            std::fs::write(&file, super::picture_png(picture)?)
                .map_err(|error| MenuError::Invalid(format!("{}: {error}", file.display())))?;
            Some(file)
        }
        None => None,
    };
    let entries: Vec<&str> = label
        .lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    let title = title_points(&menu, label.title.trim(), ring.1);
    let mut points = (!entries.is_empty()).then(|| chapters_points(&menu, &entries, ring));
    let made = loop {
        let width = ring.1 / 2.0 * PT_PER_MM * 1.3 * MEASURE_DPI / 72.0;
        let chapters = points.map(|points| {
            (
                pack_lines(&menu, &entries, (points, MEASURE_DPI), width).join("\u{2028}"),
                points,
            )
        });
        let (text, step) = script(
            label,
            &Setting {
                family: &family,
                title_points: title,
                chapters,
                picture: picture.as_deref(),
            },
        );
        let result = run(cli, &text, out);
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if let Some(file) = &picture {
                    let _ = std::fs::remove_file(file);
                }
                return Err(error);
            }
        };
        let left_out = step.is_some_and(|step| overset(&result, step));
        match points {
            Some(size) if left_out && size > LINES_POINTS.1 => points = Some(size - 0.5),
            _ => {
                break Made {
                    chapters_points: points,
                    chapters_overset: left_out,
                };
            }
        }
    };
    if let Some(file) = &picture {
        let _ = std::fs::remove_file(file);
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::{Setting, overset, script};
    use crate::label::{DiscLabel, LabelNames};
    use serde_json::json;
    use std::path::Path;

    fn label() -> DiscLabel {
        DiscLabel {
            inner_mm: 22.0,
            outer_mm: 118.0,
            dpi: 300.0,
            title: "Summer Trip 2026".to_owned(),
            lines: Vec::new(),
            picture: None,
            names: LabelNames {
                face: "Disc face".to_owned(),
                picture: "Picture".to_owned(),
                guides: "Guides".to_owned(),
                lines: "Chapters".to_owned(),
            },
        }
    }

    /// M6.507: the script makes a 120 mm page in points, the ring as one path of two circles with the picture in
    /// it, the texts in the font given, and the guides on a layer that does not print; its steps refer to the
    /// results of the steps before them.
    #[test]
    fn the_script_makes_the_disc_in_points() {
        let (text, chapters) = script(
            &label(),
            &Setting {
                family: "Yu Gothic UI",
                title_points: 16.0,
                chapters: Some(("1. One\u{2028}2. Two".to_owned(), 7.0)),
                picture: Some(Path::new("C:/pictures/top.png")),
            },
        );
        let steps: Vec<&str> = text.lines().collect();
        // A step's parameters.
        let params = |step: &str| -> serde_json::Value {
            step.split_once(' ')
                .and_then(|(_, json)| serde_json::from_str(json).ok())
                .unwrap_or_default()
        };
        let near = |value: &serde_json::Value, wanted: f64| {
            value
                .as_f64()
                .is_some_and(|value| (value - wanted).abs() < 0.01)
        };
        assert!(steps[0].starts_with("file.new "), "{}", steps[0]);
        // 120 mm, and the ring's circles of 118 mm and 22 mm about the centre, in points.
        assert!(near(&params(steps[0])["width"], 340.157), "{}", steps[0]);
        let mm = 72.0 / 25.4;
        for (step, radius) in [(1, 59.0 * mm), (2, 11.0 * mm)] {
            let rect = &params(steps[step])["rect"];
            assert!(near(&rect[0], 340.157 / 2.0 - radius), "{}", steps[step]);
            assert!(near(&rect[3], 340.157 / 2.0 + radius), "{}", steps[step]);
        }
        assert_eq!(
            steps[3],
            "object.makeCompoundPath {\"ids\":[\"$1.id\",\"$2.id\"]}"
        );
        assert!(steps[4].starts_with("file.place ") && steps[4].contains("\"frame\":\"$3.id\""));
        assert!(text.contains("\"fontFamily\":\"Yu Gothic UI\""));
        assert!(text.contains("\"fill\":\"[Paper]\""));
        let chapters = chapters.unwrap_or_default();
        assert!(steps[chapters].starts_with("frame.create ") && steps[chapters].contains("1. One"));
        assert!(text.contains("\"printable\":false"));
        assert_eq!(steps.last(), Some(&"document.inspect null"));
        // M6.508: the guides' layer ends locked and the face's in use (a picture pasted in later prints).
        let count = steps.len();
        assert!(
            steps[count - 3].starts_with("layer.set ")
                && steps[count - 3].contains("\"locked\":true"),
            "{}",
            steps[count - 3]
        );
        assert_eq!(steps[count - 2], "layer.activate {\"id\":1}");
        // Without a picture: no place, dark text.
        let (plain, _) = script(
            &label(),
            &Setting {
                family: "Yu Gothic UI",
                title_points: 16.0,
                chapters: None,
                picture: None,
            },
        );
        assert!(!plain.contains("file.place"));
        assert!(plain.contains("\"fill\":\"[Black]\""));
    }

    /// M6.507: with DesignCraft installed (else the test has nothing to run), the label is made as a document: its
    /// chapters fit, at a size within the range, and the picture's file made for it is gone again.
    #[test]
    fn designcraft_makes_the_label() -> Result<(), Box<dyn std::error::Error>> {
        let Some(cli) = super::find_cli() else {
            return Ok(());
        };
        let dir = std::env::temp_dir().join(format!("disccraft-label-dc-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let out = dir.join("label.designcraft");
        let made = super::make(
            &DiscLabel {
                lines: (1..=30).map(|n| format!("{n}. Chapter name {n}")).collect(),
                picture: Some(([40u8, 80, 160].repeat(32 * 18), 32, 18, 16.0 / 9.0)),
                ..label()
            },
            &cli,
            &out,
        )?;
        assert!(out.is_file());
        assert!(!made.chapters_overset, "{made:?}");
        assert!(
            made.chapters_points
                .is_some_and(|points| (4.5..=8.0).contains(&points))
        );
        assert!(!dir.join("label.picture.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// M6.507: the chapters' story is found by the story its frame's step made, in the last step's inspection.
    #[test]
    fn an_overset_chapters_story_is_found() {
        let result = json!({"results": [
            {"index": 1},
            {"id": 10, "story": 11},
            {"stories": [{"id": 11, "overset": true}, {"id": 12, "overset": false}]},
        ]});
        assert!(overset(&result, 1));
        let fits = json!({"results": [
            {"index": 1},
            {"id": 10, "story": 12},
            {"stories": [{"id": 11, "overset": true}, {"id": 12, "overset": false}]},
        ]});
        assert!(!overset(&fits, 1));
        assert!(!overset(&json!({}), 1));
    }
}
