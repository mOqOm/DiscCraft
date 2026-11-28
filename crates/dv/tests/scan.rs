//! M6.569: a frame's field scan comes from its VAUX source control pack (ITU-R BT.1618-1 Table 14): IL = 0 is
//! progressive, FS = 1 field 1 (the lower field) first, FS = 0 field 2 (the upper field) first; without the pack DV's
//! usual lower field first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use disccraft_dv::{DIF_BLOCK, SEQUENCE, Scan, header};

/// The first DIF sequence of a 525/60 frame: its header, subcode and VAUX blocks, with the VAUX source control pack
/// (PC0 0x61) first in the first VAUX block when `control` gives its PC3.
fn sequence(control: Option<u8>) -> Vec<u8> {
    let mut frame = vec![0u8; SEQUENCE];
    frame[..3].copy_from_slice(&[0x1F, 0x07, 0x00]);
    frame[3] = 0x3F;
    frame[DIF_BLOCK] = 0x3F;
    frame[2 * DIF_BLOCK] = 0x3F;
    frame[3 * DIF_BLOCK] = 0x5F;
    if let Some(pc3) = control {
        let pack = 3 * DIF_BLOCK + 3;
        frame[pack..pack + 5].copy_from_slice(&[0x61, 0x3F, 0xC8, pc3, 0xFF]);
    }
    frame
}

#[test]
fn the_source_control_pack_says_how_the_fields_are_scanned() {
    // FF FS FC IL in the top four bits of PC3, the reserved bits set as the format asks.
    for (pc3, scan) in [
        (0b1111_1100, Scan::LowerFieldFirst),
        (0b1011_1100, Scan::UpperFieldFirst),
        (0b1110_1100, Scan::Progressive),
        (0b1010_1100, Scan::Progressive),
        // A field shown twice (FF = 0) still says which.
        (0b0111_1100, Scan::LowerFieldFirst),
        (0b0011_1100, Scan::UpperFieldFirst),
    ] {
        assert_eq!(
            header(&sequence(Some(pc3))).unwrap().scan,
            scan,
            "PC3 {pc3:08b}"
        );
    }
    assert_eq!(header(&sequence(None)).unwrap().scan, Scan::LowerFieldFirst);
}
