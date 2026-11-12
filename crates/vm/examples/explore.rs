//! `cargo run -p disccraft-vm --example explore -- <VIDEO_TS>` — simulates every remote-control path and prints what is reachable.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args()
        .nth(1)
        .ok_or("usage: explore <VIDEO_TS folder>")?;
    let model = disccraft_vm::load_video_ts(std::path::Path::new(&dir))?;
    let report = disccraft_vm::explore(&model);
    println!("menus reached: {:?}", report.menus_seen);
    println!(
        "chapters reached (title, chapter): {:?}",
        report.chapters_reached
    );
    println!("dead ends: {:?}", report.dead_ends);
    println!("loops: {:?}", report.loops);
    Ok(())
}
