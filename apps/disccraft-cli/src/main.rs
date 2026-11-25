//! DiscCraft headless CLI (L6).
//!
//! Provides non-interactive access to the DiscCraft editing engine:
//! command discovery, single command execution, scripted execution, and
//! project inspection.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod bd_chapters;
mod inspect;

use clap::{Parser, Subcommand};
use disccraft_engine::{Session, commands};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// M6.500: mimalloc as the allocator. A transcode asks for pictures of megabytes many times a second; the system's
/// allocator gave each fresh pages (half a million page faults a second), on which several transcodes side by side
/// waited for one another. mimalloc reuses its own pages: five clips at once went 1.8 times as fast.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const MAX_SCRIPT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Parser)]
#[command(name = "disccraft-cli", version, about = "DiscCraft headless CLI")]
struct Cli {
    #[command(subcommand)]
    command: Sub,
}

#[derive(Subcommand)]
enum Sub {
    Commands {
        #[arg(long)]
        json: bool,
    },
    Exec {
        project: PathBuf,
        command: String,
        params: Option<String>,
    },
    Run {
        project: PathBuf,
        script: PathBuf,
    },
    Inspect {
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Run the MCP server (JSON-RPC over stdin / stdout) for AI agents
    Mcp,
    Build {
        project: PathBuf,
        /// Output folder (VIDEO_TS is written inside it)
        #[arg(long, required_unless_present_any = ["iso", "ddp"])]
        out: Option<PathBuf>,
        /// Write a DVD ISO image instead of a folder
        #[arg(long)]
        iso: Option<PathBuf>,
        /// Write a DVD's DDP image for replication into this folder (MAIN.DAT, CONTROL.DAT, DDPID)
        #[arg(long, conflicts_with = "iso")]
        ddp: Option<PathBuf>,
        /// Keep transcoded media in this folder (shared between builds) instead of next to the output
        #[arg(long)]
        cache: Option<PathBuf>,
        /// Language (ISO 639-1, e.g. ja) of audio without its own when the project names none; else the
        /// system's language
        #[arg(long)]
        audio_language: Option<String>,
    },
    /// List the disc recorders and the discs in them (M9.35)
    Recorders,
    /// Write a disc image (a DVD or Blu-ray ISO that `build --iso` makes) to the disc in a recorder (M9.35)
    Burn {
        image: PathBuf,
        /// The recorder: its id or drive letter as `recorders` lists them (default: the first)
        #[arg(long)]
        recorder: Option<String>,
        /// Write speed as a multiple of 1x (default: the fastest)
        #[arg(long)]
        speed: Option<f64>,
        /// Erase (or write over) a rewritable disc with data on it
        #[arg(long)]
        erase: bool,
        /// Do not read the disc back to compare it with the image after writing (M9.38)
        #[arg(long)]
        no_verify: bool,
    },
    /// List a Blu-ray's playlists and print the chapters of the main one (M9.41): a disc in a drive (`D:`), a
    /// disc's folder, its BDMV or PLAYLIST folder, or one .mpls file. Encrypted discs are not read
    BdChapters {
        path: PathBuf,
        /// The playlist whose chapters are printed (1, 00001 or 00001.mpls; default: the longest)
        #[arg(long, conflicts_with = "all")]
        playlist: Option<String>,
        /// The chapters of every playlist that has any
        #[arg(long)]
        all: bool,
        /// Print JSON (every playlist and its chapters in seconds)
        #[arg(long, conflicts_with = "ogm")]
        json: bool,
        /// Print the chapters as an OGM chapter file (CHAPTER01=00:00:00.000 / CHAPTER01NAME=...)
        #[arg(long)]
        ogm: bool,
    },
    /// Read the disc in a recorder back and compare it with a disc image, sector by sector (M9.38)
    Verify {
        image: PathBuf,
        /// The recorder: its id or drive letter as `recorders` lists them (default: the first)
        #[arg(long)]
        recorder: Option<String>,
    },
    /// Print what the drive tells of the disc in a recorder: its status, sessions and tracks (reserved, blank, next
    /// writable address, size) and a dual-layer disc's layers (M9.45). Nothing is written
    DiscInfo {
        /// The recorder: its id or drive letter as `recorders` lists them (default: the first)
        #[arg(long)]
        recorder: Option<String>,
    },
    /// Make the disc's label (M6.506, M6.507): a DesignCraft document of the disc's face, 120 mm square, to print on
    /// a printable disc. A menu's background filling the printable ring (nothing is printed on the hole or past the
    /// edge), the disc's name and its named chapters as type, and guides that do not print. An --out ending in .psd
    /// writes a PSD for PhotoCraft instead (the ring as a layer mask)
    DiscLabel {
        project: PathBuf,
        /// The document to write: .designcraft (made by DesignCraft's command line), or .psd
        #[arg(long)]
        out: PathBuf,
        /// The printable ring's inner diameter in mm, as the disc's package says (a "wide printable" disc 22,
        /// others 43)
        #[arg(long, default_value_t = 22.0)]
        inner: f32,
        /// The printable ring's outer diameter in mm
        #[arg(long, default_value_t = 118.0)]
        outer: f32,
        /// Pixels an inch of a PSD (150 to 600)
        #[arg(long, default_value_t = 300.0)]
        dpi: f32,
        /// The menu whose background is the picture, by its name (default: the menu the disc plays first); `none`
        /// for a white disc
        #[arg(long)]
        menu: Option<String>,
        /// The title across the upper half (default: the disc's name; none without one)
        #[arg(long)]
        title: Option<String>,
        /// Leave the chapters out
        #[arg(long)]
        no_chapters: bool,
        /// Link the label to the project and save the project (M6.509: as the application does, which opens it
        /// again with File ▸ Open Disc Label)
        #[arg(long)]
        link: bool,
    },
}

fn main() -> ExitCode {
    // M12.40: DISCCRAFT_HARDWARE_ENCODER=1 encodes HD H.264 on the system's hardware encoder (as the application's
    // Preferences ▸ Memory does), falling back to the CPU.
    if std::env::var_os("DISCCRAFT_HARDWARE_ENCODER").is_some_and(|value| value == "1") {
        disccraft_platform::use_for_transcodes(true);
    }
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            use std::io::Write;
            // Like the output, a closed error stream is not a reason to panic.
            let _ = writeln!(std::io::stderr(), "error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Sub::Commands { json } => list_commands(json),
        Sub::Exec {
            project,
            command,
            params,
        } => exec_command(&project, &command, params.as_deref()),
        Sub::Run { project, script } => run_script(&project, &script),
        Sub::Inspect { project, json } => inspect_project(&project, json),
        Sub::Mcp => disccraft_automation::McpServer::new()
            .serve(std::io::stdin().lock(), std::io::stdout().lock())
            .map_err(|e| format!("mcp: {e}")),
        Sub::Build {
            project,
            out,
            iso,
            ddp,
            cache,
            audio_language,
        } => {
            let (path, output) = match (iso, ddp, out) {
                (Some(iso), _, _) => (iso, "iso"),
                (None, Some(ddp), _) => (ddp, "ddp"),
                (None, None, Some(out)) => (out, "folder"),
                (None, None, None) => return Err("--out, --iso or --ddp is required".to_string()),
            };
            build_project(
                &project,
                &path,
                output,
                (cache.as_deref(), audio_language.as_deref()),
            )
        }
        Sub::Recorders => list_recorders(),
        Sub::Burn {
            image,
            recorder,
            speed,
            erase,
            no_verify,
        } => burn_image(&image, recorder.as_deref(), speed, (erase, !no_verify)),
        Sub::Verify { image, recorder } => verify_disc(&image, recorder.as_deref()),
        Sub::DiscInfo { recorder } => disc_info(recorder.as_deref()),
        Sub::DiscLabel {
            project,
            out,
            inner,
            outer,
            dpi,
            menu,
            title,
            no_chapters,
            link,
        } => disc_label(
            &project,
            &out,
            (inner, outer, dpi),
            (menu.as_deref(), title.as_deref()),
            (!no_chapters, link),
        ),
        Sub::BdChapters {
            path,
            playlist,
            all,
            json,
            ogm,
        } => bd_chapters::run(
            &path,
            &bd_chapters::Options {
                playlist,
                all,
                json,
                ogm,
            },
        ),
    }
}

/// The recorder chosen by id or drive letter, else the first.
fn find_recorder(wanted: Option<&str>) -> Result<disccraft_platform::burn::Recorder, String> {
    use disccraft_platform::burn;
    let recorders = burn::recorders().map_err(|error| error.to_string())?;
    match wanted {
        None => recorders.first(),
        Some(wanted) => recorders.iter().find(|recorder| {
            recorder.id == wanted
                || recorder.drives.iter().any(|drive| {
                    drive
                        .trim_end_matches(['\\', ':'])
                        .eq_ignore_ascii_case(wanted.trim_end_matches(['\\', ':']))
                })
        }),
    }
    .cloned()
    .ok_or_else(|| "no such recorder (see `disccraft-cli recorders`)".to_string())
}

/// Reads the disc in the recorder back and compares it with `image` (M9.38), with the progress on standard error.
fn verify_disc(image: &Path, wanted: Option<&str>) -> Result<(), String> {
    use disccraft_platform::burn;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    let recorder = find_recorder(wanted)?;
    let progress: Arc<dyn Fn(f32) + Send + Sync> = Arc::new(|fraction| {
        use std::io::Write;
        // `as` saturates; the fraction is within 0..=1.
        let _ = write!(
            std::io::stderr(),
            "\rverifying {:3}%",
            (fraction * 100.0).round() as u32
        );
    });
    let result = burn::verify(
        &recorder.id,
        image,
        progress,
        Arc::new(AtomicBool::new(false)),
    );
    eprintln!();
    result.map_err(|error| error.to_string())?;
    println!("the disc in {} is the same as the image", recorder.label());
    Ok(())
}

/// Writes the project's disc label to `out` (M6.506): the picture of the menu named `menu` (default: the one the disc
/// plays first; `none`: white), the disc's name, and its named chapters when `chapters`. With `link`, links it to
/// the project and saves the project (M6.509).
fn disc_label(
    project: &Path,
    out: &Path,
    (inner_mm, outer_mm, dpi): (f32, f32, f32),
    (menu, title): (Option<&str>, Option<&str>),
    (chapters, link): (bool, bool),
) -> Result<(), String> {
    use disccraft_menu::label::{self, DiscLabel, LabelNames};
    let mut session = Session::open(project).map_err(|error| error.to_string())?;
    let project_data = &session.project;
    let chosen = match menu.map(str::trim) {
        Some(name) if name.eq_ignore_ascii_case("none") => None,
        Some(name) => Some(
            project_data
                .menus
                .values()
                .find(|menu| menu.page_of.is_none() && menu.name.trim() == name)
                .map(|menu| menu.id)
                .ok_or_else(|| format!("the project has no menu named {name}"))?,
        ),
        None => label::first_menu(project_data),
    };
    let picture = chosen
        .and_then(|id| project_data.menu(id))
        .map(|menu| -> Result<_, String> {
            let (rgb, width, height) =
                disccraft_menu::menu_background_rgb(project_data, menu, None)
                    .map_err(|error| error.to_string())?;
            let frame = disccraft_project::frame_size(&project_data.settings);
            Ok((
                rgb,
                usize::from(width),
                usize::from(height),
                label::display_aspect(menu, frame),
            ))
        })
        .transpose()?;
    // M6.508: the title given, else the disc's own name (not the project file's).
    let title = title.map_or_else(
        || project_data.settings.name.trim().to_owned(),
        |title| title.trim().to_owned(),
    );
    let lines = if chapters {
        label::named_chapters(project_data)
    } else {
        Vec::new()
    };
    let count = lines.len();
    let made = DiscLabel {
        inner_mm,
        outer_mm,
        dpi,
        title,
        lines,
        picture,
        names: LabelNames {
            face: "Disc face".to_owned(),
            picture: "Picture".to_owned(),
            guides: "Guides (hidden, not printed)".to_owned(),
            lines: "Chapters".to_owned(),
        },
    };
    // M6.507: a DesignCraft document, or a PSD for PhotoCraft when the file named is one.
    let psd = out
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("psd"));
    if psd {
        label::export_disc_label_psd(&made, out).map_err(|error| error.to_string())?;
        println!(
            "wrote {}: the disc 120 mm across at {dpi:.0} dpi, printable from {inner_mm} to {outer_mm} mm, {count} chapters",
            out.display()
        );
    } else {
        let cli = label::designcraft::find_cli().ok_or_else(|| {
            "DesignCraft was not found (set DISCCRAFT_DESIGNCRAFT_CLI to its designcraft-cli), or write a PSD for PhotoCraft with an --out ending in .psd".to_owned()
        })?;
        let result =
            label::designcraft::make(&made, &cli, out).map_err(|error| error.to_string())?;
        let chapters = match result.chapters_points {
            Some(points) if result.chapters_overset => {
                format!("{count} chapters at {points} pt, some left out (overset)")
            }
            Some(points) => format!("{count} chapters at {points} pt"),
            None => "no chapters".to_owned(),
        };
        println!(
            "wrote {}: the disc 120 mm across, printable from {inner_mm} to {outer_mm} mm, {chapters}",
            out.display()
        );
    }
    if link {
        session.execute(
            "disc.setLabel",
            &serde_json::json!({ "path": out.to_string_lossy() }),
        )?;
        let saved = session.save(None)?;
        println!("linked it to {}", saved.display());
    }
    Ok(())
}

