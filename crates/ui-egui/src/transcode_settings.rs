//! The Transcode Settings dialog (Encore: Transcode Settings… of an asset), laid out as Encore's: on the left the
//! "DVD Transcode" group (Quality Preset) and the "Blu-ray Transcode" group (Codec, Dimensions, Frame Rate, Field,
//! Quality Preset); on the right OK, Cancel, Edit Quality Presets…, Transcode Defaults… and the two rendering
//! checkboxes.
//!
//! A DVD quality preset is Encore's name for a rate and a way of keeping it ("NTSC DV High quality 7Mb VBR 2 Pass":
//! 7 Mbit/s, two passes, Dolby Digital): Encore's built-in ones (an NTSC project's 23.976 ones first: film,
//! progressive pictures shown by 3:2 pulldown), then the user's own (Edit Quality Presets…), then Automatic.
//! A Blu-ray's are High Quality / Long Form / Maximum Quality of the project's Blu-ray format, whose codec,
//! dimensions and frame rate are the project's (Transcode Defaults…): the same for every asset here.

use disccraft_project::{
    Asset, AssetId, AssetKind, BEST_QUALITY, BdVideo, BlurayTranscode, MAX_TRANSCODE_KBPS,
    MIN_QUALITY, MIN_TRANSCODE_KBPS, TranscodeAudio, TranscodeMode, VideoStandard,
};
use serde_json::json;

use crate::app::DiscCraftApp;
use crate::i18n::{tl, tlf};

/// A DVD quality preset: its name, rate (kbit/s), how the rate is kept and what the sound is made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DvdPreset {
    pub(crate) name: String,
    pub(crate) kbps: u32,
    pub(crate) mode: TranscodeMode,
    pub(crate) audio: TranscodeAudio,
    /// Film: 23.976 progressive pictures shown by 3:2 pulldown (Encore's "NTSC DV 23.976 …").
    pub(crate) film: bool,
    /// How hard the encoder searches for motion (Encore's quality: its "High quality" presets' 5, "Low quality"
    /// ones' 2).
    pub(crate) quality: u8,
}

/// A Blu-ray quality preset: its name, target and peak rates (kbit/s), quality (Encore's: an MPEG-2 Long Form
/// one's 3, the others' 5) and how the rate is kept (Encore's: one-pass VBR; the user's own may differ, M6.328).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BlurayPreset {
    pub(crate) name: String,
    pub(crate) kbps: u32,
    pub(crate) max_kbps: u32,
    pub(crate) quality: u8,
    pub(crate) mode: TranscodeMode,
}

/// The quality of Encore's "Low quality" DVD presets (its "High quality" ones' is [`BEST_QUALITY`]).
const LOW_QUALITY: u8 = 2;

/// The quality of Encore's MPEG-2 "Long Form" Blu-ray presets.
const LONG_FORM_QUALITY: u8 = 3;

/// What was picked in a quality preset combo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Picked<P> {
    /// A preset.
    Preset(P),
    /// Automatic (the build fits the rate to the disc).
    Automatic,
    /// Use Project Settings (a Blu-ray's: the project's format at the automatic rate).
    ProjectSettings,
}

/// Encore's NTSC DVD presets: (name after the standard, kbit/s, way of keeping the rate, film). The 23.976 ones
/// are film (an NTSC project's only); a PAL project's others are the same with "PAL".
const DVD_PRESETS: [(&str, u32, TranscodeMode, bool); 9] = [
    (
        "DV 23.976 High quality 4Mb VBR 2 Pass",
        4_000,
        TranscodeMode::Vbr2Pass,
        true,
    ),
    (
        "DV 23.976 High quality 7Mb VBR 2 Pass",
        7_000,
        TranscodeMode::Vbr2Pass,
        true,
    ),
    (
        "DV 23.976 High quality 8Mb CBR 1 Pass",
        8_000,
        TranscodeMode::Cbr,
        true,
    ),
    (
        "DV 23.976 Low quality 4Mb CBR 1 Pass",
        4_000,
        TranscodeMode::Cbr,
        true,
    ),
    (
        "DV High quality 4Mb VBR 2 Pass",
        4_000,
        TranscodeMode::Vbr2Pass,
        false,
    ),
    (
        "DV High quality 7Mb VBR 2 Pass",
        7_000,
        TranscodeMode::Vbr2Pass,
        false,
    ),
    (
        "DV High quality 8Mb CBR 1 Pass",
        8_000,
        TranscodeMode::Cbr,
        false,
    ),
    (
        "DV Low quality 4Mb CBR 1 Pass",
        4_000,
        TranscodeMode::Cbr,
        false,
    ),
    (
        "Progressive High quality 7Mb VBR 2 Pass",
        7_000,
        TranscodeMode::Vbr2Pass,
        false,
    ),
];

/// The DVD quality presets offered for a project of `standard`: the built-in ones, then the user's own.
pub(crate) fn dvd_presets(
    standard: VideoStandard,
    user: &[crate::workspace::QualityPreset],
) -> Vec<DvdPreset> {
    let tv = match standard {
        VideoStandard::Ntsc => "NTSC",
        VideoStandard::Pal => "PAL",
    };
    DVD_PRESETS
        .iter()
        .filter(|(.., film)| !film || standard == VideoStandard::Ntsc)
        .map(|(name, kbps, mode, film)| DvdPreset {
            name: format!("{tv} {name}"),
            kbps: *kbps,
            mode: *mode,
            audio: TranscodeAudio::DolbyDigital,
            film: *film,
            // Encore's "Low quality" presets are quality 2, the others 5.
            quality: if name.contains("Low quality") {
                LOW_QUALITY
            } else {
                BEST_QUALITY
            },
        })
        .chain(
            user.iter()
                .filter(|preset| preset.bluray_max_kbps.is_none())
                .map(|preset| DvdPreset {
                    name: preset.name.clone(),
                    kbps: preset.kbps,
                    mode: preset.mode,
                    audio: preset.audio,
                    film: false,
                    quality: preset.quality,
                }),
        )
        .collect()
}

/// How a project's Blu-ray format is shown in Transcode Settings (Encore's Blu-ray group).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BlurayFormat {
    pub(crate) codec: &'static str,
    pub(crate) dimensions: String,
    pub(crate) rate: &'static str,
    /// Interlaced (its field is "Odd Field First"), else progressive.
    pub(crate) interlaced: bool,
    /// The start of its presets' names ("1920x1080i 29.97"; a standard-definition one's "NTSC SD").
    stem: String,
    /// What Automatic is called for it ("1920x1080i MPEG-2").
    pub(crate) automatic: String,
}

