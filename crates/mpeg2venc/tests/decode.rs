use disccraft_mpeg2venc::{Frame420, Standard, StillOptions, encode_still};
use filmcraft_mpeg2v::{Decoder, Picture};

fn default_options(standard: Standard) -> StillOptions {
    StillOptions {
        standard,
        quantiser_scale_code: 4,
        max_bytes: 224 * 1024,
        wide: false,
    }
}

fn flat_frame(width: u16, height: u16, value: u8) -> Frame420 {
    let w = usize::from(width);
    let h = usize::from(height);
    let cw = w / 2;
    let ch = h / 2;

    Frame420 {
        width,
        height,
        y: vec![value; w * h],
        cb: vec![128; cw * ch],
        cr: vec![128; cw * ch],
    }
}

fn gradient_bars_frame() -> Frame420 {
    let width = 720u16;
    let height = 480u16;
    let w = usize::from(width);
    let h = usize::from(height);
    let cw = w / 2;
    let ch = h / 2;

    let mut y = vec![0u8; w * h];
    let mut cb = vec![0u8; cw * ch];
    let mut cr = vec![0u8; cw * ch];

    for row in 0..h {
        for col in 0..w {
            let luma = 16 + (col * 219) / (w - 1);
            let index = row * w + col;
            if let Some(slot) = y.get_mut(index) {
                *slot = luma as u8;
            }
        }
    }

    let bar_width = cw / 8;
    for cy in 0..ch {
        for cx in 0..cw {
            let bar = cx / bar_width.max(1);
            let cb_value = 16 + (bar as u8 * 30).min(224);
            let cr_value = 16 + ((7 - bar) as u8 * 30).min(224);
            let index = cy * cw + cx;

            if let Some(slot) = cb.get_mut(index) {
                *slot = cb_value;
            }
            if let Some(slot) = cr.get_mut(index) {
                *slot = cr_value;
            }
        }
    }

    Frame420 {
        width,
        height,
        y,
        cb,
        cr,
    }
}

fn checkerboard_frame() -> Frame420 {
    let width = 720u16;
    let height = 480u16;
    let w = usize::from(width);
    let h = usize::from(height);
    let cw = w / 2;
    let ch = h / 2;

    let mut y = vec![0u8; w * h];
    let cb = vec![128u8; cw * ch];
    let cr = vec![128u8; cw * ch];

    for row in 0..h {
        for col in 0..w {
            let block = (row / 16 + col / 16) % 2;
            let value = if block == 0 { 16 } else { 235 };
            let index = row * w + col;
            if let Some(slot) = y.get_mut(index) {
                *slot = value;
            }
        }
    }

    Frame420 {
        width,
        height,
        y,
        cb,
        cr,
    }
}

fn decode_one(data: &[u8]) -> Result<Picture, String> {
    let mut decoder = Decoder::new();
    let mut pictures = decoder.decode(data, 0).map_err(|e| e.to_string())?;
    pictures.extend(decoder.flush());

    if pictures.len() != 1 {
        return Err(format!("expected one picture, got {}", pictures.len()));
    }

    if decoder.errors() != 0 {
        return Err(format!("decoder reported {} errors", decoder.errors()));
    }

    pictures.pop().ok_or_else(|| "no picture".to_string())
}

fn luma_psnr(decoded: &Picture, input: &Frame420) -> f64 {
    let mut mse = 0.0f64;
    let mut count = 0usize;

    for (decoded_value, input_value) in decoded.y.iter().zip(input.y.iter()) {
        let diff = i32::from(*decoded_value) - i32::from(*input_value);
        mse += f64::from(diff * diff);
        count += 1;
    }

    if count == 0 {
        return 0.0;
    }

    mse /= count as f64;
    if mse == 0.0 {
        return f64::INFINITY;
    }

    10.0 * ((255.0f64 * 255.0) / mse).log10()
}

#[test]
fn flat_ntsc_decodes() -> Result<(), String> {
    let frame = flat_frame(720, 480, 128);
    let data = encode_still(&frame, &default_options(Standard::Ntsc)).map_err(|e| e.to_string())?;
    assert!(data.len() < 224 * 1024);

    let picture = decode_one(&data)?;
    assert_eq!(picture.width, 720);
    assert_eq!(picture.height, 480);

    let psnr = luma_psnr(&picture, &frame);
    assert!(psnr > 38.0);
    Ok(())
}

#[test]
fn gradient_bars_ntsc_decodes() -> Result<(), String> {
    let frame = gradient_bars_frame();
    let data = encode_still(&frame, &default_options(Standard::Ntsc)).map_err(|e| e.to_string())?;
    assert!(data.len() < 224 * 1024);

    let picture = decode_one(&data)?;
    assert_eq!(picture.width, 720);
    assert_eq!(picture.height, 480);

    let psnr = luma_psnr(&picture, &frame);
    assert!(psnr > 38.0);
    Ok(())
}

#[test]
fn checkerboard_ntsc_decodes() -> Result<(), String> {
    let frame = checkerboard_frame();
    let data = encode_still(&frame, &default_options(Standard::Ntsc)).map_err(|e| e.to_string())?;
    assert!(data.len() < 224 * 1024);

    let picture = decode_one(&data)?;
    assert_eq!(picture.width, 720);
    assert_eq!(picture.height, 480);

    let psnr = luma_psnr(&picture, &frame);
    assert!(psnr > 30.0);
    Ok(())
}

#[test]
fn flat_pal_decodes() -> Result<(), String> {
    let frame = flat_frame(720, 576, 128);
    let data = encode_still(&frame, &default_options(Standard::Pal)).map_err(|e| e.to_string())?;
    assert!(data.len() < 224 * 1024);

    let picture = decode_one(&data)?;
    assert_eq!(picture.width, 720);
    assert_eq!(picture.height, 576);

    let psnr = luma_psnr(&picture, &frame);
    assert!(psnr > 38.0);
    Ok(())
}