/// Prints what the drive tells of the disc in the recorder (M9.45).
fn disc_info(wanted: Option<&str>) -> Result<(), String> {
    use disccraft_platform::burn;
    let recorder = find_recorder(wanted)?;
    let report = burn::disc_report(&recorder.id).map_err(|error| error.to_string())?;
    println!("{}: {}", recorder.label(), report.kind.name());
    // M9.47: who made the disc, as the drive reads it (a DVD+R's or DVD+RW's).
    if let Some(id) = &report.media_id {
        println!("media ID: {id}");
    }
    match &report.disc {
        Ok(disc) => println!(
            "disc: {}, last session {}, {} session(s), tracks {}..{} in the last session, last possible lead-out at {}{}",
            match disc.status {
                0 => "empty",
                1 => "incomplete (appendable)",
                2 => "finalized",
                _ => "other",
            },
            match disc.last_session {
                0 => "empty",
                1 => "incomplete",
                2 => "reserved or damaged",
                _ => "complete",
            },
            disc.sessions,
            disc.first_track_in_last_session,
            disc.last_track_in_last_session,
            disc.last_possible_lead_out,
            if disc.erasable { ", erasable" } else { "" }
        ),
        Err(error) => println!("disc: {error}"),
    }
    for track in &report.tracks {
        match track {
            Ok(track) => println!(
                "track {} (session {}): start {}, size {}, free {}{}{}, next writable {}, last recorded {}",
                track.track,
                track.session,
                track.start,
                track.size,
                track.free_blocks,
                if track.reserved { ", reserved" } else { "" },
                if track.blank { ", blank" } else { "" },
                track
                    .next_writable
                    .map_or_else(|| "-".to_owned(), |lba| lba.to_string()),
                track
                    .last_recorded
                    .map_or_else(|| "-".to_owned(), |lba| lba.to_string()),
            ),
            Err(error) => println!("track: {error}"),
        }
    }
    if let Some(boundary) = report.layer_boundary {
        println!(
            "layer 0: {} sectors ({})",
            boundary.layer0_sectors,
            if boundary.fixed {
                "fixed"
            } else {
                "can be made smaller"
            }
        );
        // M9.47: an empty DVD+R DL with its layer 0 fixed had a writing stop on it.
        if boundary.fixed
            && report.kind == burn::MediaKind::DvdPlusRDualLayer
            && report.disc.as_ref().is_ok_and(|disc| disc.status == 0)
        {
            println!(
                "a writing stopped on this disc before: the drive calls it empty, but part of it may be written"
            );
        }
    }
    if let Some(written) = report.written_layer0 {
        println!("written layer 0 ends after {written} sectors");
    }
    Ok(())
}

