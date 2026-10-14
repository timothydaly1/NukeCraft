//! PNG (ISO/IEC 15948) via the pure-Rust `png` crate. PNG rows run top-down; NukeCraft's run
//! bottom-up, so row `r` of the file is `y = height − 1 − r`.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use nukecraft_geom::Format;
use nukecraft_raster::{Channel, ChannelSet, Image, MAX_PIXELS};

use crate::{Decoded, FileInfo, Result, file_error};

/// Decoder allocation limit: the largest image NukeCraft accepts at 16-bit RGBA.
const DECODE_LIMIT_BYTES: usize = 1 << 31;

fn decoder(path: &Path) -> Result<png::Decoder<BufReader<File>>> {
    let file = File::open(path).map_err(|e| file_error(path, e))?;
    let mut d = png::Decoder::new_with_limits(BufReader::new(file), png::Limits { bytes: DECODE_LIMIT_BYTES });
    d.set_transformations(png::Transformations::EXPAND);
    Ok(d)
}

fn channels_for(color: png::ColorType) -> ChannelSet {
    match color {
        png::ColorType::Grayscale | png::ColorType::Rgb | png::ColorType::Indexed => ChannelSet::rgb(),
        png::ColorType::GrayscaleAlpha | png::ColorType::Rgba => ChannelSet::rgba(),
    }
}

fn check_size(path: &Path, w: u32, h: u32) -> Result<()> {
    if w as u64 * h as u64 > MAX_PIXELS || w == 0 || h == 0 {
        return Err(file_error(path, format!("{w}×{h} is outside the supported image size")));
    }
    Ok(())
}

pub fn read_info(path: &Path) -> Result<FileInfo> {
    let reader = decoder(path)?.read_info().map_err(|e| file_error(path, e))?;
    let info = reader.info();
    check_size(path, info.width, info.height)?;
    let format = Format::new(info.width, info.height, 1.0);
    Ok(FileInfo { bbox: format.rect(), format, channels: channels_for(info.color_type) })
}

pub fn read(path: &Path) -> Result<Decoded> {
    let mut reader = decoder(path)?.read_info().map_err(|e| file_error(path, e))?;
    let (w, h) = (reader.info().width, reader.info().height);
    check_size(path, w, h)?;
    let size = reader.output_buffer_size().ok_or_else(|| file_error(path, "image too large"))?;
    let mut buf = vec![0u8; size];
    let out = reader.next_frame(&mut buf).map_err(|e| file_error(path, e))?;
    let format = Format::new(w, h, 1.0);
    let channels = channels_for(out.color_type);
    let mut img = Image::new(format.rect(), channels).map_err(|e| file_error(path, e))?;
    let samples: usize = match out.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(file_error(path, "palette was not expanded")),
    };
    let wide = out.bit_depth == png::BitDepth::Sixteen;
    let bytes = if wide { 2 } else { 1 };
    let max = if wide { 65535.0 } else { 255.0 };
    for row in 0..h as usize {
        let y = (h as usize - 1 - row) as i32;
        let line = buf.get(row * out.line_size..(row + 1) * out.line_size).ok_or_else(|| file_error(path, "truncated image data"))?;
        for x in 0..w as usize {
            let sample = |s: usize| -> f32 {
                let at = (x * samples + s) * bytes;
                let v = if wide {
                    line.get(at..at + 2).and_then(|b| Some(u16::from_be_bytes([*b.first()?, *b.get(1)?]))).unwrap_or(0) as f32
                } else {
                    line.get(at).copied().unwrap_or(0) as f32
                };
                v / max
            };
            let (rgb, alpha) = match samples {
                1 => ([sample(0); 3], None),
                2 => ([sample(0); 3], Some(sample(1))),
                3 => ([sample(0), sample(1), sample(2)], None),
                _ => ([sample(0), sample(1), sample(2)], Some(sample(3))),
            };
            for (c, v) in rgb.iter().enumerate() {
                img.set_at(c, x as i32, y, *v);
            }
            if let Some(a) = alpha {
                img.set_at(3, x as i32, y, a);
            }
        }
    }
    Ok(Decoded { format, image: img })
}

/// Encodes 8-bit RGBA (rows top-down) into PNG bytes.
pub fn encode_rgba8(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mem = Path::new("<memory>");
    check_size(mem, w, h)?;
    if rgba.len() != w as usize * h as usize * 4 {
        return Err(file_error(mem, "pixel buffer does not match the size"));
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| file_error(mem, e))?;
    writer.write_image_data(rgba).map_err(|e| file_error(mem, e))?;
    writer.finish().map_err(|e| file_error(mem, e))?;
    Ok(out)
}

/// Writes RGB or RGBA (8 or 16 bit) over the format; outside the image's bbox is black.
pub fn write(path: &Path, img: &Image, format: &Format, bits: u8) -> Result<()> {
    let (w, h) = (format.width, format.height);
    check_size(path, w, h)?;
    let has_alpha = img.channels().contains(&Channel::alpha());
    let chans: Vec<Option<usize>> =
        [Channel::red(), Channel::green(), Channel::blue()].into_iter().chain(has_alpha.then(Channel::alpha)).map(|c| img.channels().index_of(&c)).collect();
    let wide = bits == 16;
    let mut data = Vec::with_capacity(w as usize * h as usize * chans.len() * if wide { 2 } else { 1 });
    for row in 0..h {
        let y = (h - 1 - row) as i32;
        for x in 0..w as i32 {
            for c in &chans {
                let v = c.map_or(0.0, |i| img.get_or_zero_at(i, x, y));
                let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
                if wide {
                    data.extend_from_slice(&((v * 65535.0 + 0.5) as u16).to_be_bytes());
                } else {
                    data.push((v * 255.0 + 0.5) as u8);
                }
            }
        }
    }
    let file = File::create(path).map_err(|e| file_error(path, e))?;
    let mut enc = png::Encoder::new(BufWriter::new(file), w, h);
    enc.set_color(if has_alpha { png::ColorType::Rgba } else { png::ColorType::Rgb });
    enc.set_depth(if wide { png::BitDepth::Sixteen } else { png::BitDepth::Eight });
    let mut writer = enc.write_header().map_err(|e| file_error(path, e))?;
    writer.write_image_data(&data).map_err(|e| file_error(path, e))?;
    writer.finish().map_err(|e| file_error(path, e))
}
