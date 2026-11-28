//! M12.51: NVIDIA's encoder, through its own interface, codes an interlaced format's frames as fields: a packet a
//! frame holding its two field pictures (two access unit delimiters), the parameter sets saying fields may be coded,
//! the first field the top one or the bottom one as asked, and a key frame at each GOP. It runs only where an NVIDIA
//! encoder codes fields (NVIDIA offers it on Blackwell cards); elsewhere it says why and passes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use disccraft_platform::{HwCodec, HwVideoConfig, HwVideoEncoder};
use disccraft_transcode::h264_sps::{first_in_annex_b, top_field_first_in_annex_b};

/// A 1920 x 1080 NV12 frame: a luma ramp moved by `frame`, grey chroma.
fn picture(frame: usize) -> Vec<u8> {
    let (width, height) = (1920, 1080);
    let mut nv12 = Vec::with_capacity(width * height * 3 / 2);
    for row in 0..height {
        nv12.extend(
            (0..width).map(|column| u8::try_from((column + row + frame * 7) % 220 + 16).unwrap()),
        );
    }
    nv12.resize(width * height * 3 / 2, 128);
    nv12
}

/// The access unit delimiters (NAL unit type 9) in Annex B `data`.
fn delimiters(data: &[u8]) -> usize {
    data.windows(4)
        .filter(|start| matches!(start, [0, 0, 1, header] if header & 0x1f == 9))
        .count()
}

#[test]
fn nvidia_codes_each_frame_as_its_two_fields() {
    for top_first in [true, false] {
        let config = HwVideoConfig {
            codec: HwCodec::H264,
            width: 1920,
            height: 1080,
            fps_num: 30_000,
            fps_den: 1001,
            bitrate: 20_000_000,
            max_bitrate: 40_000_000,
            buffer_bits: 30_000_000,
            gop: 12,
            b_frames: 0,
            slices: 4,
            sar: (1, 1),
            cbr: false,
            fields: Some(top_first),
        };
        let mut encoder = match HwVideoEncoder::open(&config) {
            Ok(encoder) => encoder,
            Err(why) => {
                eprintln!("no NVIDIA encoder that codes fields here; skipping ({why})");
                return;
            }
        };
        let mut packets = Vec::new();
        for frame in 0..30 {
            let pts = i64::try_from(frame).unwrap() * 10_010_000 / 30;
            packets.extend(encoder.encode(&picture(frame), pts, false).unwrap());
        }
        packets.extend(encoder.finish().unwrap());
        assert_eq!(packets.len(), 30);
        let keys: Vec<usize> = (0..packets.len()).filter(|at| packets[*at].key).collect();
        assert_eq!(keys, vec![0, 12, 24]);
        for packet in &packets {
            assert_eq!(delimiters(&packet.data), 2, "two field pictures a frame");
        }
        let sps = first_in_annex_b(&packets[0].data).expect("parameter sets before the first IDR");
        assert!(!sps.frame_mbs_only);
        assert_eq!((sps.profile, sps.level), (100, 41));
        assert_eq!(
            top_field_first_in_annex_b(&packets[0].data, &sps),
            Some(top_first)
        );
    }
}