/// The project's Blu-ray format as Encore's Transcode Settings show it.
pub(crate) fn bluray_format(video: BdVideo, standard: VideoStandard) -> BlurayFormat {
    let (codec, width, height, rate, interlaced) = match video {
        BdVideo::Sd => (
            "MPEG-2",
            720,
            if standard == VideoStandard::Ntsc {
                480
            } else {
                576
            },
            if standard == VideoStandard::Ntsc {
                "29.97"
            } else {
                "25"
            },
            true,
        ),
        BdVideo::Hd1080p23 => ("H.264", 1920, 1080, "23.976", false),
        BdVideo::Hd1080p24 => ("H.264", 1920, 1080, "24", false),
        BdVideo::Hd1080i59 => ("MPEG-2", 1920, 1080, "29.97", true),
        BdVideo::Hd1080i50 => ("MPEG-2", 1920, 1080, "25", true),
        BdVideo::Hd720p59 => ("H.264", 1280, 720, "59.94", false),
        BdVideo::Hd720p50 => ("H.264", 1280, 720, "50", false),
        BdVideo::Hd720p23 => ("H.264", 1280, 720, "23.976", false),
        BdVideo::Hd720p24 => ("H.264", 1280, 720, "24", false),
    };
    let tv = if standard == VideoStandard::Ntsc {
        "NTSC"
    } else {
        "PAL"
    };
    let (stem, automatic) = match video {
        BdVideo::Sd => (format!("{tv} SD"), "SD MPEG-2".to_owned()),
        _ if interlaced => (
            format!("{width}x{height}i {rate}"),
            format!("{width}x{height}i {codec}"),
        ),
        _ => (
            format!("{width}x{height}p {rate}"),
            format!("{width}x{height} {rate}p {codec}"),
        ),
    };
    BlurayFormat {
        codec,
        dimensions: format!("{width} x {height}"),
        rate,
        interlaced,
        stem,
        automatic,
    }
}

/// The HD format an asset `shown` (as the dialog has it) is made in on a Blu-ray with `settings`: its own on an HD
/// Blu-ray (M6.325), else the project's.
fn shown_bd_video(settings: &disccraft_project::DiscSettings, shown: &Asset) -> BdVideo {
    if settings.bd_video.hd().is_none() {
        return settings.bd_video;
    }
    shown
        .bluray_video
        .filter(|video| video.hd().is_some())
        .unwrap_or(settings.bd_video)
}

/// The names, without repeats and in Encore's order, of the parts `part` of the HD formats `keep` keeps (as
/// Transcode Settings show them).
fn hd_parts(
    standard: VideoStandard,
    keep: impl Fn(&BlurayFormat) -> bool,
    part: impl Fn(&BlurayFormat) -> String,
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for video in BdVideo::ALL
        .into_iter()
        .filter(|video| video.hd().is_some())
    {
        let found = bluray_format(video, standard);
        let name = part(&found);
        if keep(&found) && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The HD format with the codec, dimensions and rate `wanted` (as Transcode Settings name them), or the nearest
/// one DiscCraft makes: that codec and those dimensions at another rate, else that codec.
fn hd_video_of(wanted: &[String; 3], standard: VideoStandard) -> Option<BdVideo> {
    let formats: Vec<(BdVideo, BlurayFormat)> = BdVideo::ALL
        .into_iter()
        .filter(|video| video.hd().is_some())
        .map(|video| (video, bluray_format(video, standard)))
        .collect();
    let [codec, dimensions, rate] = wanted;
    let find = |test: &dyn Fn(&BlurayFormat) -> bool| {
        formats
            .iter()
            .find(|(_, found)| test(found))
            .map(|(video, _)| *video)
    };
    find(&|found| {
        found.codec == codec.as_str()
            && found.dimensions == *dimensions
            && found.rate == rate.as_str()
    })
    .or_else(|| find(&|found| found.codec == codec.as_str() && found.dimensions == *dimensions))
    .or_else(|| find(&|found| found.codec == codec.as_str()))
}

/// The Blu-ray quality presets offered for a format: Encore's ([`bluray_presets`]), then the user's own Blu-ray
/// presets (M6.328, for any format).
pub(crate) fn bluray_presets_with(
    format: &BlurayFormat,
    user: &[crate::workspace::QualityPreset],
) -> Vec<BlurayPreset> {
    let mut presets = bluray_presets(format);
    presets.extend(user.iter().filter_map(|preset| {
        Some(BlurayPreset {
            name: preset.name.clone(),
            kbps: preset.kbps,
            max_kbps: preset.bluray_max_kbps?,
            quality: preset.quality,
            mode: preset.mode,
        })
    }));
    presets
}

/// The Blu-ray quality presets of a format (Encore's): High Quality (25 Mbit/s, peak 30), Long Form (9 or, for a
/// progressive one, 8 Mbit/s, peak 11) and Maximum Quality (35 Mbit/s, peak 35 for H.264, 40 for MPEG-2), each
/// one-pass VBR.
pub(crate) fn bluray_presets(format: &BlurayFormat) -> Vec<BlurayPreset> {
    let long = if format.interlaced { 9_000 } else { 8_000 };
    let most = if format.codec == "MPEG-2" {
        40_000
    } else {
        35_000
    };
    let sd = format.stem.ends_with(" SD");
    let named = |quality: &str| {
        if sd {
            // Encore's own names for a standard-definition Blu-ray.
            match quality {
                "Long Form" => format!("{} MPEG-2 Long Form MPEG-2", format.stem),
                other => format!("{} MPEG-2 {other}", format.stem),
            }
        } else {
            format!("{} {quality} {}", format.stem, format.codec)
        }
    };
    // An MPEG-2 Long Form preset searches less hard (Encore's quality 3); H.264 has no such setting.
    let long_quality = if format.codec == "MPEG-2" {
        LONG_FORM_QUALITY
    } else {
        BEST_QUALITY
    };
    vec![
        BlurayPreset {
            name: named("High Quality"),
            kbps: 25_000,
            max_kbps: 30_000,
            quality: BEST_QUALITY,
            mode: TranscodeMode::Vbr,
        },
        BlurayPreset {
            name: named("Long Form"),
            kbps: long,
            max_kbps: 11_000,
            quality: long_quality,
            mode: TranscodeMode::Vbr,
        },
        BlurayPreset {
            name: named("Maximum Quality"),
            kbps: 35_000,
            max_kbps: most,
            quality: BEST_QUALITY,
            mode: TranscodeMode::Vbr,
        },
    ]
}

/// What an asset's DVD setting is called: its preset's name, else its rate (and two passes or CBR), else
/// Automatic.
pub(crate) fn dvd_setting_name(asset: &Asset) -> String {
    match (asset.transcode_kbps, &asset.transcode_preset) {
        (Some(_), Some(name)) => name.clone(),
        (Some(kbps), None) => match mode_name(asset.transcode_mode) {
            Some(how) => format!("{} {how}", mbps_text(kbps)),
            None => mbps_text(kbps),
        },
        (None, _) => tl!("Automatic").to_owned(),
    }
}

/// What an asset's Blu-ray setting is called: its preset's name, else its rate, else Automatic (with the format
/// when `format` is given, as Encore's combo shows it).
pub(crate) fn bluray_setting_name(asset: &Asset, format: Option<&BlurayFormat>) -> String {
    match (&asset.bluray_transcode, format) {
        (Some(setting), _) if !setting.preset.is_empty() => setting.preset.clone(),
        (Some(setting), _) => mbps_text(setting.kbps),
        (None, Some(format)) => tlf!("Automatic: {format}", format = format.automatic),
        (None, None) => tl!("Automatic").to_owned(),
    }
}

/// A quality preset combo (Encore's): the presets, a separator, Automatic, `extra` (a Blu-ray's "Use Project
/// Settings") and "Don't Transcode" for a video that `fits` the disc as it is. When a preset can make such a video transcoded all the same (`forceable`: a DVD's),
/// "Don't Transcode" is its setting until a preset is picked, and picking it again takes the preset away; otherwise
/// it is ticked and the rest greyed. What was picked, if anything ("Don't Transcode" as [`Picked::Automatic`]: no
/// setting of its own).
pub(crate) fn preset_combo<P: Clone>(
    ui: &mut egui::Ui,
    salt: &str,
    (current, automatic): (&str, &str),
    presets: &[(String, P)],
    (fits, forceable): (bool, bool),
    extra: Option<&str>,
) -> Option<Picked<P>> {
    let mut picked = None;
    let locked = fits && !forceable;
    // Goes on the disc as it is: a video that fits, with no preset of its own (or none that can change that).
    let as_is = fits && (locked || current == automatic);
    let shown = if as_is {
        tl!("Don't Transcode")
    } else {
        current
    };
    let _ = egui::ComboBox::from_id_salt(salt)
        .width(260.0)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.set_min_width(300.0);
            for (name, preset) in presets {
                let response = ui.add_enabled(
                    !locked,
                    egui::Button::selectable(!as_is && current == name, name),
                );
                if response.clicked() {
                    picked = Some(Picked::Preset(preset.clone()));
                }
            }
            ui.separator();
            if ui
                .add_enabled(
                    !locked,
                    egui::Button::selectable(!fits && current == automatic, automatic),
                )
                .clicked()
            {
                picked = Some(Picked::Automatic);
            }
            if let Some(label) = extra
                && ui
                    .add_enabled(!locked, egui::Button::selectable(false, label))
                    .clicked()
            {
                picked = Some(Picked::ProjectSettings);
            }
            if fits {
                let response = ui
                    .add(egui::Button::selectable(as_is, tl!("Don't Transcode")))
                    .on_hover_text(tl!(
                        "This video goes on the disc as it is: it is not transcoded"
                    ));
                if response.clicked() && !locked {
                    picked = Some(Picked::Automatic);
                }
            }
        });
    picked
}

