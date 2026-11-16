//! `mixdown <session.scraft> <out.wav>`: bounce a SoundCraft session's mix (a check of the SoundCraft integration).

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(session), Some(out)) = (args.get(1), args.get(2)) else {
        return Err("usage: mixdown <session.scraft> <out.wav>".into());
    };
    let mix = disccraft_sound::mixdown(std::path::Path::new(session), std::path::Path::new(out))?;
    println!(
        "{:.2} s, {:?} LUFS, peak {:?} dB",
        mix.seconds, mix.lufs, mix.peak_db
    );
    Ok(())
}
