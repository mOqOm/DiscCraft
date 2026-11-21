//! A DVD title whose chapters have parental alternatives (M13.17): the title becomes a parental block (M13.16), one
//! PGC for each range of player levels ([`Timeline::parental_variants`]). The alternatives are muxed into the
//! title's own VOBS after its last picture, each a chapter of its own of an "extended" timeline, so the title set's
//! sound streams stay one coding; the block's PGCs then list the title's chapters, an alternative's cells in place
//! of a chapter where the version plays it.

use std::path::Path;

use disccraft_ifo::ParentalTable;
use disccraft_project::{
    AudioClip, AudioTrack, Chapter, DiscKind, ParentalVariant, Project, Timeline,
};

use crate::DvdError;
use crate::prepare::Budget;

/// How a title's parental block is made from its extended timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParentalPlan {
    /// The timeline's own chapters: the extended timeline's first ones; the alternatives' follow, one each.
    pub(crate) chapters: usize,
    /// For each of the timeline's parental chapters, in its order: the chapter its alternative stands in for.
    pub(crate) replaced_chapters: Vec<usize>,
    /// The block's versions, the one for the highest levels (every chapter as it is) first.
    pub(crate) variants: Vec<ParentalVariant>,
}

/// Whether a title of `project` is a parental block: a DVD timeline with parental chapters.
pub(crate) fn has_parental_blocks(project: &Project) -> bool {
    project.settings.kind == DiscKind::Dvd
        && project
            .timelines
            .values()
            .any(|timeline| !timeline.parental_variants().is_empty())
}

/// The timeline `timeline` is built from when it has parental chapters (a DVD's), and how its block is made: its
/// clips, then each alternative as a clip and a chapter of its own, its own sound (when it has one) on the first
/// audio track (made from the timeline's video's own sound when the timeline has no audio track but that sound).
/// `None` for a timeline without them.
///
/// # Errors
///
/// When the timeline's or an alternative's video cannot be prepared or read.
pub(crate) fn extended_timeline(
    project: &Project,
    timeline: &Timeline,
    (cache_dir, budget): (&Path, &Budget),
    progress: &mut dyn FnMut(&str, f32),
) -> Result<Option<(Timeline, ParentalPlan)>, DvdError> {
    let variants = timeline.parental_variants();
    if project.settings.kind != DiscKind::Dvd || variants.is_empty() {
        return Ok(None);
    }
    // How many pictures a timeline's video has, and whether it brings its own sound (no audio track of its own).
    let mut frames_of = |one: &Timeline| -> Result<(u32, bool), DvdError> {
        let prepared = crate::prepare::prepare_title(project, one, cache_dir, budget, progress)?;
        let (video, _) = crate::source::load_video(&prepared.video_path)?;
        let frames = u32::try_from(video.pictures.len())
            .map_err(|_| DvdError::Unsupported("a video too long for a title".to_string()))?;
        Ok((frames, prepared.transcoded_audio.is_some()))
    };
    let (mut frames, own_sound) = frames_of(timeline)?;
    let rate = project.settings.frame_rate();
    let mut extended = timeline.clone();
    extended.parental_chapters.clear();
    if extended.audio.is_empty() && own_sound {
        extended.audio.push(AudioTrack::of(timeline.video));
    }
    for parental in &timeline.parental_chapters {
        // The alternative as a timeline of its own (as a clip of the extended one is prepared).
        let mut one = timeline.clone();
        one.video = parental.alternative;
        one.more_video.clear();
        one.video_trims.clear();
        one.video_gaps.clear();
        one.audio.clear();
        one.subtitles.clear();
        one.parental_chapters.clear();
        one.chapters = vec![Chapter {
            time: rate.tick_of(0),
            name: String::new(),
            poster: None,
        }];
        let (length, sound) = frames_of(&one)?;
        extended.more_video.push(parental.alternative);
        extended.chapters.push(Chapter {
            time: rate.tick_of(i64::from(frames)),
            name: String::new(),
            poster: None,
        });
        if let Some(track) = extended.audio.first_mut().filter(|_| sound) {
            track.more.push(AudioClip {
                asset: parental.alternative,
                start: frames,
                trim_start: 0,
                trim_end: None,
            });
        }
        frames = frames
            .checked_add(length)
            .ok_or_else(|| DvdError::Unsupported("a title too long".to_string()))?;
    }
    let plan = ParentalPlan {
        chapters: timeline.chapters.len(),
        replaced_chapters: timeline
            .parental_chapters
            .iter()
            .map(|parental| parental.chapter)
            .collect(),
        variants,
    };
    Ok(Some((extended, plan)))
}

/// The disc's VMG_PTL_MAIT for `vts_count` title sets: every country, level `l`'s mask bit `l - 1` for the VMG and
/// every title set (the bits a version of a block is for, [`ParentalVariant::parental_id`]).
pub(crate) fn parental_table(vts_count: u8) -> ParentalTable {
    let masks = usize::from(vts_count).saturating_add(1);
    ParentalTable {
        countries: COUNTRIES
            .split_whitespace()
            .filter_map(|code| {
                let bytes = code.as_bytes();
                Some([*bytes.first()?, *bytes.get(1)?])
            })
            .collect(),
        levels: std::array::from_fn(|index| vec![1u16 << index; masks]),
    }
}

/// The officially assigned ISO 3166-1 alpha-2 country codes: a player looks its own (SPRM12) up among them.
const COUNTRIES: &str = "AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR \
    BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ \
    FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE \
    JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR \
    MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU \
    RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG \
    UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT ZA ZM ZW";

#[cfg(test)]
mod tests {
    use super::parental_table;

    #[test]
    fn the_table_lists_every_country_once_with_a_bit_a_level() {
        let table = parental_table(3);
        assert_eq!(table.countries.len(), 249);
        let mut sorted = table.countries.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 249);
        assert!(table.countries.contains(b"JP") && table.countries.contains(b"US"));
        assert_eq!(table.levels[0], vec![0x0001; 4]);
        assert_eq!(table.levels[7], vec![0x0080; 4]);
    }
}
