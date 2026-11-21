//! The Transcode Settings dialog (Encore: Transcode Settings… of an asset), laid out as Encore's: on the left the
//! "DVD Transcode" group (Quality Preset) and the "Blu-ray Transcode" group (Codec, Dimensions, Frame Rate, Field,
//! Quality Preset); on the right OK, Cancel, Edit Quality Presets…, Transcode Defaults… and the two rendering
//! checkboxes.
//!
//! A DVD quality preset is Encore's name for a rate and a way of keeping it ("NTSC DV High quality 7Mb VBR 2 Pass":
//! 7 Mbit/s, two passes, Dolby Digital): Encore's built-in ones (an NTSC project's 23.976 ones first: film,
//! progressive pictures shown by 3:2 pulldown), then the user's own (Edit Quality Presets…), then Automatic.
//! A Blu-ray's are High Quality / Long Form / Maximum Quality of the asset's Blu-ray format: its codec, dimensions
//! and frame rate, its own on an HD Blu-ray (M6.325; the codec since M10.39, every format in both), else the
//! project's (Transcode Defaults…).

use disccraft_project::{
    Asset, AssetId, AssetKind, BEST_QUALITY, BdCodec, BdVideo, BlurayTranscode, MAX_TRANSCODE_KBPS,
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

/// What was picked in the whole Blu-ray list ([`bluray_list`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ListChoice {
    /// An entry of the list.
    Pick(ListPick),
    /// Automatic at its end: the project's format and codec at the automatic rate.
    ProjectSettings,
    /// Don't Transcode: no rate of its own (a video that goes on the disc as it is).
    AsIs,
}

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

