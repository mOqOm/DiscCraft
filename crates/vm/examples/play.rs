//! `cargo run -p disccraft-vm --example play -- <VIDEO_TS> <step>…` — steps: up, down, left, right, enter, finish (end the playing title).
//! Prints the location after every step.
use disccraft_vm::{Key, Vm, load_video_ts};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().ok_or("usage: play <VIDEO_TS> <steps…>")?;
    let model = load_video_ts(std::path::Path::new(&dir))?;
    let mut vm = Vm::start(&model)?;
    println!("start  -> {:?}", vm.location());
    for step in args {
        let location = match step.as_str() {
            "up" => *vm.press(Key::Up)?,
            "down" => *vm.press(Key::Down)?,
            "left" => *vm.press(Key::Left)?,
            "right" => *vm.press(Key::Right)?,
            "enter" => *vm.press(Key::Enter)?,
            "finish" => *vm.finish_title()?,
            other => return Err(format!("unknown step {other}").into()),
        };
        println!("{step:<6} -> {location:?}");
    }
    Ok(())
}
