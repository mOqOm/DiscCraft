//! DV in a MOV / MP4 or a Matroska file (M6.413): the media readers open those containers themselves and ask the
//! registered decoder factories for a decoder of each video track's sample entry; this one answers for DV (a
//! QuickTime `dvc ` / `dvcp` / `dvpp` entry, or a Matroska track whose VFW codec is `dvsd`) with disccraft-dv.
//! Every DV sample is one whole frame, coded on its own. Raw DV files and AVIs of DV are read by [`crate::dv_source`].

use std::sync::Once;

use craft_media::isobmff::SampleEntry;
use filmcraft_codecs::video::{DecodedFrame, VideoDecoder};
use filmcraft_codecs::{CodecError, Result};
use filmcraft_time::Tick;

/// Registers the DV decoder with the media readers (once; later calls do nothing).
pub(crate) fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| filmcraft_codecs::register_video_decoder(factory));
}

/// A DV decoder for `entry` when its format is DV.
fn factory(entry: &SampleEntry) -> Option<Result<Box<dyn VideoDecoder>>> {
    crate::dv_source::dv_handler(&entry.format.0)
        .then(|| Ok(Box::new(DvDecoder) as Box<dyn VideoDecoder>))
}

/// Decodes each DV sample (one frame) as it comes.
struct DvDecoder;

impl VideoDecoder for DvDecoder {
    fn decode(&mut self, sample: &[u8], pts: i64) -> Result<Vec<DecodedFrame>> {
        let header =
            disccraft_dv::header(sample).map_err(|error| CodecError::Decode(error.to_string()))?;
        let picture = disccraft_dv::decode_video(sample)
            .map_err(|error| CodecError::Decode(error.to_string()))?;
        Ok(vec![DecodedFrame {
            pts,
            frame: crate::dv_source::video_frame(Some(picture), &header, Tick(0)),
            draft: false,
        }])
    }

    fn flush(&mut self) -> Vec<DecodedFrame> {
        Vec::new()
    }

    fn reset(&mut self) {}

    fn name(&self) -> &str {
        "DV"
    }

    fn intra_only(&self) -> bool {
        true
    }
}
