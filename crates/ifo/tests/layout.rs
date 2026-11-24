//! Byte-exact checks of the attribute, command and PGC encodings against docs/formats/dvd-video.md.

use disccraft_ifo::{
    AudioAttr, AudioCoding, BcdTime, Cell, Pgc, Standard, VideoAttr, VmCommand, read_pgc, write_pgc,
};

#[test]
fn video_attributes() {
    assert_eq!(
        VideoAttr {
            standard: Standard::Ntsc,
            wide: false,
            resolution: disccraft_ifo::Resolution::Full,
            mpeg1: false,
        }
        .to_bytes(),
        // 4:3: neither automatic pan/scan nor letterbox (they apply to 16:9 only).
        [0x43, 0x00]
    );
    assert_eq!(
        VideoAttr {
            standard: Standard::Pal,
            wide: false,
            resolution: disccraft_ifo::Resolution::Full,
            mpeg1: false,
        }
        .to_bytes(),
        [0x53, 0x00]
    );
    assert_eq!(
        VideoAttr {
            standard: Standard::Ntsc,
            wide: true,
            resolution: disccraft_ifo::Resolution::Full,
            mpeg1: false,
        }
        .to_bytes(),
        [0x4e, 0x00]
    );
    // Discs written before (4:3 with both allowed) still read.
    assert!(!VideoAttr::from_bytes([0x40, 0]).unwrap().wide);
    assert_eq!(
        VideoAttr::from_bytes([0x50, 0]).unwrap().standard,
        Standard::Pal
    );
}

#[test]
fn audio_attributes() {
    let ac3 = AudioAttr {
        coding: AudioCoding::Ac3,
        channels: 2,
        language: Some(*b"ja"),
        bits: 16,
        sample_rate: 48_000,
    };
    assert_eq!(ac3.to_bytes(), [0x04, 0x01, b'j', b'a', 0, 0, 0, 0]);
    let lpcm = AudioAttr {
        coding: AudioCoding::Lpcm,
        channels: 2,
        language: None,
        bits: 16,
        sample_rate: 48_000,
    };
    assert_eq!(lpcm.to_bytes(), [0x80, 0x01, 0, 0, 0, 0, 0, 0]);
    // 96 kHz 24-bit stereo LPCM: quantization 2 and sampling frequency 1 above the channels.
    let fine = AudioAttr {
        bits: 24,
        sample_rate: 96_000,
        ..lpcm
    };
    assert_eq!(fine.to_bytes(), [0x80, 0x91, 0, 0, 0, 0, 0, 0]);
    assert_eq!(AudioAttr::from_bytes(fine.to_bytes()).unwrap(), fine);
    // Only LPCM has those; AC-3 with them is not read.
    assert!(AudioAttr::from_bytes([0x00, 0x91, 0, 0, 0, 0, 0, 0]).is_err());
    assert_eq!(AudioAttr::from_bytes(ac3.to_bytes()).unwrap(), ac3);
    assert_eq!(AudioAttr::from_bytes(lpcm.to_bytes()).unwrap(), lpcm);
}