/// Prints each recorder (id, drive and name) and the disc in it.
fn list_recorders() -> Result<(), String> {
    use disccraft_platform::burn;
    let recorders = burn::recorders().map_err(|error| error.to_string())?;
    if recorders.is_empty() {
        println!("no recorder found");
    }
    for recorder in &recorders {
        println!("{}\t{}", recorder.id, recorder.label());
        match burn::media(&recorder.id) {
            Ok(None) => println!("\tno disc"),
            Ok(Some(media)) => {
                let speeds: Vec<String> = media
                    .write_speeds
                    .iter()
                    .map(|speed| format!("{}x", media.kind.speed_factor(*speed)))
                    .collect();
                println!(
                    "\t{}{}{}, {} of {} bytes free, speeds {}",
                    media.kind.name(),
                    if media.supported {
                        ""
                    } else {
                        " (cannot be written)"
                    },
                    if media.started_before() {
                        ", blank as the drive says, but a writing stopped on it before (cannot be written)"
                    } else if media.blank {
                        ", blank"
                    } else {
                        ""
                    },
                    media.free_bytes,
                    media.total_bytes,
                    speeds.join(" ")
                );
                // M9.42: where a dual-layer disc's first layer ends, and whether it can still be set.
                if let Some(boundary) = media.layer_boundary {
                    println!(
                        "	layer 0: {} sectors ({})",
                        boundary.layer0_sectors,
                        if boundary.fixed {
                            "fixed"
                        } else {
                            "can be made smaller"
                        }
                    );
                }
            }
            Err(error) => println!("\t{error}"),
        }
    }
    Ok(())
}

