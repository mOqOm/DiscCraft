use crate::io::ProjectError;
use crate::model::{DiscSettings, VideoStandard};
use filmcraft_time::{FrameRate, Tick, format_timecode_frames, parse_timecode};

/// Parse user-typed timecode into exact ticks.
pub fn parse_time(input: &str, settings: &DiscSettings) -> Result<Tick, ProjectError> {
    let drop_frame = input.contains(';');
    let rate = settings.frame_rate();
    let frames = parse_timecode(input, rate, drop_frame, 0)
        .map_err(|e| ProjectError::Timecode(e.to_string()))?;
    if frames < 0 {
        return Err(ProjectError::Timecode("timecode is negative".to_string()));
    }
    Ok(rate.tick_of(frames))
}

/// Format a frame number as a timecode, the way Encore shows it: drop-frame "HH;MM;SS;FF" at the NTSC rates
/// (29.97 / 59.94), otherwise non-drop "HH:MM:SS:FF".
pub fn format_frames(frame: i64, rate: FrameRate) -> String {
    let rate = rate.sane();
    format_timecode_frames(frame, rate, rate.supports_drop_frame())
}

/// Format exact ticks as a timecode string, at the nearest frame (Encore shows 6 s of NTSC as 00;00;06;00, not
/// the 179 whole frames 00;00;05;29).
pub fn format_time(t: Tick, settings: &DiscSettings) -> String {
    let rate = settings.frame_rate();
    let drop_frame = settings.standard == VideoStandard::Ntsc;
    format_timecode_frames(nearest_frame(t, rate), rate, drop_frame)
}

/// Ticks per second of [`Tick`].
const TICKS_PER_SECOND: f64 = 254_016_000_000.0;

/// The frame nearest to `t`.
fn nearest_frame(t: Tick, rate: FrameRate) -> i64 {
    let seconds = t.0 as f64 / TICKS_PER_SECOND;
    let frame = (seconds * rate.as_f64()).round();
    if frame.is_finite() {
        // `as` saturates.
        frame as i64
    } else {
        rate.frame_at(t)
    }
}

#[cfg(test)]
mod tests {
    use super::{format_frames, format_time, parse_time};
    use crate::{DiscSettings, FrameRate, VideoStandard};
    use std::error::Error;

    #[test]
    fn parse_and_format() -> Result<(), Box<dyn Error>> {
        let ntsc = DiscSettings::default();
        let pal = DiscSettings {
            standard: VideoStandard::Pal,
            ..DiscSettings::default()
        };

        let t = parse_time("00:05:00;00", &ntsc)?;
        assert_eq!(t, FrameRate::FPS_29_97.tick_of(8990));

        let t = parse_time("00:05:00;02", &ntsc)?;
        assert_eq!(t, FrameRate::FPS_29_97.tick_of(8992));
        assert_eq!(format_time(t, &ntsc), "00;05;00;02");

        let t = parse_time("00:01:00:00", &pal)?;
        assert_eq!(t, FrameRate::FPS_25.tick_of(1500));
        assert_eq!(format_time(t, &pal), "00:01:00:00");

        assert!(parse_time("-1", &ntsc).is_err());
        Ok(())
    }

    #[test]
    fn frames_use_drop_frame_only_at_ntsc_rates() {
        assert_eq!(format_frames(180, FrameRate::FPS_29_97), "00;00;06;00");
        // Frames 00 and 01 of each minute (but every tenth) are skipped.
        assert_eq!(format_frames(1800, FrameRate::FPS_29_97), "00;01;00;02");
        assert_eq!(format_frames(17982, FrameRate::FPS_29_97), "00;10;00;00");
        assert_eq!(format_frames(150, FrameRate::FPS_25), "00:00:06:00");
        assert_eq!(format_frames(144, FrameRate::FPS_23_976), "00:00:06:00");
        // A damaged rate falls back to the default instead of dividing by zero.
        assert!(!format_frames(10, FrameRate::new(0, 0)).is_empty());
    }
}
