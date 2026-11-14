use serde_json::{Map, Value, json};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

struct RunResult {
    success: bool,
    stdout: String,
    stderr: String,
}

#[test]
fn flow() -> Result<(), Box<dyn Error>> {
    let dir = temp_dir("flow")?;
    let result = flow_inner(&dir);
    let _ = fs::remove_dir_all(&dir);
    result
}

#[test]
fn run_script_with_undo() -> Result<(), Box<dyn Error>> {
    let dir = temp_dir("run_script_with_undo")?;
    let result = run_script_with_undo_inner(&dir);
    let _ = fs::remove_dir_all(&dir);
    result
}

#[test]
fn error_exit() -> Result<(), Box<dyn Error>> {
    let dir = temp_dir("error_exit")?;
    let result = error_exit_inner(&dir);
    let _ = fs::remove_dir_all(&dir);
    result
}

/// The reader going away (`disccraft-cli commands | head`) ends the output, not the program: `println!` panicked
/// there (exit 101), and a script stopped at its first line without saving.
#[test]
fn a_closed_output_is_not_a_crash() -> Result<(), Box<dyn Error>> {
    let closed = |args: &[&str]| -> Result<process::Output, Box<dyn Error>> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_disccraft-cli"))
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        // Close the reading end before the program writes.
        drop(child.stdout.take());
        Ok(child.wait_with_output()?)
    };
    let listed = closed(&["commands"])?;
    let stderr = String::from_utf8_lossy(&listed.stderr);
    assert!(listed.status.success(), "{:?}: {stderr}", listed.status);
    assert!(!stderr.contains("panicked"), "{stderr}");

    // A script runs to the end and is saved though nobody reads its output.
    let dir = temp_dir("closed_output")?;
    let project = dir.join("closed.dcproj");
    let script = dir.join("script.jsonl");
    fs::write(
        &script,
        "{\"command\": \"menu.create\", \"params\": {\"name\": \"A\"}}
{\"command\": \"menu.create\", \"params\": {\"name\": \"B\"}}
",
    )?;
    let ran = closed(&["run", &path_arg(&project), &path_arg(&script)])?;
    let stderr = String::from_utf8_lossy(&ran.stderr);
    let saved = fs::read_to_string(&project).unwrap_or_default();
    let _ = fs::remove_dir_all(&dir);
    assert!(ran.status.success(), "{:?}: {stderr}", ran.status);
    assert!(saved.contains("\"B\""), "the second command was not saved");
    Ok(())
}

#[test]
fn commands_json() -> Result<(), Box<dyn Error>> {
    let result = run_cli(&["commands", "--json"])?;
    assert_success(&result, "commands --json")?;
    assert_contains(&result.stdout, "timeline.addChapter", "commands json")?;
    Ok(())
}

fn flow_inner(dir: &Path) -> Result<(), Box<dyn Error>> {
    let movie = dir.join("movie.mp4");
    fs::write(&movie, b"fake")?;

    let project = dir.join("movie.dcproj");
    let project_arg = path_arg(&project);
    let movie_arg = path_arg(&movie);

    let import_params = json!({ "path": movie_arg }).to_string();

    let result = run_cli(&[
        "exec",
        project_arg.as_str(),
        "file.new",
        r#"{"kind":"dvd","standard":"ntsc"}"#,
    ])?;
    assert_success(&result, "file.new")?;

    let result = run_cli(&[
        "exec",
        project_arg.as_str(),
        "asset.import",
        import_params.as_str(),
    ])?;
    assert_success(&result, "asset.import")?;

    let result = run_cli(&[
        "exec",
        project_arg.as_str(),
        "timeline.create",
        r#"{"asset":1}"#,
    ])?;
    assert_success(&result, "timeline.create")?;

    let result = run_cli(&[
        "exec",
        project_arg.as_str(),
        "timeline.addChapter",
        r#"{"timeline":2,"timecode":"00:10:00;00","name":"第2章"}"#,
    ])?;
    assert_success(&result, "timeline.addChapter")?;

    let result = run_cli(&["inspect", project_arg.as_str(), "--json"])?;
    assert_success(&result, "inspect --json")?;

    let value: Value = serde_json::from_str(&result.stdout)?;
    let chapters =
        find_timeline_chapters(&value, 2).ok_or("timeline 2 not found in inspect JSON")?;

    if chapters.len() != 2 {
        return Err(format!("expected 2 chapters, got {}", chapters.len()).into());
    }

    let second_name = chapters[1]
        .get("name")
        .and_then(Value::as_str)
        .ok_or("second chapter has no name")?;

    if second_name != "第2章" {
        return Err(format!("second chapter name is {second_name}").into());
    }

    let result = run_cli(&["inspect", project_arg.as_str()])?;
    assert_success(&result, "inspect")?;
    assert_contains(&result.stdout, "第2章", "inspect text")?;

    Ok(())
}