/// Writes `image` to the disc in the recorder chosen by id or drive letter (else the first), with the progress on
/// standard error.
fn burn_image(
    image: &Path,
    wanted: Option<&str>,
    speed: Option<f64>,
    (erase, verify): (bool, bool),
) -> Result<(), String> {
    use disccraft_platform::burn::{self, BurnRequest, BurnStep};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    let recorder = find_recorder(wanted)?;
    // The speed asked for, as the nearest the disc offers.
    let speed = match speed {
        None => None,
        Some(factor) => {
            let media = burn::media(&recorder.id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "there is no disc in the recorder".to_string())?;
            media.write_speeds.iter().copied().min_by(|a, b| {
                let off = |speed: u32| (media.kind.speed_factor(speed) - factor).abs();
                off(*a).total_cmp(&off(*b))
            })
        }
    };
    let request = BurnRequest {
        image: image.to_path_buf(),
        recorder: recorder.id.clone(),
        speed,
        erase_rewritable: erase,
        eject: true,
        verify,
        // M9.42: a DVD-9 image's layer break, from the note the build writes beside it.
        layer_break: disccraft_build::image_layer_break(image),
    };
    let progress: Arc<dyn Fn(BurnStep, f32) + Send + Sync> = Arc::new(|step, fraction| {
        use std::io::Write;
        let what = match step {
            BurnStep::Erasing => "erasing",
            BurnStep::Writing => "writing",
            BurnStep::Closing => "closing",
            BurnStep::Verifying => "verifying",
        };
        // `as` saturates; the fraction is within 0..=1.
        let _ = write!(
            std::io::stderr(),
            "\r{what} {:3}%",
            (fraction * 100.0).round() as u32
        );
    });
    burn::burn(&request, progress, Arc::new(AtomicBool::new(false)))
        .map_err(|error| error.to_string())?;
    eprintln!();
    println!(
        "written to {}{}",
        recorder.label(),
        if verify {
            "; verified: the disc is the same as the image"
        } else {
            ""
        }
    );
    Ok(())
}