/// A Blu-ray format coded as MPEG-2 (`mpeg2`) or H.264, as Encore's Transcode Settings show it.
pub(crate) fn bluray_format(video: BdVideo, mpeg2: bool, standard: VideoStandard) -> BlurayFormat {
    let codec = if mpeg2 { "MPEG-2" } else { "H.264" };
    let (width, height, rate, interlaced) = match video {
        BdVideo::Sd => (
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
        BdVideo::Hd1080p23 => (1920, 1080, "23.976", false),
        BdVideo::Hd1080p24 => (1920, 1080, "24", false),
        BdVideo::Hd1080i59 => (1920, 1080, "29.97", true),
        BdVideo::Hd1080i50 => (1920, 1080, "25", true),
        BdVideo::Hd720p59 => (1280, 720, "59.94", false),
        BdVideo::Hd720p50 => (1280, 720, "50", false),
        BdVideo::Hd720p23 => (1280, 720, "23.976", false),
        BdVideo::Hd720p24 => (1280, 720, "24", false),
        BdVideo::Hd1440p23 => (1440, 1080, "23.976", false),
        BdVideo::Hd1440p24 => (1440, 1080, "24", false),
        BdVideo::Hd1440i59 => (1440, 1080, "29.97", true),
        BdVideo::Hd1440i50 => (1440, 1080, "25", true),
    };
    let tv = if standard == VideoStandard::Ntsc {
        "NTSC"
    } else {
        "PAL"
    };
    // Encore names the 1440 x 1080 presets with spaces round the "x" ("1440 x 1080i 29.97 High Quality H.264"), but
    // not their Automatic ("1440x1080i H.264").
    let size = if width == 1440 {
        format!("{width} x {height}")
    } else {
        format!("{width}x{height}")
    };
    let (stem, automatic) = match video {
        BdVideo::Sd => (format!("{tv} SD"), format!("SD {codec}")),
        _ if interlaced => (
            format!("{size}i {rate}"),
            format!("{width}x{height}i {codec}"),
        ),
        _ => (
            format!("{size}p {rate}"),
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

/// Whether an asset `shown` is coded as MPEG-2 on a Blu-ray with `settings` (else H.264): its own codec on an HD
/// Blu-ray (M10.39), else the project's, for its format ([`shown_bd_video`]).
fn shown_bd_mpeg2(settings: &disccraft_project::DiscSettings, shown: &Asset) -> bool {
    let codec = if settings.bd_video.hd().is_none() {
        settings.bd_codec
    } else {
        shown.bluray_codec.unwrap_or(settings.bd_codec)
    };
    codec.mpeg2(shown_bd_video(settings, shown))
}

/// Every HD format in both codecs, as Transcode Settings name them: the codecs in Encore's order (MPEG-2, H.264),
/// each format's sizes and rates from the smallest.
fn hd_formats(standard: VideoStandard) -> Vec<((BdVideo, bool), BlurayFormat)> {
    let mut formats: Vec<((BdVideo, bool), BlurayFormat)> = [true, false]
        .into_iter()
        .flat_map(|mpeg2| {
            BdVideo::ALL
                .into_iter()
                .filter(|video| video.hd().is_some())
                .map(move |video| ((video, mpeg2), bluray_format(video, mpeg2, standard)))
        })
        .collect();
    // A stable sort: the codecs stay in their order.
    formats.sort_by(|((a, a_mpeg2), _), ((b, b_mpeg2), _)| {
        let key = |video: BdVideo| {
            let (_, height, num, den) = video.hd().unwrap_or((0, 0, 0, 1));
            // The video's own size (1440 x 1080 between 720p and 1920 x 1080).
            let width = video.video_width().unwrap_or(0);
            (
                u64::from(width) * u64::from(height),
                u64::from(num) * 1_000_000 / u64::from(den.max(1)),
            )
        };
        b_mpeg2.cmp(a_mpeg2).then(key(*a).cmp(&key(*b)))
    });
    formats
}

/// The names, without repeats and in Encore's order, of the parts `part` of the HD formats `keep` keeps (as
/// Transcode Settings show them).
fn hd_parts(
    standard: VideoStandard,
    keep: impl Fn(&BlurayFormat) -> bool,
    part: impl Fn(&BlurayFormat) -> String,
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (_, found) in hd_formats(standard) {
        let name = part(&found);
        if keep(&found) && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The HD format and codec (MPEG-2: `true`) with the codec, dimensions and rate `wanted` (as Transcode Settings name
/// them), or the nearest one DiscCraft makes: that codec and those dimensions at another rate, else that codec.
fn hd_video_of(wanted: &[String; 3], standard: VideoStandard) -> Option<(BdVideo, bool)> {
    let formats = hd_formats(standard);
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
        if sd && format.codec == "H.264" {
            // Encore's names for a standard-definition Blu-ray in H.264 (M10.41).
            format!("{} H.264 {quality} H.264", format.stem)
        } else if sd {
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

/// What an entry of the whole Blu-ray list picks (M10.42).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ListPick {
    /// A Blu-ray preset of a format in a codec (MPEG-2: `true`).
    Preset {
        video: BdVideo,
        mpeg2: bool,
        preset: BlurayPreset,
    },
    /// A DVD quality preset: its rate as an SD Blu-ray's (MPEG-2, as a DVD's).
    Dvd(DvdPreset),
    /// A format in a codec at the automatic rate.
    Automatic { video: BdVideo, mpeg2: bool },
}

/// One entry of the whole Blu-ray list: its name, what it picks, and why it cannot be picked for this project.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ListEntry {
    pub(crate) name: String,
    pub(crate) pick: ListPick,
    pub(crate) unavailable: Option<&'static str>,
}

/// Whether a format's frame rate is one of the TV standard's list: Encore's NTSC project lists 23.976, 24, 29.97 and
/// 59.94, its PAL project 25 and 50 only (seen 2026-11-21, M10.43).
fn rate_of_standard(video: BdVideo, standard: VideoStandard) -> bool {
    match video.hd() {
        None => true,
        Some((_, _, 24_000, 1001) | (_, _, 24, 1)) => standard == VideoStandard::Ntsc,
        Some((_, _, 30_000, 1001) | (_, _, 60_000, 1001)) => standard == VideoStandard::Ntsc,
        Some(_) => standard == VideoStandard::Pal,
    }
}

/// Encore's whole Blu-ray list of an asset (its Properties: M10.42, as the user's screenshots of 2026-11-21 show it):
/// first every preset by name (each format of the TV standard in both codecs, the DVD presets, the user's Blu-ray
/// presets for the asset's format `shown`), then Automatic in each format and codec (SD, the interlaced formats from
/// the widest, then the progressive ones by rate and from the widest; MPEG-2 before H.264). The ones the project
/// cannot make are given with the reason: an HD format on an SD Blu-ray or SD on an HD one (the disc's video is the
/// project's), SD in the other codec than the disc's, a DVD preset but on an SD Blu-ray in MPEG-2, and film (3:2
/// pulldown is a DVD's).
pub(crate) fn bluray_list(
    settings: &disccraft_project::DiscSettings,
    shown: (BdVideo, bool),
    user: &[crate::workspace::QualityPreset],
) -> (Vec<ListEntry>, Vec<ListEntry>) {
    let standard = settings.standard;
    let hd_disc = settings.bd_video.hd().is_some();
    let sd_mpeg2 = settings.bd_codec.mpeg2(BdVideo::Sd);
    let why = |video: BdVideo, mpeg2: bool| -> Option<&'static str> {
        match (hd_disc, video.is_sd()) {
            (true, true) => Some(tl!(
                "SD titles are made on an SD Blu-ray (the project's Blu-ray video, Transcode Defaults…)"
            )),
            (false, false) => Some(tl!(
                "HD titles are made on an HD Blu-ray (the project's Blu-ray video, Transcode Defaults…)"
            )),
            (false, true) if mpeg2 != sd_mpeg2 => Some(tl!(
                "An SD Blu-ray's titles all take the project's codec (Transcode Defaults…)"
            )),
            _ => None,
        }
    };
    let formats: Vec<(BdVideo, bool)> = [true, false]
        .into_iter()
        .flat_map(|mpeg2| {
            BdVideo::ALL
                .into_iter()
                .filter(move |video| rate_of_standard(*video, standard))
                .map(move |video| (video, mpeg2))
        })
        .collect();
    let mut presets: Vec<ListEntry> = formats
        .iter()
        .flat_map(|&(video, mpeg2)| {
            bluray_presets(&bluray_format(video, mpeg2, standard))
                .into_iter()
                .map(move |preset| ListEntry {
                    name: preset.name.clone(),
                    pick: ListPick::Preset {
                        video,
                        mpeg2,
                        preset,
                    },
                    unavailable: why(video, mpeg2),
                })
        })
        .collect();
    for preset in dvd_presets(standard, &[]) {
        let unavailable = if hd_disc || !sd_mpeg2 {
            Some(tl!(
                "A DVD preset is an SD Blu-ray's in MPEG-2 (Transcode Defaults…)"
            ))
        } else if preset.film {
            Some(tl!("Film shown by 3:2 pulldown is a DVD's"))
        } else {
            None
        };
        presets.push(ListEntry {
            name: preset.name.clone(),
            pick: ListPick::Dvd(preset),
            unavailable,
        });
    }
    let (video, mpeg2) = shown;
    let format = bluray_format(video, mpeg2, standard);
    for preset in bluray_presets_with(&format, user).into_iter().skip(3) {
        presets.push(ListEntry {
            name: preset.name.clone(),
            pick: ListPick::Preset {
                video,
                mpeg2,
                preset,
            },
            unavailable: why(video, mpeg2),
        });
    }
    presets.sort_by(|a, b| a.name.cmp(&b.name));

    // Automatic: SD, then interlaced from the widest, then progressive by rate and from the widest.
    let mut kinds: Vec<BdVideo> = BdVideo::ALL
        .into_iter()
        .filter(|video| rate_of_standard(*video, standard))
        .collect();
    kinds.sort_by_key(|video| {
        let (_, _, num, den) = video.hd().unwrap_or((0, 0, 0, 1));
        let rate = u64::from(num) * 1000 / u64::from(den.max(1));
        let width = video.video_width().unwrap_or(0);
        (
            !video.is_sd(),
            !video.interlaced(),
            if video.interlaced() { 0 } else { rate },
            std::cmp::Reverse(width),
        )
    });
    let automatic = kinds
        .into_iter()
        .flat_map(|video| [(video, true), (video, false)])
        .map(|(video, mpeg2)| ListEntry {
            name: tlf!(
                "Automatic: {format}",
                format = bluray_format(video, mpeg2, standard).automatic
            ),
            pick: ListPick::Automatic { video, mpeg2 },
            unavailable: why(video, mpeg2),
        })
        .collect();
    (presets, automatic)
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
    /// The asset's own codec for a Blu-ray (`None`: the project's).
    bluray_codec: Option<BdCodec>,
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
        shown.bluray_codec = self.bluray_codec;
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
            bluray_codec: found.bluray_codec,
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
        // The presets of the asset's own format (M6.325) and codec (M10.39), else the project's.
        let format = bluray_format(
            shown_bd_video(settings, shown),
            shown_bd_mpeg2(settings, shown),
            settings.standard,
        );
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

    /// The Properties panel's Blu-ray combo (M10.42): Encore's whole list ([`bluray_list`]), a separator and
    /// Automatic (the project's settings), and "Don't Transcode" for a video that `passes` onto the disc as it is.
    /// What was picked, if anything.
    pub(crate) fn bluray_list_combo(
        &self,
        ui: &mut egui::Ui,
        salt: &str,
        shown: &Asset,
        passes: bool,
    ) -> Option<ListChoice> {
        let settings = &self.session.project.settings;
        let video = shown_bd_video(settings, shown);
        let mpeg2 = shown_bd_mpeg2(settings, shown);
        let (presets, automatic) = bluray_list(settings, (video, mpeg2), &self.quality_presets);
        let format = bluray_format(video, mpeg2, settings.standard);
        // What is ticked: its preset, else its own format's Automatic, else Automatic.
        let own = shown.bluray_video.is_some() || shown.bluray_codec.is_some();
        let current = match &shown.bluray_transcode {
            Some(_) => Some(bluray_setting_name(shown, Some(&format))),
            None if own && settings.bd_video.hd().is_some() => {
                Some(tlf!("Automatic: {format}", format = format.automatic))
            }
            None => None,
        };
        let as_is = passes && shown.bluray_transcode.is_none();
        let text = if as_is {
            tl!("Don't Transcode").to_owned()
        } else {
            current
                .clone()
                .unwrap_or_else(|| tl!("Automatic").to_owned())
        };
        let mut picked = None;
        let _ = egui::ComboBox::from_id_salt(salt)
            .width(260.0)
            .height(480.0)
            .selected_text(text)
            .show_ui(ui, |ui| {
                ui.set_min_width(340.0);
                for entry in presets.iter().chain(&automatic) {
                    let ticked = !as_is && current.as_deref() == Some(entry.name.as_str());
                    let mut response = ui.add_enabled(
                        entry.unavailable.is_none(),
                        egui::Button::selectable(ticked, &entry.name),
                    );
                    if let Some(why) = entry.unavailable {
                        response = response.on_disabled_hover_text(why);
                    }
                    if response.clicked() {
                        picked = Some(ListChoice::Pick(entry.pick.clone()));
                    }
                }
                ui.separator();
                let project = !as_is && current.is_none();
                if ui
                    .add(egui::Button::selectable(project, tl!("Automatic")))
                    .on_hover_text(tl!(
                        "The project's Blu-ray format and codec, at the rate that fits the disc"
                    ))
                    .clicked()
                {
                    picked = Some(ListChoice::ProjectSettings);
                }
                if passes
                    && ui
                        .add(egui::Button::selectable(as_is, tl!("Don't Transcode")))
                        .on_hover_text(tl!(
                            "This video goes on the disc as it is: it is not transcoded"
                        ))
                        .clicked()
                {
                    picked = Some(ListChoice::AsIs);
                }
            });
        picked
    }

    /// Sets an asset's Blu-ray setting from the whole list (one undo step): a preset or an Automatic of an HD
    /// format sets the asset's own format and codec with it; SD's are the project's.
    pub(crate) fn apply_bluray_list(&mut self, asset: AssetId, choice: ListChoice) {
        let own = |video: BdVideo, mpeg2: bool| {
            if video.is_sd() {
                (serde_json::Value::Null, serde_json::Value::Null)
            } else {
                (
                    json!(video.id()),
                    json!(if mpeg2 { BdCodec::Mpeg2 } else { BdCodec::H264 }.id()),
                )
            }
        };
        let params = match choice {
            ListChoice::Pick(ListPick::Preset {
                video,
                mpeg2,
                preset,
            }) => {
                let (video, codec) = own(video, mpeg2);
                json!({
                    "asset": asset.0,
                    "kbps": preset.kbps,
                    "maxKbps": preset.max_kbps,
                    "preset": preset.name,
                    "quality": preset.quality,
                    "mode": preset.mode.as_str(),
                    "video": video,
                    "codec": codec,
                })
            }
            // A DVD preset's peak: its rate when constant, else Encore's 9 Mbit/s.
            ListChoice::Pick(ListPick::Dvd(preset)) => json!({
                "asset": asset.0,
                "kbps": preset.kbps,
                "maxKbps": if preset.mode == TranscodeMode::Cbr { preset.kbps } else { preset.kbps.max(9_000) },
                "preset": preset.name,
                "quality": preset.quality,
                "mode": preset.mode.as_str(),
                "video": null,
            }),
            ListChoice::Pick(ListPick::Automatic { video, mpeg2 }) => {
                let (video, codec) = own(video, mpeg2);
                json!({ "asset": asset.0, "kbps": null, "video": video, "codec": codec })
            }
            ListChoice::ProjectSettings => json!({ "asset": asset.0, "kbps": null, "video": null }),
            ListChoice::AsIs => json!({ "asset": asset.0, "kbps": null }),
        };
        let _ = self.run_command("asset.setBlurayTranscode", params);
        self.transcode_dirty = true;
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
        let mpeg2 = shown_bd_mpeg2(&settings, &draft.as_asset(&asset));
        // Another format chosen here: "Don't Transcode" only if the video goes on its titles as it is.
        if video != shown_bd_video(&settings, &asset) {
            bluray_passes = self.fits_bluray_as(draft.asset, video, &mut draft.fits_as);
        }
        let format = bluray_format(video, mpeg2, settings.standard);
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
                                let mut chosen: Option<(BdVideo, bool)> = None;
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
                                                    draft.bluray_codec = None;
                                                    None
                                                }
                                            };
                                        }
                                        ui.end_row();
                                    });
                                // Another format or codec: its own presets (a preset of the one before is let go).
                                if let Some((next, next_mpeg2)) =
                                    chosen.filter(|next| *next != (video, mpeg2))
                                {
                                    draft.bluray_video = Some(next);
                                    draft.bluray_codec =
                                        Some(if next_mpeg2 { BdCodec::Mpeg2 } else { BdCodec::H264 });
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
                let codec = draft.bluray_codec.map(BdCodec::id);
                let bluray = match draft.bluray {
                    Some(setting) => json!({
                        "asset": draft.asset.0,
                        "kbps": setting.kbps,
                        "maxKbps": setting.max_kbps,
                        "preset": setting.preset,
                        "quality": setting.quality,
                        "mode": setting.mode.as_str(),
                        "video": video,
                        "codec": codec,
                    }),
                    None => json!({
                        "asset": draft.asset.0,
                        "kbps": null,
                        "video": video,
                        "codec": codec,
                    }),
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
    /// DiscCraft makes. M10.39: every format in both codecs (MPEG-2 first, as Encore lists them), the sizes and rates
    /// from the smallest.
    #[test]
    fn a_format_is_chosen_by_its_parts() {
        use disccraft_project::{BdVideo, VideoStandard};
        let pick =
            |parts: [&str; 3]| super::hd_video_of(&parts.map(str::to_owned), VideoStandard::Ntsc);
        assert_eq!(
            pick(["H.264", "1280 x 720", "59.94"]),
            Some((BdVideo::Hd720p59, false))
        );
        assert_eq!(
            pick(["MPEG-2", "1920 x 1080", "29.97"]),
            Some((BdVideo::Hd1080i59, true))
        );
        assert_eq!(
            pick(["H.264", "1920 x 1080", "29.97"]),
            Some((BdVideo::Hd1080i59, false))
        );
        assert_eq!(
            pick(["MPEG-2", "1280 x 720", "59.94"]),
            Some((BdVideo::Hd720p59, true))
        );
        assert_eq!(
            pick(["MPEG-2", "1920 x 1080", "23.976"]),
            Some((BdVideo::Hd1080p23, true))
        );
        // No such rate at that size: its first.
        assert_eq!(
            pick(["H.264", "1280 x 720", "29.97"]),
            Some((BdVideo::Hd720p23, false))
        );
        assert_eq!(pick(["VC-1", "1280 x 720", "59.94"]), None);
        let codecs = super::hd_parts(
            VideoStandard::Ntsc,
            |_| true,
            |found| found.codec.to_owned(),
        );
        assert_eq!(codecs, ["MPEG-2", "H.264"]);
        let sizes = super::hd_parts(
            VideoStandard::Ntsc,
            |found| found.codec == "H.264",
            |found| found.dimensions.clone(),
        );
        assert_eq!(sizes, ["1280 x 720", "1440 x 1080", "1920 x 1080"]);
        // M10.40: 1440 x 1080, named as Encore names it (spaces round the "x" in the presets, none in Automatic).
        assert_eq!(
            pick(["H.264", "1440 x 1080", "29.97"]),
            Some((BdVideo::Hd1440i59, false))
        );
        let hdv = super::bluray_format(BdVideo::Hd1440i59, false, VideoStandard::Ntsc);
        assert_eq!(hdv.automatic, "1440x1080i H.264");
        assert_eq!(
            super::bluray_presets(&hdv)
                .first()
                .map(|preset| preset.name.as_str()),
            Some("1440 x 1080i 29.97 High Quality H.264")
        );
        let film = super::bluray_format(BdVideo::Hd1440p23, true, VideoStandard::Ntsc);
        assert_eq!(film.automatic, "1440x1080 23.976p MPEG-2");
        assert_eq!(
            super::bluray_presets(&film)
                .get(1)
                .map(|preset| (preset.name.as_str(), preset.kbps)),
            Some(("1440 x 1080p 23.976 Long Form MPEG-2", 8_000))
        );
        let rates = super::hd_parts(
            VideoStandard::Ntsc,
            |found| found.codec == "MPEG-2" && found.dimensions == "1920 x 1080",
            |found| found.rate.to_owned(),
        );
        assert_eq!(rates, ["23.976", "24", "25", "29.97"]);
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

        let interlaced = bluray_format(BdVideo::Hd1080i59, true, VideoStandard::Ntsc);
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
        let film = bluray_format(BdVideo::Hd1080p23, false, VideoStandard::Ntsc);
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
        // M10.39: the progressive formats in MPEG-2 too, named and rated as Encore's.
        let mpeg2_film = bluray_format(BdVideo::Hd1080p23, true, VideoStandard::Ntsc);
        assert_eq!(mpeg2_film.automatic, "1920x1080 23.976p MPEG-2");
        let rated: Vec<(String, u32, u32, u8)> = bluray_presets(&mpeg2_film)
            .into_iter()
            .map(|preset| (preset.name, preset.kbps, preset.max_kbps, preset.quality))
            .collect();
        assert_eq!(
            rated,
            [
                (
                    "1920x1080p 23.976 High Quality MPEG-2".to_owned(),
                    25_000,
                    30_000,
                    5
                ),
                (
                    "1920x1080p 23.976 Long Form MPEG-2".to_owned(),
                    8_000,
                    11_000,
                    3
                ),
                (
                    "1920x1080p 23.976 Maximum Quality MPEG-2".to_owned(),
                    35_000,
                    40_000,
                    5
                ),
            ]
        );
        let fast = bluray_format(BdVideo::Hd720p59, true, VideoStandard::Ntsc);
        assert_eq!(fast.automatic, "1280x720 59.94p MPEG-2");
        assert_eq!(
            bluray_presets(&fast)
                .first()
                .map(|preset| preset.name.as_str()),
            Some("1280x720p 59.94 High Quality MPEG-2")
        );
        // M10.41: an SD Blu-ray in H.264, named as Encore names it.
        let sd_h264 = bluray_format(BdVideo::Sd, false, VideoStandard::Ntsc);
        assert_eq!(sd_h264.automatic, "SD H.264");
        let names: Vec<String> = bluray_presets(&sd_h264)
            .into_iter()
            .map(|preset| preset.name)
            .collect();
        assert_eq!(
            names,
            [
                "NTSC SD H.264 High Quality H.264",
                "NTSC SD H.264 Long Form H.264",
                "NTSC SD H.264 Maximum Quality H.264",
            ]
        );
        let sd = bluray_format(BdVideo::Sd, true, VideoStandard::Ntsc);
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

    /// M10.42: Encore's whole Blu-ray list (the user's screenshots of an NTSC project, 2026-11-21): 69 presets by
    /// name (every format of the standard in both codecs, the DVD ones, SD in both codecs), then 20 Automatics (SD,
    /// the interlaced formats from the widest, the progressive ones by rate and from the widest, MPEG-2 first); what
    /// the project cannot make is greyed with the reason.
    #[test]
    fn the_whole_blu_ray_list_is_encores() {
        use super::{ListPick, bluray_list};
        use disccraft_project::{BdCodec, BdVideo, DiscKind, DiscSettings};
        let hd = DiscSettings {
            kind: DiscKind::BluRay,
            bd_video: BdVideo::Hd1080p23,
            ..DiscSettings::default()
        };
        let (presets, automatic) = bluray_list(&hd, (BdVideo::Hd1080p23, false), &[]);
        assert_eq!((presets.len(), automatic.len()), (69, 20));
        let names: Vec<&str> = presets.iter().map(|entry| entry.name.as_str()).collect();
        // The order of the screenshots, from their first lines, their turns and their last lines.
        let seen = [
            "1280x720p 23.976 High Quality H.264",
            "1280x720p 23.976 High Quality MPEG-2",
            "1280x720p 23.976 Long Form H.264",
            "1280x720p 59.94 Maximum Quality MPEG-2",
            "1440 x 1080i 29.97 High Quality H.264",
            "1440 x 1080p 23.976 Maximum Quality MPEG-2",
            "1440 x 1080p 24 High Quality H.264",
            "1920x1080i 29.97 High Quality H.264",
            "1920x1080p 24 Maximum Quality MPEG-2",
            "NTSC DV 23.976 High quality 4Mb VBR 2 Pass",
            "NTSC DV Low quality 4Mb CBR 1 Pass",
            "NTSC Progressive High quality 7Mb VBR 2 Pass",
            "NTSC SD H.264 High Quality H.264",
            "NTSC SD H.264 Maximum Quality H.264",
            "NTSC SD MPEG-2 High Quality",
            "NTSC SD MPEG-2 Long Form MPEG-2",
            "NTSC SD MPEG-2 Maximum Quality",
        ];
        let places: Vec<usize> = seen
            .iter()
            .map(|name| names.iter().position(|found| found == name).unwrap())
            .collect();
        assert!(
            places
                .windows(2)
                .all(|pair| matches!(pair, [before, after] if before < after)),
            "{names:?}"
        );
        assert_eq!(places.first(), Some(&0));
        assert_eq!(places.last(), Some(&68));
        let order: Vec<(BdVideo, bool)> = automatic
            .iter()
            .map(|entry| match entry.pick {
                ListPick::Automatic { video, mpeg2 } => (video, mpeg2),
                _ => panic!("{entry:?}"),
            })
            .collect();
        assert_eq!(
            order
                .iter()
                .step_by(2)
                .map(|(video, _)| *video)
                .collect::<Vec<_>>(),
            [
                BdVideo::Sd,
                BdVideo::Hd1080i59,
                BdVideo::Hd1440i59,
                BdVideo::Hd1080p23,
                BdVideo::Hd1440p23,
                BdVideo::Hd720p23,
                BdVideo::Hd1080p24,
                BdVideo::Hd1440p24,
                BdVideo::Hd720p24,
                BdVideo::Hd720p59,
            ]
        );
        assert!(
            order
                .chunks(2)
                .all(|pair| matches!(pair, [(_, true), (_, false)]))
        );
        // An HD disc: every HD format, no SD (nor the DVD presets).
        let open = |entries: &[super::ListEntry], name: &str| {
            entries
                .iter()
                .find(|entry| entry.name == name)
                .map(|entry| entry.unavailable.is_none())
        };
        assert_eq!(
            open(&presets, "1280x720p 59.94 High Quality MPEG-2"),
            Some(true)
        );
        assert_eq!(
            open(&presets, "NTSC SD H.264 High Quality H.264"),
            Some(false)
        );
        assert_eq!(
            open(&presets, "NTSC DV High quality 7Mb VBR 2 Pass"),
            Some(false)
        );
        // An SD disc in MPEG-2: SD MPEG-2 and the DVD presets but film; not HD, nor SD H.264.
        let sd = DiscSettings {
            kind: DiscKind::BluRay,
            ..DiscSettings::default()
        };
        let (presets, automatic) = bluray_list(&sd, (BdVideo::Sd, true), &[]);
        assert_eq!(open(&presets, "NTSC SD MPEG-2 High Quality"), Some(true));
        assert_eq!(
            open(&presets, "NTSC SD H.264 High Quality H.264"),
            Some(false)
        );
        assert_eq!(
            open(&presets, "NTSC DV High quality 7Mb VBR 2 Pass"),
            Some(true)
        );
        assert_eq!(
            open(&presets, "NTSC DV 23.976 High quality 7Mb VBR 2 Pass"),
            Some(false)
        );
        assert_eq!(
            open(&presets, "1920x1080i 29.97 High Quality MPEG-2"),
            Some(false)
        );
        assert!(matches!(
            automatic.as_slice(),
            [first, second, ..] if first.unavailable.is_none() && second.unavailable.is_some()
        ));
        // An SD disc in H.264: SD H.264 only.
        let sd_h264 = DiscSettings {
            bd_codec: BdCodec::H264,
            ..sd
        };
        let (presets, _) = bluray_list(&sd_h264, (BdVideo::Sd, false), &[]);
        assert_eq!(open(&presets, "NTSC SD H.264 Long Form H.264"), Some(true));
        assert_eq!(open(&presets, "NTSC SD MPEG-2 High Quality"), Some(false));
        assert_eq!(
            open(&presets, "NTSC DV High quality 7Mb VBR 2 Pass"),
            Some(false)
        );
    }

    /// M10.43: a PAL project's whole list (Encore's DCPALLIST, 2026-11-21): 25 and 50 only (no 23.976 nor 24), 29
    /// presets and 8 Automatics: SD, 1920x1080i, 1440x1080i, 1280x720 50p.
    #[test]
    fn a_pal_projects_whole_list_is_encores() {
        use super::{ListPick, bluray_list};
        use disccraft_project::{BdVideo, DiscKind, DiscSettings};
        let hd = DiscSettings {
            kind: DiscKind::BluRay,
            ..DiscSettings::default()
        };
        let pal = DiscSettings {
            standard: disccraft_project::VideoStandard::Pal,
            bd_video: BdVideo::Hd1080i50,
            ..hd
        };
        let (presets, automatic) = bluray_list(&pal, (BdVideo::Hd1080i50, false), &[]);
        assert_eq!((presets.len(), automatic.len()), (29, 8));
        let names: Vec<&str> = presets.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names.first(), Some(&"1280x720p 50 High Quality H.264"));
        assert_eq!(names.get(6), Some(&"1440 x 1080i 25 High Quality H.264"));
        assert_eq!(names.get(18), Some(&"PAL DV High quality 4Mb VBR 2 Pass"));
        assert_eq!(names.last(), Some(&"PAL SD MPEG-2 Maximum Quality"));
        assert!(
            !names
                .iter()
                .any(|name| name.contains("29.97") || name.contains("23.976"))
        );
        let order: Vec<BdVideo> = automatic
            .iter()
            .step_by(2)
            .map(|entry| match entry.pick {
                ListPick::Automatic { video, .. } => video,
                _ => panic!("{entry:?}"),
            })
            .collect();
        assert_eq!(
            order,
            [
                BdVideo::Sd,
                BdVideo::Hd1080i50,
                BdVideo::Hd1440i50,
                BdVideo::Hd720p50
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
