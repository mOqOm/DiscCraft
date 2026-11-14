//! Byte-exact checks of the attribute, command and PGC encodings against docs/formats/dvd-video.md.

use disccraft_ifo::{
    AudioAttr, AudioCoding, BcdTime, Cell, Pgc, Standard, VideoAttr, VmCommand, read_pgc, write_pgc,
};

#[test]
fn video_attributes() {
    assert_eq!(
        VideoAttr {
            standard: Standard::Ntsc,
            wide: false
        }
        .to_bytes(),
        // 4:3: neither automatic pan/scan nor letterbox (they apply to 16:9 only).
        [0x43, 0x00]
    );
    assert_eq!(
        VideoAttr {
            standard: Standard::Pal,
            wide: false
        }
        .to_bytes(),
        [0x53, 0x00]
    );
    assert_eq!(
        VideoAttr {
            standard: Standard::Ntsc,
            wide: true
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
    };
    assert_eq!(ac3.to_bytes(), [0x04, 0x01, b'j', b'a', 0, 0, 0, 0]);
    let lpcm = AudioAttr {
        coding: AudioCoding::Lpcm,
        channels: 2,
        language: None,
    };
    assert_eq!(lpcm.to_bytes(), [0x80, 0x01, 0, 0, 0, 0, 0, 0]);
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
        palette: Pgc::default_palette(),
    };
    let bytes = write_pgc(&pgc).unwrap();
    assert_eq!(read_pgc(&bytes).unwrap(), pgc);
    // Every prefix is rejected or parsed without panicking.
    for end in 0..bytes.len() {
        let _ = read_pgc(&bytes[..end]);
    }
}
