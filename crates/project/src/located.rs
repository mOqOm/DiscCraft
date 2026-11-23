//! Encore's File ▸ Locate Transcoded File ▸ DVD… / Blu-ray…: a video asset remembers the files it was transcoded
//! to for each kind of disc (made elsewhere, or by an earlier build whose source is gone), and a build of that kind
//! takes them in place of the asset's own file.

use std::path::Path;

use crate::ids::AssetId;
use crate::io::ProjectError;
use crate::model::{AudioTrack, LocatedTranscode, Project};

impl Project {
    /// Whether any asset has transcoded files located for the project's kind of disc.
    #[must_use]
    pub fn has_located_transcodes(&self) -> bool {
        let kind = self.settings.kind;
        self.assets
            .values()
            .any(|asset| asset.located(kind).is_some())
    }

    /// The project as a build of its kind of disc sees it: each video asset with transcoded files located for that
    /// kind (whose video file is there) is those files. Its file becomes the transcoded video, which needs no
    /// transcode of its own. Its transcoded sound, when it has one, becomes a sound asset of its own: it takes the
    /// video asset's place on audio tracks, and the timelines that play the video without audio tracks of their
    /// own get it as their soundtrack (cut as their video is).
    ///
    /// # Errors
    ///
    /// When an asset for a transcoded sound cannot be added (the project's ids are used up).
    pub fn with_located_transcodes(&self) -> Result<Project, ProjectError> {
        let kind = self.settings.kind;
        let located: Vec<(AssetId, LocatedTranscode)> = self
            .assets
            .values()
            .filter_map(|asset| {
                let found = asset.located(kind)?;
                Path::new(&found.video)
                    .is_file()
                    .then(|| (asset.id, found.clone()))
            })
            .collect();
        let mut project = self.clone();
        for (id, found) in located {
            if let Some(asset) = project.assets.get_mut(&id) {
                asset.path = found.video.clone();
                asset.composition = None;
                asset.offline = false;
                asset.transcode_kbps = None;
                asset.transcode_preset = None;
                asset.bluray_transcode = None;
            }
            let Some(audio) = found
                .audio
                .as_deref()
                .filter(|audio| Path::new(audio).is_file())
            else {
                continue;
            };
            let sound = project.add_asset(Path::new(audio))?;
            let language = project.settings.audio_language.clone().unwrap_or_default();
            for timeline in project.timelines.values_mut() {
                for track in &mut timeline.audio {
                    if track.asset == id {
                        track.asset = sound;
                    }
                    for clip in &mut track.more {
                        if clip.asset == id {
                            clip.asset = sound;
                        }
                    }
                }
                // The video's own sound: a soundtrack of one clip, cut as the video clip is.
                if timeline.video == id
                    && timeline.audio.is_empty()
                    && timeline.more_video.is_empty()
                {
                    let trim = timeline.video_trims.first().copied().unwrap_or_default();
                    timeline.audio.push(AudioTrack {
                        asset: sound,
                        stream: 0,
                        language: language.clone(),
                        start: 0,
                        trim_start: trim.start,
                        trim_end: trim.end,
                        more: Vec::new(),
                    });
                }
            }
        }
        Ok(project)
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{AssetKind, ClipTrim, DiscKind, LocatedTranscode, Project};
    use std::error::Error;
    use std::path::Path;

    /// A DVD build sees the located files: the video asset is the transcoded video, and its sound a new asset that
    /// the timeline without audio tracks takes, cut as its video; a Blu-ray build (nothing located for it) and
    /// files that are gone change nothing.
    #[test]
    fn located_files_take_the_assets_place() -> Result<(), Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-located-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let (video, audio) = (dir.join("movie.m2v"), dir.join("movie.ac3"));
        std::fs::write(&video, b"video")?;
        std::fs::write(&audio, b"audio")?;
        let mut project = Project::default();
        let source = project.add_asset(Path::new("gone.mp4"))?;
        let plain = project.add_timeline(source, Some("Plain".to_owned()))?;
        let with_track = project.add_timeline(source, Some("Track".to_owned()))?;
        project
            .timeline_mut(plain)
            .ok_or("timeline")?
            .video_trims
            .push(ClipTrim {
                start: 30,
                end: Some(300),
            });
        let music = project.add_asset(Path::new("music.wav"))?;
        project
            .timeline_mut(with_track)
            .ok_or("timeline")?
            .audio
            .push(crate::AudioTrack {
                asset: music,
                stream: 0,
                language: "en".to_owned(),
                start: 0,
                trim_start: 0,
                trim_end: None,
                more: Vec::new(),
            });
        project.settings.audio_language = Some("ja".to_owned());
        project.assets.get_mut(&source).ok_or("asset")?.located_dvd = Some(LocatedTranscode {
            video: video.to_string_lossy().into_owned(),
            audio: Some(audio.to_string_lossy().into_owned()),
        });
        assert!(project.has_located_transcodes());

        let seen = project.with_located_transcodes()?;
        let asset = seen.assets.get(&source).ok_or("asset")?;
        assert_eq!(Path::new(&asset.path), video);
        let track = seen
            .timeline(plain)
            .and_then(|timeline| timeline.audio.first())
            .ok_or("soundtrack")?;
        let sound = seen.assets.get(&track.asset).ok_or("sound asset")?;
        assert_eq!(Path::new(&sound.path), audio);
        assert_eq!(sound.kind, AssetKind::Audio);
        assert_eq!(
            (track.language.as_str(), track.trim_start, track.trim_end),
            ("ja", 30, Some(300))
        );
        // A timeline with an audio track of its own keeps it.
        assert_eq!(
            seen.timeline(with_track).map(|timeline| timeline
                .audio
                .iter()
                .map(|track| track.asset)
                .collect()),
            Some(vec![music])
        );

        // Nothing located for a Blu-ray disc; and a located video that is gone is not taken.
        let mut bluray = project.clone();
        bluray.settings.kind = DiscKind::BluRay;
        assert!(!bluray.has_located_transcodes());
        assert_eq!(bluray.with_located_transcodes()?, bluray);
        std::fs::remove_file(&video)?;
        assert_eq!(project.with_located_transcodes()?, project);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