fn run_script_with_undo_inner(dir: &Path) -> Result<(), Box<dyn Error>> {
    let movie = dir.join("movie.mp4");
    fs::write(&movie, b"fake")?;

    let project = dir.join("movie.dcproj");
    let script = dir.join("script.jsonl");

    let project_arg = path_arg(&project);
    let script_arg = path_arg(&script);
    let movie_arg = path_arg(&movie);

    let new = json!({
        "command": "file.new",
        "params": { "kind": "dvd", "standard": "ntsc" }
    })
    .to_string();

    let import = json!({
        "command": "asset.import",
        "params": { "path": movie_arg }
    })
    .to_string();

    let create = json!({
        "command": "timeline.create",
        "params": { "asset": 1 }
    })
    .to_string();

    let undo = json!({ "command": "edit.undo" }).to_string();

    let content = format!("{new}\n{import}\n{create}\n{undo}\n");
    fs::write(&script, content)?;

    let result = run_cli(&["run", project_arg.as_str(), script_arg.as_str()])?;
    assert_success(&result, "run")?;

    let result = run_cli(&["inspect", project_arg.as_str(), "--json"])?;
    assert_success(&result, "inspect --json")?;

    let value: Value = serde_json::from_str(&result.stdout)?;

    if !find_asset(&value, 1) {
        return Err("asset 1 not found after undo".into());
    }

    if find_timeline(&value, 2) {
        return Err("timeline 2 still present after undo".into());
    }

    Ok(())
}

fn error_exit_inner(dir: &Path) -> Result<(), Box<dyn Error>> {
    let project = dir.join("missing.dcproj");
    let project_arg = path_arg(&project);

    let result = run_cli(&[
        "exec",
        project_arg.as_str(),
        "timeline.addChapter",
        r#"{"timeline":99,"time":1}"#,
    ])?;

    if result.success {
        return Err("expected failure".into());
    }

    if project.exists() {
        return Err("project file was created after failure".into());
    }

    Ok(())
}

fn temp_dir(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let mut path = env::temp_dir();
    path.push(format!("disccraft-cli-{}-{name}", process::id()));
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn run_cli(args: &[&str]) -> Result<RunResult, Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_disccraft-cli"))
        .args(args)
        .output()?;

    Ok(RunResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn assert_success(result: &RunResult, context: &str) -> Result<(), Box<dyn Error>> {
    if !result.success {
        return Err(format!("{context} failed: {}", result.stderr).into());
    }
    Ok(())
}

fn assert_contains(haystack: &str, needle: &str, context: &str) -> Result<(), Box<dyn Error>> {
    if !haystack.contains(needle) {
        return Err(format!("{context} missing {needle:?}: {haystack}").into());
    }
    Ok(())
}

fn id_matches(value: &Value, id: u64) -> bool {
    match value {
        Value::Number(number) => number.as_u64() == Some(id),
        Value::String(text) => text.parse::<u64>().ok() == Some(id),
        _ => false,
    }
}

fn is_timeline_object(obj: &Map<String, Value>) -> bool {
    obj.contains_key("chapters")
        || obj.contains_key("video")
        || obj.contains_key("endAction")
        || obj.contains_key("end_action")
}

fn is_asset_object(obj: &Map<String, Value>) -> bool {
    obj.contains_key("path") || obj.contains_key("kind") || obj.contains_key("offline")
}

fn find_timeline_chapters(value: &Value, id: u64) -> Option<&Vec<Value>> {
    if let Some(obj) = value.as_object() {
        if (obj.get("id").is_some_and(|v| id_matches(v, id))
            || obj.get("timeline").is_some_and(|v| id_matches(v, id)))
            && let Some(chapters) = obj.get("chapters").and_then(Value::as_array)
        {
            return Some(chapters);
        }

        if let Some(v) = obj.get(&id.to_string())
            && let Some(chapters) = v.get("chapters").and_then(Value::as_array)
        {
            return Some(chapters);
        }

        for v in obj.values() {
            if let Some(chapters) = find_timeline_chapters(v, id) {
                return Some(chapters);
            }
        }
    } else if let Some(arr) = value.as_array() {
        for v in arr {
            if let Some(chapters) = find_timeline_chapters(v, id) {
                return Some(chapters);
            }
        }
    }

    None
}

fn find_timeline(value: &Value, id: u64) -> bool {
    if let Some(obj) = value.as_object() {
        if (obj.get("id").is_some_and(|v| id_matches(v, id))
            || obj.get("timeline").is_some_and(|v| id_matches(v, id)))
            && is_timeline_object(obj)
        {
            return true;
        }

        if let Some(v) = obj.get(&id.to_string())
            && v.as_object().is_some_and(is_timeline_object)
        {
            return true;
        }

        for v in obj.values() {
            if find_timeline(v, id) {
                return true;
            }
        }
    } else if let Some(arr) = value.as_array() {
        for v in arr {
            if find_timeline(v, id) {
                return true;
            }
        }
    }

    false
}

fn find_asset(value: &Value, id: u64) -> bool {
    if let Some(obj) = value.as_object() {
        if (obj.get("id").is_some_and(|v| id_matches(v, id))
            || obj.get("asset").is_some_and(|v| id_matches(v, id)))
            && is_asset_object(obj)
        {
            return true;
        }

        if let Some(v) = obj.get(&id.to_string())
            && v.as_object().is_some_and(is_asset_object)
        {
            return true;
        }

        for v in obj.values() {
            if find_asset(v, id) {
                return true;
            }
        }
    } else if let Some(arr) = value.as_array() {
        for v in arr {
            if find_asset(v, id) {
                return true;
            }
        }
    }

    false
}
