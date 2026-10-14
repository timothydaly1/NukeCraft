//! OpenEXR via the pure-Rust `exr` crate (first layer, flat images).
//!
//! EXR coordinates run y-down from the display window's top-left; NukeCraft's run y-up from the
//! format's bottom-left. With display window `D` (height `H`), file pixel `(ex, ey)` is
//! NukeCraft pixel `(ex − D.x, D.y + H − 1 − ey)`. The data window becomes the bbox, so a
//! bbox larger or smaller than the format survives a round trip.
//!
//! Channel names map the way Nuke reads them: `R G B A` → `rgba.*`, `Z` → `depth.Z`,
//! `layer.R` → `layer.red`, anything else → `layer.name` or `other.name`.

use std::path::Path;

use exr::prelude::{self as x, ReadChannels, ReadLayers, WritableImage};
use nukecraft_geom::{Format, IRect, MAX_COORD};
use nukecraft_raster::{Channel, ChannelSet, Image, MAX_CHANNELS, MAX_PIXELS};

use crate::{Decoded, FileInfo, Result, file_error};

/// NukeCraft's name for an EXR channel name.
fn channel_from_exr(name: &str) -> Option<Channel> {
    let (layer, chan) = match name.rsplit_once('.') {
        Some((l, c)) => (Some(l), c),
        None => (None, name),
    };
    let chan = match chan {
        "R" | "r" => "red",
        "G" | "g" => "green",
        "B" | "b" => "blue",
        "A" | "a" => "alpha",
        other => other,
    };
    let clean = |s: &str| -> String { s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).take(64).collect() };
    let full = match (layer, chan) {
        (None, "red" | "green" | "blue" | "alpha") => format!("rgba.{chan}"),
        (None, "Z") => "depth.Z".to_string(),
        (None, c) => format!("other.{}", clean(c)),
        (Some(l), c) => format!("{}.{}", clean(l), clean(c)),
    };
    Channel::parse(&full).ok()
}

/// The EXR name for a NukeCraft channel.
fn channel_to_exr(c: &Channel) -> String {
    match c.name() {
        "rgba.red" => "R".into(),
        "rgba.green" => "G".into(),
        "rgba.blue" => "B".into(),
        "rgba.alpha" => "A".into(),
        "depth.Z" => "Z".into(),
        other => {
            let (layer, chan) = other.split_once('.').unwrap_or(("other", other));
            let chan = match chan {
                "red" => "R",
                "green" => "G",
                "blue" => "B",
                "alpha" => "A",
                c => c,
            };
            format!("{layer}.{chan}")
        }
    }
}

/// Bounds of a header: (format, bbox), checked against NukeCraft's limits.
fn bounds(path: &Path, display: x::IntegerBounds, data: x::IntegerBounds, aspect: f32) -> Result<(Format, IRect)> {
    let ok = |v: usize| v > 0 && v <= MAX_COORD as usize;
    if !ok(display.size.0) || !ok(display.size.1) || data.size.0 as u64 * data.size.1 as u64 > MAX_PIXELS {
        return Err(file_error(path, "image size outside the supported range"));
    }
    let format = Format::new(display.size.0 as u32, display.size.1 as u32, aspect as f64);
    let h = display.size.1 as i64;
    let x0 = data.position.0 as i64 - display.position.0 as i64;
    let top = data.position.1 as i64 - display.position.1 as i64;
    let y1 = h - top;
    let y0 = y1 - data.size.1 as i64;
    let c = |v: i64| v.clamp(-(MAX_COORD as i64), MAX_COORD as i64) as i32;
    Ok((format, IRect::new(c(x0), c(y0), c(x0 + data.size.0 as i64), c(y1))))
}

pub fn read_info(path: &Path) -> Result<FileInfo> {
    let meta = x::MetaData::read_from_file(path, false).map_err(|e| file_error(path, e))?;
    let header = meta.headers.first().ok_or_else(|| file_error(path, "no image layers"))?;
    let (format, bbox) = bounds(path, header.shared_attributes.display_window, header.data_window(), header.shared_attributes.pixel_aspect)?;
    let channels: ChannelSet = header.channels.list.iter().filter_map(|c| channel_from_exr(&c.name.to_string())).take(MAX_CHANNELS).collect();
    Ok(FileInfo { format, bbox, channels })
}