/// The name of what a video's sound is made when it is transcoded.
pub(crate) fn audio_name(audio: TranscodeAudio) -> &'static str {
    match audio {
        TranscodeAudio::DolbyDigital => tl!("Dolby Digital"),
        TranscodeAudio::Pcm => tl!("PCM"),
    }
}

/// Encore's "Use Maximum Render Quality" and "Use Frame Blending" checkboxes for `(max_quality, frame_blend)`:
/// whether either changed.
pub(crate) fn rendering_checkboxes(
    ui: &mut egui::Ui,
    (max_quality, frame_blend): &mut (bool, bool),
) -> bool {
    let quality = ui
        .checkbox(max_quality, tl!("Use Maximum Render Quality"))
        .on_hover_text(tl!(
            "Scaling down averages the pixels each new one covers (fine detail does not shimmer); scaling up blends between pixel centres. Slower"
        ));
    let blend = ui
        .checkbox(frame_blend, tl!("Use Frame Blending"))
        .on_hover_text(tl!(
            "When the disc's frame rate differs from the video's, a picture that falls between two frames mixes them instead of repeating one"
        ));
    quality.changed() || blend.changed()
}

/// A combo of the ways of keeping the rate (VBR 1 pass, VBR 2 pass, CBR) for `mode`.
fn encoding_combo(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    mode: &mut TranscodeMode,
) {
    let name = |mode: TranscodeMode| mode_name(mode).unwrap_or(tl!("VBR, 1 Pass"));
    egui::ComboBox::from_id_salt(salt)
        .selected_text(name(*mode))
        .show_ui(ui, |ui| {
            for choice in TranscodeMode::ALL {
                let _ = ui.selectable_value(mode, choice, name(choice));
            }
        });
}

/// A combo of what the sound is made (Dolby Digital or PCM) for `audio`.
fn audio_combo(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    audio: &mut TranscodeAudio,
) {
    egui::ComboBox::from_id_salt(salt)
        .selected_text(audio_name(*audio))
        .show_ui(ui, |ui| {
            for choice in TranscodeAudio::ALL {
                let _ = ui.selectable_value(audio, choice, audio_name(choice));
            }
        });
}

/// The name of a way of keeping the rate, as Encore's quality presets end ("VBR, 2 Pass"); `None` for one-pass VBR
/// (the usual one, not named in the settings column).
pub(crate) fn mode_name(mode: TranscodeMode) -> Option<&'static str> {
    match mode {
        TranscodeMode::Vbr => None,
        TranscodeMode::Vbr2Pass => Some(tl!("VBR, 2 Pass")),
        TranscodeMode::Cbr => Some(tl!("CBR, 1 Pass")),
    }
}

/// A rate in kbit/s as megabits a second, in text: "7 Mbps", "4.5 Mbps".
pub(crate) fn mbps_text(kbps: u32) -> String {
    let mbps = f64::from(kbps) / 1000.0;
    if (mbps - mbps.round()).abs() < 0.05 {
        format!("{mbps:.0} Mbps")
    } else {
        format!("{mbps:.1} Mbps")
    }
}

/// Megabits a second as a setting in kbit/s, inside what a setting takes.
fn kbps_of(mbps: f32) -> u32 {
    // `as` saturates; the value is clamped right after.
    ((f64::from(mbps) * 10.0).round() as u32)
        .saturating_mul(100)
        .clamp(MIN_TRANSCODE_KBPS, MAX_TRANSCODE_KBPS)
}

/// The dialog's state while it is open: the asset's settings as they will be set on OK.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TranscodeDraft {
    asset: AssetId,
    /// The DVD setting: rate (`None`: automatic), way of keeping it, sound, preset name and film.
    dvd: DvdDraft,
    /// The Blu-ray setting (`None`: automatic).
    bluray: Option<BlurayTranscode>,
    /// The asset's own HD format for a Blu-ray (`None`: the project's).
    bluray_video: Option<BdVideo>,
    /// Whether the video goes on a title in a format chosen here as it is (`asset.fitsBluray`), asked once.
    fits_as: Option<(BdVideo, bool)>,
    /// Encore's "Use Maximum Render Quality" and "Use Frame Blending".
    rendering: (bool, bool),
}

impl TranscodeDraft {
    /// The draft as an asset, for naming its settings.
    fn as_asset(&self, asset: &Asset) -> Asset {
        let mut shown = asset.clone();
        let dvd = self.dvd.clone();
        shown.transcode_kbps = dvd.kbps;
        shown.transcode_mode = dvd.mode;
        shown.transcode_audio = dvd.audio;
        shown.transcode_preset = dvd.preset;
        shown.transcode_film = dvd.film;
        shown.transcode_quality = dvd.quality;
        shown.bluray_transcode.clone_from(&self.bluray);
        shown.bluray_video = self.bluray_video;
        shown
    }
}