#[test]
fn vm_commands() {
    assert_eq!(VmCommand::jump_tt(1).0, [0x30, 0x02, 0, 0, 0, 1, 0, 0]);
    assert_eq!(VmCommand::exit().0, [0x30, 0x01, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn bcd_time() {
    // 1:02:03 + 4 frames at 29.97 → frame byte 0b11 << 6 | 0x04.
    let t = (3_600 + 2 * 60 + 3) * 90_000 + 4 * 3_003;
    let bcd = BcdTime::from_90k(t, Standard::Ntsc);
    assert_eq!(bcd.0, [0x01, 0x02, 0x03, 0xC4]);
    let pal = BcdTime::from_90k(10 * 90_000 + 12 * 3_600, Standard::Pal);
    assert_eq!(pal.0, [0x00, 0x00, 0x10, 0x52]);
    assert_eq!(pal.to_90k().unwrap(), 10 * 90_000 + 12 * 3_600);
}

fn cell(id: u8, first: u32, last: u32) -> Cell {
    Cell {
        vob_id: 1,
        cell_id: id,
        stc_discontinuity: id == 1,
        seamless: id != 1,
        still_time: 0,
        playback_time: BcdTime::from_90k(5 * 90_000, Standard::Ntsc),
        first_sector: first,
        last_vobu_start: last - 10,
        last_sector: last,
    }
}

#[test]
fn pgc_round_trip() {
    let pgc = Pgc {
        prohibited_ops: 0,
        playback_time: BcdTime::from_90k(15 * 90_000, Standard::Ntsc),
        audio_streams: 1,
        program_entry_cells: vec![1, 2, 3],
        cells: vec![cell(1, 0, 99), cell(2, 100, 199), cell(3, 200, 299)],
        pre: vec![],
        post: vec![VmCommand::exit()],
        cell_commands: vec![],
        still_time: 0,
        playback_mode: 0,
        subpicture_streams: 0,
        letterbox_subpictures: false,
        palette: Pgc::default_palette(),
    };
    let bytes = write_pgc(&pgc).unwrap();
    assert_eq!(bytes[2], 3); // programs
    assert_eq!(bytes[3], 3); // cells
    assert_eq!(bytes[0x0C], 0x80); // audio stream 0 present
    assert_eq!(read_pgc(&bytes).unwrap(), pgc);
}

#[test]
fn first_play_pgc_round_trip() {
    let pgc = Pgc {
        prohibited_ops: 0,
        playback_time: BcdTime([0, 0, 0, 0]),
        audio_streams: 0,
        program_entry_cells: vec![],
        cells: vec![],
        pre: vec![VmCommand::jump_tt(1)],
        post: vec![],
        cell_commands: vec![],
        still_time: 0,
        playback_mode: 0,
        subpicture_streams: 0,
        letterbox_subpictures: false,
        palette: Pgc::default_palette(),
    };
    let bytes = write_pgc(&pgc).unwrap();
    assert_eq!(read_pgc(&bytes).unwrap(), pgc);
    // Every prefix is rejected or parsed without panicking.
    for end in 0..bytes.len() {
        let _ = read_pgc(&bytes[..end]);
    }
}

/// The picture size sits in bits 5 to 3 of the second attribute byte (0 = 720, 1 = 704, 2 = 352 at full height,
/// 3 = 352 at half height), and reads back.
#[test]
fn video_attributes_carry_the_picture_size() {
    use disccraft_ifo::Resolution;
    for (resolution, byte) in [
        (Resolution::Full, 0x00),
        (Resolution::Cropped, 0x08),
        (Resolution::Half, 0x10),
        (Resolution::Quarter, 0x18),
    ] {
        let attr = VideoAttr {
            standard: Standard::Ntsc,
            wide: false,
            resolution,
            mpeg1: false,
        };
        assert_eq!(attr.to_bytes(), [0x43, byte]);
        assert_eq!(VideoAttr::from_bytes([0x43, byte]).unwrap(), attr);
    }
    // M6.350: MPEG-1 has the coding mode bits 0 (NTSC 0x03, PAL 0x13 with the 4:3 flags).
    for (standard, byte) in [(Standard::Ntsc, 0x03), (Standard::Pal, 0x13)] {
        let attr = VideoAttr {
            standard,
            wide: false,
            resolution: Resolution::Quarter,
            mpeg1: true,
        };
        assert_eq!(attr.to_bytes(), [byte, 0x18]);
        assert_eq!(VideoAttr::from_bytes([byte, 0x18]).unwrap(), attr);
    }
    assert_eq!(
        Resolution::of_picture(352, 288, Standard::Pal),
        Some(Resolution::Quarter)
    );
    assert_eq!(Resolution::of_picture(352, 288, Standard::Ntsc), None);
    assert_eq!(Resolution::of_picture(640, 480, Standard::Ntsc), None);
    // Other bits of the second byte are not ours to read.
    assert!(VideoAttr::from_bytes([0x43, 0x80]).is_err());
}