fn list_commands(json: bool) -> Result<(), String> {
    let specs = commands();

    if json {
        let mut items = Vec::with_capacity(specs.len());

        for spec in specs {
            let mut map = Map::new();
            map.insert("id".to_string(), Value::String(spec.id.to_string()));
            map.insert("label".to_string(), Value::String(spec.label.to_string()));
            map.insert(
                "menu".to_string(),
                Value::Array(
                    spec.menu
                        .iter()
                        .map(|item| Value::String(item.to_string()))
                        .collect(),
                ),
            );

            let shortcut = match spec.shortcut {
                Some(shortcut) => Value::String(shortcut.to_string()),
                None => Value::Null,
            };
            map.insert("shortcut".to_string(), shortcut);
            map.insert("params".to_string(), Value::String(spec.params.to_string()));
            map.insert("journal".to_string(), Value::Bool(spec.journal));

            items.push(Value::Object(map));
        }

        print_json_pretty(&Value::Array(items))?;
    } else {
        let id_width = specs
            .iter()
            .fold(0usize, |acc, spec| acc.max(spec.id.chars().count()));
        let label_width = specs
            .iter()
            .fold(0usize, |acc, spec| acc.max(spec.label.chars().count()));

        for spec in specs {
            say(&format!(
                "{id:<id_width$}  {label:<label_width$}  {params}",
                id = spec.id,
                label = spec.label,
                params = spec.params,
                id_width = id_width,
                label_width = label_width
            ))?;
        }
    }

    Ok(())
}

fn exec_command(project: &Path, command: &str, params: Option<&str>) -> Result<(), String> {
    let params = parse_params(params)?;
    let mut session = open_session(project)?;

    let result = session.execute(command, &params)?;
    print_json_pretty(&result)?;
    save_if_dirty(&mut session, project)?;

    Ok(())
}