/// The dialog's DVD setting: rate (`None`: automatic), way of keeping it, sound, preset name, film and quality.
#[derive(Clone, Debug, PartialEq)]
struct DvdDraft {
    kbps: Option<u32>,
    mode: TranscodeMode,
    audio: TranscodeAudio,
    preset: Option<String>,
    film: bool,
    quality: u8,
}

impl DvdDraft {
    /// Automatic: the rate that fits the disc, one-pass VBR, Dolby Digital, the best quality.
    fn automatic() -> Self {
        Self {
            kbps: None,
            mode: TranscodeMode::Vbr,
            audio: TranscodeAudio::DolbyDigital,
            preset: None,
            film: false,
            quality: BEST_QUALITY,
        }
    }

    /// A preset's setting.
    fn of(preset: DvdPreset) -> Self {
        Self {
            kbps: Some(preset.kbps),
            mode: preset.mode,
            audio: preset.audio,
            preset: Some(preset.name),
            film: preset.film,
            quality: preset.quality,
        }
    }
}

impl DiscCraftApp {
    /// Whether `asset` has a transcode setting to change: a video asset.
    pub(crate) fn has_transcode_settings(&self, asset: AssetId) -> bool {
        self.session
            .project
            .assets
            .get(&asset)
            .is_some_and(|asset| asset.kind == AssetKind::Video)
    }

    /// Opens the dialog for `asset`.
    pub(crate) fn open_transcode_settings(&mut self, asset: AssetId) {
        let Some(found) = self.session.project.assets.get(&asset) else {
            return;
        };
        self.transcode_settings_dialog = Some(TranscodeDraft {
            asset,
            dvd: DvdDraft {
                kbps: found.transcode_kbps,
                mode: found.transcode_mode,
                audio: found.transcode_audio,
                preset: found.transcode_preset.clone(),
                film: found.transcode_film,
                quality: found.transcode_quality,
            },
            bluray: found.bluray_transcode.clone(),
            bluray_video: found.bluray_video,
            fits_as: None,
            rendering: (found.max_render_quality, found.frame_blend),
        });
    }

    /// Whether `asset`'s video goes on a Blu-ray title in `video`, a format chosen in the dialog, as it is: asked
    /// of the engine once per format (`known`: the last answer).
    fn fits_bluray_as(
        &mut self,
        asset: AssetId,
        video: BdVideo,
        known: &mut Option<(BdVideo, bool)>,
    ) -> bool {
        if let Some((asked, fits)) = *known
            && asked == video
        {
            return fits;
        }
        let fits = self
            .run_query(
                "asset.fitsBluray",
                json!({ "asset": asset.0, "video": video.id() }),
            )
            .and_then(|answer| answer.get("fits").and_then(serde_json::Value::as_bool))
            .unwrap_or(false);
        *known = Some((video, fits));
        fits
    }

    /// Whether `asset` fits a disc of each kind as it is (DVD, Blu-ray): its preset combos then offer "Don't
    /// Transcode" (a preset transcodes it all the same).
    pub(crate) fn passes_through_kinds(&self, asset: AssetId) -> (bool, bool) {
        let unneeded = crate::project_panel::TranscodeState::Unneeded;
        let (fits_dvd, fits_bluray) = self.video_fits(asset);
        self.transcode_columns(asset)
            .map_or((false, false), |(dvd, bluray)| {
                (
                    dvd == unneeded || fits_dvd,
                    bluray == unneeded || fits_bluray,
                )
            })
    }

    /// The DVD preset combo for an asset as `shown` (in the dialog or the Properties): what was picked.
    pub(crate) fn dvd_preset_combo(
        &self,
        ui: &mut egui::Ui,
        salt: &str,
        shown: &Asset,
        passes: bool,
    ) -> Option<Picked<DvdPreset>> {
        let presets: Vec<(String, DvdPreset)> = dvd_presets(
            self.session.project.settings.standard,
            &self.quality_presets,
        )
        .into_iter()
        .map(|preset| (preset.name.clone(), preset))
        .collect();
        preset_combo(
            ui,
            salt,
            (&dvd_setting_name(shown), tl!("Automatic")),
            &presets,
            (passes, true),
            None,
        )
    }

    /// The Blu-ray preset combo for an asset as `shown`: what was picked.
    pub(crate) fn bluray_preset_combo(
        &self,
        ui: &mut egui::Ui,
        salt: &str,
        shown: &Asset,
        passes: bool,
    ) -> Option<Picked<BlurayPreset>> {
        let settings = &self.session.project.settings;
        // The presets of the asset's own format (M6.325), else the project's.
        let format = bluray_format(shown_bd_video(settings, shown), settings.standard);
        let presets: Vec<(String, BlurayPreset)> =
            bluray_presets_with(&format, &self.quality_presets)
                .into_iter()
                .map(|preset| (preset.name.clone(), preset))
                .collect();
        let automatic = tlf!("Automatic: {format}", format = format.automatic);
        preset_combo(
            ui,
            salt,
            (&bluray_setting_name(shown, Some(&format)), &automatic),
            &presets,
            (passes, true),
            Some(tl!("Use Project Settings")),
        )
    }

    /// Sets an asset's DVD setting from a pick (one undo step).
    pub(crate) fn apply_dvd_pick(&mut self, asset: AssetId, picked: Picked<DvdPreset>) {
        let params = match picked {
            Picked::Preset(preset) => json!({
                "asset": asset.0,
                "kbps": preset.kbps,
                "mode": preset.mode.as_str(),
                "audio": preset.audio.as_str(),
                "preset": preset.name,
                "film": preset.film,
                "quality": preset.quality,
            }),
            Picked::Automatic | Picked::ProjectSettings => json!({
                "asset": asset.0,
                "kbps": null,
                "mode": TranscodeMode::Vbr.as_str(),
                "audio": TranscodeAudio::DolbyDigital.as_str(),
                "film": false,
            }),
        };
        let _ = self.run_command("asset.setTranscode", params);
        self.transcode_dirty = true;
    }

    /// Sets an asset's Blu-ray setting from a pick (one undo step).
    pub(crate) fn apply_bluray_pick(&mut self, asset: AssetId, picked: Picked<BlurayPreset>) {
        let params = match picked {
            Picked::Preset(preset) => json!({
                "asset": asset.0,
                "kbps": preset.kbps,
                "maxKbps": preset.max_kbps,
                "preset": preset.name,
                "quality": preset.quality,
                "mode": preset.mode.as_str(),
            }),
            Picked::Automatic => json!({ "asset": asset.0, "kbps": null }),
            // The project's format, at the automatic rate.
            Picked::ProjectSettings => json!({ "asset": asset.0, "kbps": null, "video": null }),
        };
        let _ = self.run_command("asset.setBlurayTranscode", params);
        self.transcode_dirty = true;
    }

