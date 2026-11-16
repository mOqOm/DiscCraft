//! `render <project.ecproj> <out.mp4> [composition]`: render an EffectCraft composition (a check of the
//! EffectCraft integration).

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(project), Some(out)) = (args.get(1), args.get(2)) else {
        return Err("usage: render <project.ecproj> <out.mp4> [composition]".into());
    };
    let rendered = disccraft_motion::render(
        std::path::Path::new(project),
        std::path::Path::new(out),
        args.get(3).map(String::as_str),
        &mut |_| {},
    )?;
    println!("rendered {}", rendered.composition);
    Ok(())
}
