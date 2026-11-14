#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

//! egui desktop UI for DiscCraft.

mod about;
mod actions;
mod app;
mod audio_out;
mod build_panel;
mod build_view;
mod button_edit;
mod chapter_playlist_view;
mod check_window;
mod color_set_dialog;
mod confirm;
mod context_menus;
mod craft_timeline;
mod dock;
mod flowchart;
mod fonts;
mod gui_params;
mod guides;
mod i18n;
mod icons;
mod layer_break_view;
mod library_panel;
mod link_dialog;
mod menu_commands;
mod menu_editor;
mod menu_graphic;
mod menu_text;
mod menubar;
mod monitor;
mod monitor_controls;
mod new_project;
mod orient;
mod panels;
mod photocraft;
mod pickwhip;
mod playback;
mod prefs;
mod preview_playback;
mod project_panel;
mod project_preview;
mod properties;
mod route_drag;
mod shadow_dialog;
mod slideshow_view;
mod subtitles;
mod theme;
mod timeline_view;
mod track_dialog;
mod user_library;
mod user_ops;
mod waveforms;
mod welcome;
mod workspace;

pub use app::DiscCraftApp;
pub use fonts::install_japanese_font;
pub use theme::apply_theme;

#[cfg(test)]
mod tests {
    use super::app::{hex_to_rgb, rgb_to_hex};
    use super::build_panel::{BuildJob, BuildState};
    use super::menu_editor::link_targets;
    use disccraft_build::{BuildOptions, BuildReport, OutputKind};
    use disccraft_project::{Link, Project};
    use std::path::PathBuf;

    #[test]
    fn link_targets_include_base_and_menu() -> Result<(), String> {
        let mut project = Project::default();
        let menu_id = project
            .add_menu(Some("メイン".to_string()))
            .map_err(|error| error.to_string())?;

        let targets = link_targets(&project, true);

        assert_eq!(targets.len(), 4);
        assert!(targets.iter().any(|(_, link)| *link == Link::NotSet));
        assert!(targets.iter().any(|(_, link)| *link == Link::Stop));
        assert!(targets.iter().any(|(_, link)| *link == Link::LastMenu));
        assert!(targets.iter().any(|(_, link)| matches!(
            link,
            Link::Menu { id, button: None } if *id == menu_id
        )));

        Ok(())
    }

    #[test]
    fn hex_colors_round_trip() {
        assert_eq!(rgb_to_hex([255, 0, 128]), "#ff0080");
        assert_eq!(hex_to_rgb("#FF0080"), Some([255, 0, 128]));
        assert_eq!(hex_to_rgb("ff0080"), Some([255, 0, 128]));
        assert_eq!(hex_to_rgb("bad"), None);
    }

    #[test]
    fn build_job_state_machine() -> Result<(), String> {
        let report = BuildReport {
            output: PathBuf::from("out"),
            titles: 1,
            chapters: 2,
            total_bytes: 3,
            seconds: 4.0,
            layer_break: None,
        };

        let mut job = BuildJob::default();
        assert_eq!(job.state, BuildState::Idle);

        job.begin();
        assert_eq!(job.state, BuildState::Running);

        job.finish(Ok(report.clone()));
        assert_eq!(job.state, BuildState::Finished);
        assert!(job.result.as_ref().is_some_and(|result| result.is_ok()));

        let options = BuildOptions {
            output: OutputKind::Folder,
            path: PathBuf::from("out"),
            cache: None,
            audio_language: None,
        };

        let mut job2 = BuildJob::default();
        job2.start(
            Project::default(),
            options,
            move |_, _, progress| {
                progress("movie", 0.5);
                Ok(report)
            },
            false,
        );
        assert_eq!(job2.current_progress(), Some(("movie".to_owned(), 0.5)));
        assert_eq!(job2.state, BuildState::Finished);

        Ok(())
    }
}