    /// Draws the dialog while it is open (Encore's layout).
    pub(crate) fn transcode_settings_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.transcode_settings_dialog.clone() else {
            return;
        };
        let Some(asset) = self.session.project.assets.get(&draft.asset).cloned() else {
            self.transcode_settings_dialog = None;
            return;
        };
        let (dvd_passes, mut bluray_passes) = self.passes_through_kinds(draft.asset);
        let settings = self.session.project.settings.clone();
        // The asset's own HD format on an HD Blu-ray (M6.325), else the project's.
        let project_hd = settings.bd_video.hd().is_some();
        let video = shown_bd_video(&settings, &draft.as_asset(&asset));
        // Another format chosen here: "Don't Transcode" only if the video goes on its titles as it is.
        if video != shown_bd_video(&settings, &asset) {
            bluray_passes = self.fits_bluray_as(draft.asset, video, &mut draft.fits_as);
        }
        let format = bluray_format(video, settings.standard);
        let mut done = None;
        let mut edit_presets = false;
        let mut defaults = false;
        let response =
            egui::Modal::new(egui::Id::unique("transcode_settings_dialog")).show(ctx, |ui| {
                ui.heading(tl!("Transcode Settings"));
                ui.label(egui::RichText::new(&asset.name).weak());
                ui.add_space(4.0);
                let shown = draft.as_asset(&asset);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(430.0);
                        crate::theme::group_box(ui, "transcode_dvd", tl!("DVD Transcode"), |ui| {
                            ui.horizontal(|ui| {
                                ui.label(tl!("Quality Preset:"));
                                if let Some(picked) =
                                    self.dvd_preset_combo(ui, "transcode_dvd_preset", &shown, dvd_passes)
                                {
                                    draft.dvd = match picked {
                                        Picked::Preset(preset) => DvdDraft::of(preset),
                                        Picked::Automatic | Picked::ProjectSettings => {
                                            DvdDraft::automatic()
                                        }
                                    };
                                }
                            });
                        });
                        ui.add_space(4.0);
                        crate::theme::group_box(
                            ui,
                            "transcode_bluray",
                            tl!("Blu-ray Transcode"),
                            |ui| {
                                // On an HD Blu-ray the asset's titles can be made in a format of their own
                                // (M6.325); an SD Blu-ray's are the project's.
                                let fixed = tl!(
                                    "The project's Blu-ray video is SD: an asset's own format is chosen on an HD Blu-ray (Transcode Defaults…)"
                                );
                                let field = if format.interlaced {
                                    tl!("Odd Field First")
                                } else {
                                    tl!("No Fields (Progressive Scan)")
                                };
                                let standard = settings.standard;
                                let mut chosen: Option<BdVideo> = None;
                                egui::Grid::new("transcode_bluray_grid")
                                    .num_columns(2)
                                    .spacing([8.0, 6.0])
                                    .show(ui, |ui| {
                                        // Codec, then the dimensions of that codec's formats, then their rates.
                                        let codecs = hd_parts(standard, |_| true, |found| found.codec.to_owned());
                                        let sizes = hd_parts(
                                            standard,
                                            |found| found.codec == format.codec,
                                            |found| found.dimensions.clone(),
                                        );
                                        let rates = hd_parts(
                                            standard,
                                            |found| {
                                                found.codec == format.codec
                                                    && found.dimensions == format.dimensions
                                            },
                                            |found| found.rate.to_owned(),
                                        );
                                        let rows: [(&str, String, Vec<String>, usize); 3] = [
                                            (tl!("Codec:"), format.codec.to_owned(), codecs, 0),
                                            (tl!("Dimensions:"), format.dimensions.clone(), sizes, 1),
                                            (tl!("Frame Rate:"), format.rate.to_owned(), rates, 2),
                                        ];
                                        for (label, value, options, part) in rows {
                                            ui.label(label);
                                            ui.add_enabled_ui(project_hd, |ui| {
                                                let _ = egui::ComboBox::from_id_salt(label)
                                                    .width(260.0)
                                                    .selected_text(value.as_str())
                                                    .show_ui(ui, |ui| {
                                                        for option in &options {
                                                            if ui
                                                                .selectable_label(*option == value, option.as_str())
                                                                .clicked()
                                                            {
                                                                let mut wanted = [
                                                                    format.codec.to_owned(),
                                                                    format.dimensions.clone(),
                                                                    format.rate.to_owned(),
                                                                ];
                                                                if let Some(slot) = wanted.get_mut(part) {
                                                                    slot.clone_from(option);
                                                                }
                                                                chosen = hd_video_of(&wanted, standard);
                                                            }
                                                        }
                                                    });
                                            })
                                            .response
                                            .on_disabled_hover_text(fixed);
                                            ui.end_row();
                                        }
                                        ui.label(tl!("Field:"));
                                        ui.add_enabled_ui(false, |ui| {
                                            let _ = egui::ComboBox::from_id_salt("transcode_bluray_field")
                                                .width(260.0)
                                                .selected_text(field)
                                                .show_ui(ui, |_| {});
                                        });
                                        ui.end_row();
                                        ui.label(tl!("Quality Preset:"));
                                        if let Some(picked) = self.bluray_preset_combo(
                                            ui,
                                            "transcode_bluray_preset",
                                            &shown,
                                            bluray_passes,
                                        ) {
                                            draft.bluray = match picked {
                                                Picked::Preset(preset) => Some(BlurayTranscode {
                                                    kbps: preset.kbps,
                                                    max_kbps: preset.max_kbps,
                                                    preset: preset.name,
                                                    quality: preset.quality,
                                                    mode: preset.mode,
                                                }),
                                                Picked::Automatic => None,
                                                Picked::ProjectSettings => {
                                                    draft.bluray_video = None;
                                                    None
                                                }
                                            };
                                        }
                                        ui.end_row();
                                    });
                                // Another format: its own presets (a preset of the one before is let go).
                                if let Some(next) = chosen.filter(|next| *next != video) {
                                    draft.bluray_video = Some(next);
                                    draft.bluray = None;
                                }
                            },
                        );
                    });
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.set_width(200.0);
                        let wide = egui::vec2(200.0, 0.0);
                        if ui.add(egui::Button::new(tl!("OK")).min_size(wide)).clicked() {
                            done = Some(true);
                        }
                        if ui.add(egui::Button::new(tl!("Cancel")).min_size(wide)).clicked() {
                            done = Some(false);
                        }
                        ui.add_space(12.0);
                        if ui
                            .add(egui::Button::new(tl!("Edit Quality Presets…")).min_size(wide))
                            .clicked()
                        {
                            edit_presets = true;
                        }
                        if ui
                            .add(egui::Button::new(tl!("Transcode Defaults…")).min_size(wide))
                            .on_hover_text(tl!(
                                "The project's settings: its TV standard and Blu-ray format"
                            ))
                            .clicked()
                        {
                            defaults = true;
                        }
                        ui.add_space(12.0);
                        // DiscCraft's conversion always scales and picks frames the same way.
                        rendering_checkboxes(ui, &mut draft.rendering);
                    });
                });
            });
        if response.should_close() {
            done = Some(false);
        }
        if edit_presets {
            // The presets are edited in their own dialog; this one stays open beneath it.
            self.transcode_settings_dialog = Some(draft);
            self.open_quality_presets();
            return;
        }
        if defaults {
            self.transcode_settings_dialog = Some(draft);
            self.open_project_settings();
            return;
        }
        match done {
            Some(true) => {
                self.transcode_settings_dialog = None;
                let dvd = draft.dvd;
                let _ = self.run_command(
                    "asset.setTranscode",
                    json!({
                        "asset": draft.asset.0,
                        "kbps": dvd.kbps,
                        "mode": dvd.mode.as_str(),
                        "audio": dvd.audio.as_str(),
                        "preset": dvd.preset,
                        "film": dvd.film,
                        "quality": dvd.quality,
                    }),
                );
                let video = draft.bluray_video.map(BdVideo::id);
                let bluray = match draft.bluray {
                    Some(setting) => json!({
                        "asset": draft.asset.0,
                        "kbps": setting.kbps,
                        "maxKbps": setting.max_kbps,
                        "preset": setting.preset,
                        "quality": setting.quality,
                        "mode": setting.mode.as_str(),
                        "video": video,
                    }),
                    None => json!({ "asset": draft.asset.0, "kbps": null, "video": video }),
                };
                let _ = self.run_command("asset.setBlurayTranscode", bluray);
                let (max_quality, frame_blend) = draft.rendering;
                if (asset.max_render_quality, asset.frame_blend) != draft.rendering {
                    let _ = self.run_command(
                        "asset.setRendering",
                        json!({
                            "asset": draft.asset.0,
                            "maxRenderQuality": max_quality,
                            "frameBlend": frame_blend,
                        }),
                    );
                }
                // Another rate is another transcode: the status is asked for again.
                self.transcode_dirty = true;
            }
            Some(false) => self.transcode_settings_dialog = None,
            None => self.transcode_settings_dialog = Some(draft),
        }
    }
}