fn run_script(project: &Path, script: &Path) -> Result<(), String> {
    let metadata = fs::metadata(script).map_err(|error| format!("cannot read script: {error}"))?;
    if metadata.len() > MAX_SCRIPT_BYTES {
        return Err("script larger than 16 MiB".to_string());
    }

    let content =
        fs::read_to_string(script).map_err(|error| format!("cannot read script: {error}"))?;
    let mut session = open_session(project)?;

    for (index, raw) in content.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        let value = serde_json::from_str::<Value>(trimmed)
            .map_err(|error| format!("line {line}: invalid JSON: {error}"))?;

        let Some(command) = value.get("command").and_then(Value::as_str) else {
            return Err(format!("line {line}: missing command"));
        };

        let params = match value.get("params") {
            Some(params) => params.clone(),
            None => Value::Null,
        };

        let result = session
            .execute(command, &params)
            .map_err(|error| format!("line {line}: {error}"))?;

        let text = serde_json::to_string(&result)
            .map_err(|error| format!("line {line}: serialization failed: {error}"))?;
        say(&text)?;
    }

    save_if_dirty(&mut session, project)?;
    Ok(())
}

fn inspect_project(project: &Path, json: bool) -> Result<(), String> {
    let mut session = Session::open(project).map_err(|error| error.to_string())?;

    if json {
        let result = session.execute("project.inspect", &Value::Null)?;
        print_json_pretty(&result)?;
    } else {
        let summary = inspect::human_summary(&session.project)?;
        say(&summary)?;
    }

    Ok(())
}

fn build_project(
    project: &Path,
    out: &Path,
    output: &str,
    (cache, audio_language): (Option<&Path>, Option<&str>),
) -> Result<(), String> {
    if !project.exists() {
        return Err(format!(
            "project file does not exist: {}",
            project.display()
        ));
    }

    let mut session = Session::open(project)?;
    let mut params = Map::new();
    params.insert("output".to_string(), Value::String(output.to_string()));
    params.insert(
        "path".to_string(),
        Value::String(out.to_string_lossy().into_owned()),
    );
    if let Some(cache) = cache {
        params.insert(
            "cache".to_string(),
            Value::String(cache.to_string_lossy().into_owned()),
        );
    }
    if let Some(language) = audio_language {
        params.insert(
            "audioLanguage".to_string(),
            Value::String(language.to_string()),
        );
    }

    let result = session.execute("build.start", &Value::Object(params))?;
    print_build_summary(&result)
}

fn print_build_summary(result: &Value) -> Result<(), String> {
    let output = result
        .get("output")
        .and_then(Value::as_str)
        .ok_or_else(|| "build result missing output".to_string())?;
    let titles = result
        .get("titles")
        .and_then(Value::as_u64)
        .ok_or_else(|| "build result missing titles".to_string())?;
    let chapters = result
        .get("chapters")
        .and_then(Value::as_u64)
        .ok_or_else(|| "build result missing chapters".to_string())?;
    let total_bytes = result
        .get("totalBytes")
        .and_then(Value::as_u64)
        .ok_or_else(|| "build result missing totalBytes".to_string())?;
    let seconds = result
        .get("seconds")
        .and_then(Value::as_f64)
        .ok_or_else(|| "build result missing seconds".to_string())?;

    // In MiB (M6.494: 1 MiB = 1,048,576 bytes; it was millions of bytes, named MB).
    let tenths = (total_bytes.saturating_mul(10) + (1 << 19)) >> 20;

    say(&format!("書き出した: {output}"))?;
    say(&format!(
        "タイトル {titles} / チャプター {chapters} / {}.{} MiB / {seconds:.1} 秒",
        tenths / 10,
        tenths % 10
    ))
}

fn open_session(project: &Path) -> Result<Session, String> {
    if project.exists() {
        Session::open(project).map_err(|error| error.to_string())
    } else {
        let mut session = Session::new();
        session.path = Some(project.to_path_buf());
        Ok(session)
    }
}

fn save_if_dirty(session: &mut Session, project: &Path) -> Result<(), String> {
    if session.dirty {
        session
            .save(Some(project))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn parse_params(params: Option<&str>) -> Result<Value, String> {
    match params {
        Some(text) => {
            serde_json::from_str(text).map_err(|error| format!("invalid params JSON: {error}"))
        }
        None => Ok(Value::Object(Map::new())),
    }
}

fn print_json_pretty(value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    say(&text)
}

/// Writes one line to standard output. When the reader has gone (`disccraft-cli commands | head`), the rest of the
/// output is dropped and the command carries on (a script still runs and saves); `println!` would panic there.
fn say(text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    match writeln!(out, "{text}").and_then(|()| out.flush()) {
        Err(error) if error.kind() != std::io::ErrorKind::BrokenPipe => {
            Err(format!("cannot write the output: {error}"))
        }
        _ => Ok(()),
    }
}
