//! `cargo run -p disccraft-udf --example make_iso -- <VIDEO_TS folder> <out.iso> [label]` — wraps a VIDEO_TS folder in a DVD ISO image.
use disccraft_udf::image::{ImageFile, dvd_file_order, write_dvd_iso};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .ok_or("usage: make_iso <VIDEO_TS> <out.iso> [label]")?,
    );
    let out = PathBuf::from(args.next().ok_or("missing output")?);
    let label = args.next().unwrap_or_else(|| "DISCCRAFT".to_string());
    let names: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    let files: Vec<ImageFile> = dvd_file_order(&names)
        .into_iter()
        .map(|n| ImageFile {
            source: dir.join(&n),
            name: n,
        })
        .collect();
    let report = write_dvd_iso(&files, &label, &out, 1_762_560_000)?;
    println!(
        "{} sectors, first file at {}",
        report.sectors, report.first_file_sector
    );
    Ok(())
}