/// A preset as the Edit Quality Presets dialog edits it: rates in Mbit/s, and whether it is a Blu-ray one (with
/// its peak) or a DVD one (with its sound).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PresetRow {
    name: String,
    mbps: f32,
    mode: TranscodeMode,
    audio: TranscodeAudio,
    quality: u8,
    bluray: bool,
    max_mbps: f32,
}

impl PresetRow {
    fn of(preset: &crate::workspace::QualityPreset) -> Self {
        Self {
            name: preset.name.clone(),
            mbps: preset.kbps as f32 / 1000.0,
            mode: preset.mode,
            audio: preset.audio,
            quality: preset.quality,
            bluray: preset.bluray_max_kbps.is_some(),
            max_mbps: preset.bluray_max_kbps.unwrap_or(30_000) as f32 / 1000.0,
        }
    }

    fn preset(self) -> crate::workspace::QualityPreset {
        crate::workspace::QualityPreset {
            name: self.name,
            kbps: kbps_of(self.mbps),
            mode: self.mode,
            audio: self.audio,
            quality: self.quality,
            bluray_max_kbps: self.bluray.then(|| kbps_of(self.max_mbps)),
        }
    }
}

/// One preset's row of Edit Quality Presets (`index` keeps its widgets apart): whether Delete was clicked.
fn preset_row(ui: &mut egui::Ui, index: usize, row: &mut PresetRow) -> bool {
    let _ = ui.add(
        egui::TextEdit::singleline(&mut row.name)
            .desired_width(200.0)
            .char_limit(60),
    );
    let rates = MIN_TRANSCODE_KBPS as f32 / 1000.0..=MAX_TRANSCODE_KBPS as f32 / 1000.0;
    let _ = ui.add(
        egui::DragValue::new(&mut row.mbps)
            .range(rates.clone())
            .speed(0.1)
            .fixed_decimals(1)
            .suffix(" Mbps"),
    );
    // Encore's export formats: MPEG2-DVD, or a Blu-ray one (M6.328: for any Blu-ray format, with a peak rate).
    let _ = egui::ComboBox::from_id_salt(("quality_preset_disc", index))
        .selected_text(if row.bluray { "Blu-ray" } else { "DVD" })
        .show_ui(ui, |ui| {
            let _ = ui.selectable_value(&mut row.bluray, false, "DVD");
            let _ = ui.selectable_value(&mut row.bluray, true, "Blu-ray");
        });
    if row.bluray {
        let _ = ui
            .add(
                egui::DragValue::new(&mut row.max_mbps)
                    .range(rates)
                    .speed(0.1)
                    .fixed_decimals(1)
                    .prefix(tl!("Peak "))
                    .suffix(" Mbps"),
            )
            .on_hover_text(tl!("The highest rate a moment of the video may take"));
    }
    encoding_combo(ui, ("quality_preset_mode", index), &mut row.mode);
    // A Blu-ray's sound is Dolby Digital.
    ui.add_enabled_ui(!row.bluray, |ui| {
        audio_combo(ui, ("quality_preset_audio", index), &mut row.audio);
    });
    // Encore's quality slider: how hard the encoder searches for motion.
    let _ = ui
        .add(
            egui::DragValue::new(&mut row.quality)
                .range(MIN_QUALITY..=BEST_QUALITY)
                .prefix(tl!("Quality ")),
        )
        .on_hover_text(tl!(
            "How hard the encoder looks for movement, 1 to 5: lower is faster, and the picture is a little worse at the same bit rate"
        ));
    ui.small_button(tl!("Delete")).clicked()
}

impl DiscCraftApp {
    /// File ▸ Transcode ▸ Edit Quality Presets…: opens the dialog on a copy of the presets.
    pub(crate) fn open_quality_presets(&mut self) {
        self.quality_presets_dialog =
            Some(self.quality_presets.iter().map(PresetRow::of).collect());
    }