pub fn read(path: &Path) -> Result<Decoded> {
    // Check the header first so a hostile size never reaches the decoder's allocations.
    read_info(path)?;
    let image = x::read()
        .no_deep_data()
        .largest_resolution_level()
        .all_channels()
        .first_valid_layer()
        .all_attributes()
        .from_file(path)
        .map_err(|e| file_error(path, e))?;
    let layer = &image.layer_data;
    let data = x::IntegerBounds::new(layer.attributes.layer_position, layer.size);
    let (format, bbox) = bounds(path, image.attributes.display_window, data, image.attributes.pixel_aspect)?;
    let (w, h) = (layer.size.0, layer.size.1);
    let named: Vec<(Channel, &x::AnyChannel<x::FlatSamples>)> =
        layer.channel_data.list.iter().filter_map(|c| channel_from_exr(&c.name.to_string()).map(|n| (n, c))).take(MAX_CHANNELS).collect();
    let channels: ChannelSet = named.iter().map(|(n, _)| n.clone()).collect();
    let mut img = Image::new(bbox, channels).map_err(|e| file_error(path, e))?;
    for (name, chan) in &named {
        let Some(i) = img.channels().index_of(name) else { continue };
        for (k, v) in chan.sample_data.values_as_f32().enumerate().take(w * h) {
            let (col, row) = (k % w, k / w);
            img.set_at(i, bbox.x0 + col as i32, bbox.y1 - 1 - row as i32, v);
        }
    }
    Ok(Decoded { format, image: img })
}

/// Writes every channel of `img` with its bbox as the data window (half or float samples,
/// ZIP-compressed).
pub fn write(path: &Path, img: &Image, format: &Format, bits: u8) -> Result<()> {
    let bbox = if img.bbox().is_empty() { IRect::new(0, 0, 1, 1) } else { img.bbox() };
    let (w, h) = (bbox.width() as usize, bbox.height() as usize);
    let mut list = Vec::new();
    for (i, c) in img.channels().iter().enumerate() {
        let mut values = Vec::with_capacity(w * h);
        for row in 0..h {
            let y = bbox.y1 - 1 - row as i32;
            for x0 in 0..w {
                values.push(img.get_or_zero_at(i, bbox.x0 + x0 as i32, y));
            }
        }
        let samples = if bits >= 32 { x::FlatSamples::F32(values) } else { x::FlatSamples::F16(values.into_iter().map(x::f16::from_f32).collect()) };
        list.push(x::AnyChannel::new(channel_to_exr(c).as_str(), samples));
    }
    if list.is_empty() {
        return Err(file_error(path, "no channels to write"));
    }
    let fh = format.height as i64;
    let top = (fh - bbox.y1 as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    let attrs = x::LayerAttributes { layer_position: x::Vec2(bbox.x0, top), ..x::LayerAttributes::default() };
    let channels = x::AnyChannels::sort(list.into_iter().collect());
    let layer = x::Layer::new((w, h), attrs, x::Encoding::SMALL_LOSSLESS, channels);
    let mut image = x::Image::from_layer(layer);
    image.attributes.display_window = x::IntegerBounds::new((0, 0), (format.width as usize, format.height as usize));
    image.attributes.pixel_aspect = format.pixel_aspect as f32;
    image.write().to_file(path).map_err(|e| file_error(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names_map_like_nuke() {
        let n = |s: &str| channel_from_exr(s).unwrap().name().to_string();
        assert_eq!(n("R"), "rgba.red");
        assert_eq!(n("Z"), "depth.Z");
        assert_eq!(n("diffuse.R"), "diffuse.red");
        assert_eq!(n("motion.u"), "motion.u");
        assert_eq!(n("weird name"), "other.weird_name");
        for name in ["rgba.red", "depth.Z", "diffuse.red", "motion.u"] {
            let c = Channel::parse(name).unwrap();
            assert_eq!(channel_from_exr(&channel_to_exr(&c)).unwrap(), c);
        }
    }
}
