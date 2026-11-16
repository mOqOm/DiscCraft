//! Checks of the streams themselves (Encore's 高すぎるタイムラインビットレート and 字幕テキストのオーバーフロー).

use crate::warning;
use disccraft_dvd::TimelineRate;
use disccraft_project::{DiscKind, Problem, Project};

/// Appends the timelines whose streams ask more of a DVD than it plays, and the subtitles whose text does not fit.
pub(crate) fn check(project: &Project, problems: &mut Vec<Problem>) {
    if project.settings.kind == DiscKind::Dvd {
        for rate in disccraft_dvd::timeline_rates(project) {
            if let Some(problem) = rate_problem(&rate) {
                problems.push(problem);
            }
        }
    }
    check_gaps(project, problems);
    for overflow in disccraft_dvd::streams::overflowing_subtitles(project) {
        // One result a subtitle, with its track and its in and out points, as Encore lists them.
        let place = match overflow.span {
            Some((start, end)) => format!(
                "subtitle track {}, {} to {}",
                overflow.track,
                disccraft_project::format_time(start, &project.settings),
                disccraft_project::format_time(end, &project.settings)
            ),
            None => format!("slide {}", overflow.number),
        };
        problems.push(warning(
            "subtitleTextOverflow",
            format!(
                "{} {} subtitle overflow: {place}",
                overflow.kind, overflow.id
            ),
        ));
    }
}

/// The subtitles of a Blu-ray timeline that follow the one before too closely (Encore's 字幕の間隔).
pub(crate) fn check_gaps(project: &Project, problems: &mut Vec<Problem>) {
    for gap in disccraft_dvd::streams::close_subtitles(project) {
        problems.push(warning(
            "subtitleSpacing",
            format!(
                "timeline {} subtitle gap of {} frame(s): subtitle track {}, {} to {}",
                gap.timeline,
                gap.frames,
                gap.track,
                disccraft_project::format_time(gap.span.0, &project.settings),
                disccraft_project::format_time(gap.span.1, &project.settings)
            ),
        ));
    }
}

/// A timeline whose video (used as it is) and audio together are over the DVD's limit.
fn rate_problem(rate: &TimelineRate) -> Option<Problem> {
    (rate.total() > TimelineRate::LIMIT).then(|| {
        warning(
            "timelineBitrateTooHigh",
            format!(
                "timeline {} bit rate {} exceeds {} bits per second (video {}, audio {})",
                rate.timeline,
                rate.total(),
                TimelineRate::LIMIT,
                rate.video,
                rate.audio
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::rate_problem;
    use disccraft_dvd::TimelineRate;

    /// Video and audio together over 9.8 Mbit/s are reported; at the limit they are not.
    #[test]
    fn a_rate_over_the_dvd_limit_is_reported() {
        let rate = |video, audio| TimelineRate {
            timeline: 7,
            video,
            audio,
        };
        assert!(rate_problem(&rate(8_000_000, 1_536_000)).is_none());
        assert!(rate_problem(&rate(9_608_000, 192_000)).is_none());
        let problem = rate_problem(&rate(9_000_000, 1_536_000));
        assert_eq!(
            problem.map(|problem| (problem.code, problem.message)),
            Some((
                "timelineBitrateTooHigh".to_owned(),
                "timeline 7 bit rate 10536000 exceeds 9800000 bits per second (video 9000000, audio 1536000)"
                    .to_owned()
            ))
        );
    }
}
