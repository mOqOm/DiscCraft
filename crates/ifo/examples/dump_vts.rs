//! Prints the title PGCs of a VTS IFO (debug helper): `cargo run -p disccraft-ifo --example dump_vts -- <VTS_xx_0.IFO>`.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: dump_vts <VTS_xx_0.IFO>")?;
    let v = disccraft_ifo::read_vtsi(&std::fs::read(path)?)?;
    for (index, pgc) in v.pgcs.iter().enumerate() {
        println!(
            "title {} pgc time {:02x?} programs {:?} playback mode {:#04x}",
            index + 1,
            pgc.playback_time.0,
            pgc.program_entry_cells,
            pgc.playback_mode
        );
        for c in &pgc.cells {
            println!(
                "  cell {} time {:02x?} ({:?} @90k) sectors {}..{} last vobu {}{}",
                c.cell_id,
                c.playback_time.0,
                c.playback_time.to_90k(),
                c.first_sector,
                c.last_sector,
                c.last_vobu_start,
                if c.stc_discontinuity { " stc" } else { "" }
            );
        }
        println!(
            "  pre {:02x?}\n  post {:02x?}",
            pgc.pre.iter().map(|c| c.0).collect::<Vec<_>>(),
            pgc.post.iter().map(|c| c.0).collect::<Vec<_>>()
        );
    }
    for (index, tmap) in v.tmaps.iter().enumerate() {
        println!(
            "tmap {} unit {} s entries {}",
            index + 1,
            tmap.unit,
            tmap.entries.len()
        );
    }
    Ok(())
}
