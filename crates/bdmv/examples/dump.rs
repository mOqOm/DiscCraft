//! Print the playlists and clip information of a `BDMV` folder.
//!
//! `cargo run -p disccraft-bdmv --example dump -- <folder that contains BDMV>`

use std::error::Error;
use std::fs;
use std::path::Path;

use disccraft_bdmv::clpi::read_clpi;
use disccraft_bdmv::index::read_index;
use disccraft_bdmv::mobj::read_movie_objects;
use disccraft_bdmv::mpls::read_mpls;

fn main() -> Result<(), Box<dyn Error>> {
    let root = std::env::args().nth(1).ok_or("usage: dump <folder>")?;
    let bdmv = Path::new(&root).join("BDMV");
    println!("{:?}", read_index(&fs::read(bdmv.join("index.bdmv"))?)?);
    for (i, object) in read_movie_objects(&fs::read(bdmv.join("MovieObject.bdmv"))?)?
        .iter()
        .enumerate()
    {
        let commands: Vec<String> = object
            .commands
            .iter()
            .map(|c| c.0.iter().map(|b| format!("{b:02x}")).collect())
            .collect();
        println!("object {i}: {commands:?}");
    }
    for dir in ["PLAYLIST", "CLIPINF"] {
        let mut names: Vec<_> = fs::read_dir(bdmv.join(dir))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        names.sort();
        for path in names {
            let data = fs::read(&path)?;
            println!("== {}", path.display());
            if dir == "PLAYLIST" {
                println!("{:#?}", read_mpls(&data)?);
            } else {
                let clip = read_clpi(&data)?;
                println!(
                    "rate {} packets {} start {} end {} ep_pid {:#x} streams {:?}",
                    clip.ts_recording_rate,
                    clip.source_packets,
                    clip.presentation_start,
                    clip.presentation_end,
                    clip.ep_pid,
                    clip.streams
                );
                for entry in clip.ep_map.iter().take(4) {
                    println!("  ep pts {} spn {}", entry.pts, entry.spn);
                }
                println!("  ... {} entries", clip.ep_map.len());
            }
        }
    }
    Ok(())
}