    /// Draws the Edit Quality Presets dialog while it is open (Encore: Edit Quality Presets…, where named
    /// transcode settings are kept; here a preset is a name, a video bit rate and how the rate is kept).
    pub(crate) fn quality_presets_window(&mut self, ctx: &egui::Context) {
        let Some(mut presets) = self.quality_presets_dialog.clone() else {
            return;
        };
        let mut done = None;
        let response =
            egui::Modal::new(egui::Id::unique("quality_presets_dialog")).show(ctx, |ui| {
                ui.set_width(680.0);
                ui.heading(tl!("Edit Quality Presets"));
                ui.label(
                    egui::RichText::new(tl!(
                        "Named video bit rates and encodings, offered in Transcode Settings. They are kept with DiscCraft's settings, for every project."
                    ))
                    .weak()
                    .small(),
                );
                ui.add_space(6.0);
                let mut removed = None;
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (index, row) in presets.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            if preset_row(ui, index, row) {
                                removed = Some(index);
                            }
                        });
                    }
                });
                if let Some(index) = removed.filter(|index| *index < presets.len()) {
                    let _ = presets.remove(index);
                }
                if presets.is_empty() {
                    ui.label(egui::RichText::new(tl!("No presets yet")).weak());
                }
                let room = presets.len() < crate::workspace::MAX_QUALITY_PRESETS;
                if ui
                    .add_enabled(room, egui::Button::new(tl!("New Preset")))
                    .clicked()
                {
                    presets.push(PresetRow {
                        name: tlf!("Preset {n}", n = presets.len().saturating_add(1)),
                        mbps: 6.0,
                        mode: TranscodeMode::Vbr,
                        audio: TranscodeAudio::DolbyDigital,
                        quality: BEST_QUALITY,
                        bluray: false,
                        max_mbps: 30.0,
                    });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(tl!("OK")).clicked() {
                        done = Some(true);
                    }
                    if ui.button(tl!("Cancel")).clicked() {
                        done = Some(false);
                    }
                });
            });
        if response.should_close() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                self.quality_presets_dialog = None;
                let kept = crate::workspace::sane_quality_presets(
                    presets.into_iter().map(PresetRow::preset).collect(),
                );
                match crate::workspace::save_quality_presets(&kept) {
                    Ok(()) => self.quality_presets = kept,
                    Err(error) => self.status = error,
                }
            }
            Some(false) => self.quality_presets_dialog = None,
            None => self.quality_presets_dialog = Some(presets),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{bluray_format, bluray_presets, dvd_presets, kbps_of, mbps_text};

    /// M6.325: an asset's own Blu-ray format from the dialog's codec, dimensions and rate, or the nearest one
    /// DiscCraft makes.
    #[test]
    fn a_format_is_chosen_by_its_parts() {
        use disccraft_project::{BdVideo, VideoStandard};
        let pick =
            |parts: [&str; 3]| super::hd_video_of(&parts.map(str::to_owned), VideoStandard::Ntsc);
        assert_eq!(
            pick(["H.264", "1280 x 720", "59.94"]),
            Some(BdVideo::Hd720p59)
        );
        assert_eq!(
            pick(["MPEG-2", "1920 x 1080", "29.97"]),
            Some(BdVideo::Hd1080i59)
        );
        // H.264 at 1920 x 1080 has no 29.97: its first rate.
        assert_eq!(
            pick(["H.264", "1920 x 1080", "29.97"]),
            Some(BdVideo::Hd1080p23)
        );
        // MPEG-2 has no 720p: its own size.
        assert_eq!(
            pick(["MPEG-2", "1280 x 720", "59.94"]),
            Some(BdVideo::Hd1080i59)
        );
        assert_eq!(pick(["VC-1", "1280 x 720", "59.94"]), None);
        let codecs = super::hd_parts(
            VideoStandard::Ntsc,
            |_| true,
            |found| found.codec.to_owned(),
        );
        assert_eq!(codecs, ["H.264", "MPEG-2"]);
        let sizes = super::hd_parts(
            VideoStandard::Ntsc,
            |found| found.codec == "H.264",
            |found| found.dimensions.clone(),
        );
        assert_eq!(sizes, ["1920 x 1080", "1280 x 720"]);
    }

    /// M6.307: Encore's quality presets: a DVD's (the 23.976 film ones first since M6.314, an NTSC project's only;
    /// then the 29.97 ones and the user's) and a Blu-ray's for the project's format, named as Encore names them, with
    /// Encore's rates.
    #[test]
    fn presets_are_named_and_rated_as_encores() {
        use crate::workspace::QualityPreset;
        use disccraft_project::{BdVideo, TranscodeAudio, TranscodeMode, VideoStandard};
        let user = vec![
            QualityPreset::dvd("Mine", 6_000, TranscodeMode::Cbr, TranscodeAudio::Pcm, 4),
            // M6.328: a Blu-ray preset of the user's is offered for a Blu-ray only, whatever its format.
            QualityPreset {
                bluray_max_kbps: Some(30_000),
                ..QualityPreset::dvd(
                    "Twice",
                    20_000,
                    TranscodeMode::Vbr2Pass,
                    TranscodeAudio::DolbyDigital,
                    5,
                )
            },
        ];
        let dvd = dvd_presets(VideoStandard::Ntsc, &user);
        let names: Vec<&str> = dvd.iter().map(|preset| preset.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "NTSC DV 23.976 High quality 4Mb VBR 2 Pass",
                "NTSC DV 23.976 High quality 7Mb VBR 2 Pass",
                "NTSC DV 23.976 High quality 8Mb CBR 1 Pass",
                "NTSC DV 23.976 Low quality 4Mb CBR 1 Pass",
                "NTSC DV High quality 4Mb VBR 2 Pass",
                "NTSC DV High quality 7Mb VBR 2 Pass",
                "NTSC DV High quality 8Mb CBR 1 Pass",
                "NTSC DV Low quality 4Mb CBR 1 Pass",
                "NTSC Progressive High quality 7Mb VBR 2 Pass",
                "Mine",
            ]
        );
        let eight = dvd
            .get(6)
            .map(|preset| (preset.kbps, preset.mode, preset.audio, preset.film));
        assert_eq!(
            eight,
            Some((
                8_000,
                TranscodeMode::Cbr,
                TranscodeAudio::DolbyDigital,
                false
            ))
        );
        let film: Vec<bool> = dvd.iter().map(|preset| preset.film).collect();
        assert_eq!(
            film,
            [
                true, true, true, true, false, false, false, false, false, false
            ]
        );
        assert_eq!(
            dvd.get(1).map(|preset| (preset.kbps, preset.mode)),
            Some((7_000, TranscodeMode::Vbr2Pass))
        );
        assert_eq!(
            dvd.get(9).map(|preset| preset.audio),
            Some(TranscodeAudio::Pcm)
        );
        // M6.326: Encore's qualities: "High quality" 5, "Low quality" 2; the user's own as they set it.
        let qualities: Vec<u8> = dvd.iter().map(|preset| preset.quality).collect();
        assert_eq!(qualities, [5, 5, 5, 2, 5, 5, 5, 2, 5, 4]);
        // A PAL project's: no film ones.
        let pal = dvd_presets(VideoStandard::Pal, &[]);
        assert_eq!(pal.len(), 5);
        assert!(
            pal.iter()
                .all(|preset| preset.name.starts_with("PAL ") && !preset.film)
        );

        let interlaced = bluray_format(BdVideo::Hd1080i59, VideoStandard::Ntsc);
        assert_eq!(
            (
                interlaced.codec,
                interlaced.dimensions.as_str(),
                interlaced.rate,
                interlaced.automatic.as_str()
            ),
            ("MPEG-2", "1920 x 1080", "29.97", "1920x1080i MPEG-2")
        );
        let rated: Vec<(String, u32, u32)> = bluray_presets(&interlaced)
            .into_iter()
            .map(|preset| (preset.name, preset.kbps, preset.max_kbps))
            .collect();
        assert_eq!(
            rated,
            [
                (
                    "1920x1080i 29.97 High Quality MPEG-2".to_owned(),
                    25_000,
                    30_000
                ),
                (
                    "1920x1080i 29.97 Long Form MPEG-2".to_owned(),
                    9_000,
                    11_000
                ),
                (
                    "1920x1080i 29.97 Maximum Quality MPEG-2".to_owned(),
                    35_000,
                    40_000
                ),
            ]
        );
        // An MPEG-2 Long Form preset is quality 3 (Encore's); the H.264 ones have no quality (the best).
        let qualities: Vec<u8> = bluray_presets(&interlaced)
            .iter()
            .map(|preset| preset.quality)
            .collect();
        assert_eq!(qualities, [5, 3, 5]);
        let film = bluray_format(BdVideo::Hd1080p23, VideoStandard::Ntsc);
        assert_eq!(film.automatic, "1920x1080 23.976p H.264");
        // Encore's are one-pass VBR; the user's Blu-ray preset follows them, with its own way.
        let offered = super::bluray_presets_with(&film, &user);
        let ways: Vec<(&str, TranscodeMode, u32)> = offered
            .iter()
            .map(|preset| (preset.name.as_str(), preset.mode, preset.max_kbps))
            .collect();
        assert_eq!(ways.len(), 4);
        assert!(
            ways.iter()
                .take(3)
                .all(|(_, mode, _)| *mode == TranscodeMode::Vbr)
        );
        assert_eq!(
            ways.last(),
            Some(&("Twice", TranscodeMode::Vbr2Pass, 30_000))
        );
        assert!(
            bluray_presets(&film)
                .iter()
                .all(|preset| preset.quality == 5)
        );
        let rated: Vec<(String, u32, u32)> = bluray_presets(&film)
            .into_iter()
            .map(|preset| (preset.name, preset.kbps, preset.max_kbps))
            .collect();
        assert_eq!(
            rated,
            [
                (
                    "1920x1080p 23.976 High Quality H.264".to_owned(),
                    25_000,
                    30_000
                ),
                (
                    "1920x1080p 23.976 Long Form H.264".to_owned(),
                    8_000,
                    11_000
                ),
                (
                    "1920x1080p 23.976 Maximum Quality H.264".to_owned(),
                    35_000,
                    35_000
                ),
            ]
        );
        let sd = bluray_format(BdVideo::Sd, VideoStandard::Ntsc);
        let names: Vec<String> = bluray_presets(&sd)
            .into_iter()
            .map(|preset| preset.name)
            .collect();
        assert_eq!(
            names,
            [
                "NTSC SD MPEG-2 High Quality",
                "NTSC SD MPEG-2 Long Form MPEG-2",
                "NTSC SD MPEG-2 Maximum Quality",
            ]
        );
    }

    /// M6.274: quality presets are kept named and within the limits: blank names go, long ones are cut, rates
    /// are brought inside what a setting takes, and no more than the most are kept.
    #[test]
    fn quality_presets_are_kept_within_their_limits() {
        use crate::workspace::{MAX_QUALITY_PRESETS, sane_quality_presets};
        use disccraft_project::{TranscodeAudio, TranscodeMode};
        let dd = TranscodeAudio::DolbyDigital;
        use crate::workspace::QualityPreset;
        let kept = sane_quality_presets(vec![
            QualityPreset::dvd("  Concert  ", 7_000, TranscodeMode::Vbr2Pass, dd, 5),
            QualityPreset::dvd("   ", 5_000, TranscodeMode::Vbr, dd, 5),
            QualityPreset::dvd(&"x".repeat(100), 1, TranscodeMode::Vbr, dd, 0),
            QualityPreset {
                // A peak below the rate is the rate.
                bluray_max_kbps: Some(1_000),
                ..QualityPreset::dvd("Too much", 9_999_999, TranscodeMode::Cbr, dd, 9)
            },
        ]);
        let shape: Vec<(usize, u32, u8)> = kept
            .iter()
            .map(|preset| (preset.name.chars().count(), preset.kbps, preset.quality))
            .collect();
        assert_eq!(
            shape,
            [
                (7, 7_000, 5),
                (60, disccraft_project::MIN_TRANSCODE_KBPS, 1),
                (8, disccraft_project::MAX_TRANSCODE_KBPS, 5)
            ]
        );
        assert_eq!(
            kept.first()
                .map(|preset| (preset.name.as_str(), preset.mode)),
            Some(("Concert", TranscodeMode::Vbr2Pass))
        );
        assert_eq!(
            kept.last().and_then(|preset| preset.bluray_max_kbps),
            Some(disccraft_project::MAX_TRANSCODE_KBPS)
        );
        let many: Vec<QualityPreset> = (0..100)
            .map(|n| QualityPreset::dvd(&format!("p{n}"), 6_000, TranscodeMode::Vbr, dd, 5))
            .collect();
        assert_eq!(sane_quality_presets(many).len(), MAX_QUALITY_PRESETS);
    }

    /// M6.301: a preset keeps its encoding in the settings file; a one-pass VBR one, and those of files written
    /// before, are a name and a rate.
    #[test]
    fn a_preset_keeps_its_encoding() -> Result<(), serde_json::Error> {
        use crate::workspace::{QualityPreset, StoredPreset};
        use disccraft_project::{TranscodeAudio, TranscodeMode};
        let dd = TranscodeAudio::DolbyDigital;
        let presets: Vec<QualityPreset> = vec![
            QualityPreset::dvd("Old", 7_000, TranscodeMode::Vbr, dd, 5),
            QualityPreset::dvd("Two", 8_000, TranscodeMode::Vbr2Pass, dd, 5),
            QualityPreset::dvd("Pcm", 6_000, TranscodeMode::Vbr, TranscodeAudio::Pcm, 5),
            // M6.326: a quality below the best is kept after the sound.
            QualityPreset::dvd("Fast", 4_000, TranscodeMode::Cbr, dd, 2),
            // M6.328: a Blu-ray preset: its rate, peak, encoding, quality and the word "bluray".
            QualityPreset {
                bluray_max_kbps: Some(30_000),
                ..QualityPreset::dvd("Disc", 20_000, TranscodeMode::Vbr2Pass, dd, 5)
            },
        ];
        let stored: Vec<StoredPreset> = presets.iter().cloned().map(StoredPreset::from).collect();
        let text = serde_json::to_string(&stored)?;
        assert_eq!(
            text,
            r#"[["Old",7000],["Two",8000,"vbr2Pass"],["Pcm",6000,"vbr","pcm"],["Fast",4000,"cbr","dolbyDigital",2],["Disc",20000,30000,"vbr2Pass",5,"bluray"]]"#
        );
        let back: Vec<StoredPreset> = serde_json::from_str(&text)?;
        let back: Vec<QualityPreset> = back.into_iter().map(QualityPreset::from).collect();
        assert_eq!(back, presets);
        Ok(())
    }

    #[test]
    fn rates_are_shown_in_megabits_and_kept_in_kilobits() {
        assert_eq!(mbps_text(7_000), "7 Mbps");
        assert_eq!(mbps_text(4_500), "4.5 Mbps");
        assert_eq!(kbps_of(4.5), 4_500);
        assert_eq!(kbps_of(7.04), 7_000);
        // Inside what a setting takes.
        assert_eq!(kbps_of(0.2), 1_000);
        assert_eq!(kbps_of(99.0), 40_000);
    }
}
