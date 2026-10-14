//! Multichannel float images for NukeCraft.
//!
//! An [`Image`] holds any number of named channels (`rgba.red`, `depth.Z`, `mylayer.u`…) over a
//! pixel box, its **bbox** (Nuke's data window), which may be smaller or larger than the format.
//! Rows are stored bottom-up to match Nuke's y-up pixel grid. Reading outside the bbox repeats
//! the nearest edge pixel, as Nuke does; a node that wants black outside its data adds a
//! one-pixel black border (Crop's and Transform's "black outside").

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::fmt;

use nukecraft_geom::IRect;
use serde::{Deserialize, Serialize};

/// The most pixels one image plane may hold (16 384 × 16 384). Larger requests are an error,
/// never an allocation.
pub const MAX_PIXELS: u64 = 16_384 * 16_384;

/// The most channels one image may carry (Nuke's own limit is 1023).
pub const MAX_CHANNELS: usize = 1023;

/// A channel name, `layer.channel` (`rgba.red`). Bare names are completed the way Nuke reads
/// them: `r`/`red` → `rgba.red`, …, `Z` → `depth.Z`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Channel(String);

/// An ordered set of channels: `rgba` first in red, green, blue, alpha order, then the other
/// layers by name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelSet(Vec<Channel>);

/// A multichannel float image over a bbox.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    bbox: IRect,
    channels: ChannelSet,
    /// One plane per channel, in `channels` order, rows bottom-up.
    planes: Vec<Vec<f32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RasterError {
    TooLarge { width: u32, height: u32 },
    TooManyChannels(usize),
    BadChannel(String),
}

pub type Result<T> = std::result::Result<T, RasterError>;

impl fmt::Display for RasterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RasterError::TooLarge { width, height } => write!(f, "image of {width}×{height} pixels is larger than the {MAX_PIXELS}-pixel limit"),
            RasterError::TooManyChannels(n) => write!(f, "{n} channels is more than the {MAX_CHANNELS}-channel limit"),
            RasterError::BadChannel(c) => write!(f, "`{c}` is not a channel name"),
        }
    }
}

impl std::error::Error for RasterError {}

impl Channel {
    /// Parses a channel name, completing Nuke's short forms.
    pub fn parse(name: &str) -> Result<Channel> {
        let name = name.trim();
        let full = match name {
            "r" | "red" => "rgba.red",
            "g" | "green" => "rgba.green",
            "b" | "blue" => "rgba.blue",
            "a" | "alpha" => "rgba.alpha",
            "Z" | "z" => "depth.Z",
            other => other,
        };
        let (layer, chan) = full.split_once('.').ok_or_else(|| RasterError::BadChannel(name.into()))?;
        let ok = |s: &str| !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok(layer) || !ok(chan) {
            return Err(RasterError::BadChannel(name.into()));
        }
        Ok(Channel(full.to_string()))
    }

    pub fn red() -> Channel {
        Channel("rgba.red".into())
    }
    pub fn green() -> Channel {
        Channel("rgba.green".into())
    }
    pub fn blue() -> Channel {
        Channel("rgba.blue".into())
    }
    pub fn alpha() -> Channel {
        Channel("rgba.alpha".into())
    }

    pub fn name(&self) -> &str {
        &self.0
    }

    pub fn layer(&self) -> &str {
        self.0.split_once('.').map_or(self.0.as_str(), |(l, _)| l)
    }

    /// Sort key: the rgba layer first, then by layer; within a layer red, green, blue and alpha
    /// come first (in that order, as Nuke lists them), then the other channels by name.
    fn order(&self) -> (u8, &str, u8, &str) {
        let (layer, chan) = self.0.split_once('.').unwrap_or((self.0.as_str(), ""));
        let rank = match chan {
            "red" => 0,
            "green" => 1,
            "blue" => 2,
            "alpha" => 3,
            _ => 4,
        };
        (if layer == "rgba" { 0 } else { 1 }, layer, rank, &self.0)
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ChannelSet {
    pub fn new() -> ChannelSet {
        ChannelSet(Vec::new())
    }

    pub fn rgb() -> ChannelSet {
        ChannelSet(vec![Channel::red(), Channel::green(), Channel::blue()])
    }

    pub fn rgba() -> ChannelSet {
        ChannelSet(vec![Channel::red(), Channel::green(), Channel::blue(), Channel::alpha()])
    }

    /// Parses a channel-set knob value: `rgb`, `rgba`, `alpha`, `all`, `none`, a layer name
    /// (`depth`), or space/comma-separated channel names. `all` is resolved against `available`.
    pub fn parse(text: &str, available: &ChannelSet) -> Result<ChannelSet> {
        let mut set = ChannelSet::new();
        for word in text.split([' ', ',']).map(str::trim).filter(|w| !w.is_empty()) {
            match word {
                "none" => {}
                "all" => set = set.union(available),
                "rgb" => set = set.union(&ChannelSet::rgb()),
                "rgba" => set = set.union(&ChannelSet::rgba()),
                "alpha" | "a" => set.insert(Channel::alpha()),
                w if !w.contains('.') && !matches!(w, "r" | "g" | "b" | "red" | "green" | "blue" | "z" | "Z") => {
                    // A layer name: every available channel of that layer, else the channels of
                    // one of Nuke's built-in layers.
                    let found: Vec<Channel> = available.iter().filter(|c| c.layer() == w).cloned().collect();
                    let found = if found.is_empty() { builtin_layer(w).ok_or_else(|| RasterError::BadChannel(w.into()))? } else { found };
                    for c in found {
                        set.insert(c);
                    }
                }
                w => set.insert(Channel::parse(w)?),
            }
        }
        Ok(set)
    }

    pub fn insert(&mut self, c: Channel) {
        if let Err(pos) = self.0.binary_search_by(|x| x.order().cmp(&c.order())) {
            self.0.insert(pos, c);
        }
    }

    pub fn contains(&self, c: &Channel) -> bool {
        self.0.binary_search_by(|x| x.order().cmp(&c.order())).is_ok()
    }

    pub fn index_of(&self, c: &Channel) -> Option<usize> {
        self.0.binary_search_by(|x| x.order().cmp(&c.order())).ok()
    }

    pub fn union(&self, o: &ChannelSet) -> ChannelSet {
        let mut s = self.clone();
        for c in o.iter() {
            s.insert(c.clone());
        }
        s
    }

    pub fn intersect(&self, o: &ChannelSet) -> ChannelSet {
        ChannelSet(self.0.iter().filter(|c| o.contains(c)).cloned().collect())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Channel> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The set as a knob value (`rgba`, `rgb` or the channel names).
    pub fn to_knob_string(&self) -> String {
        if *self == ChannelSet::rgba() {
            "rgba".into()
        } else if *self == ChannelSet::rgb() {
            "rgb".into()
        } else {
            self.0.iter().map(Channel::name).collect::<Vec<_>>().join(" ")
        }
    }
}

/// The channels of Nuke's built-in layers other than rgba, which exist even before any image has
/// them: `depth`, `forward`, `backward`, `motion` (forward and backward) and `mask`.
pub fn builtin_layer(name: &str) -> Option<Vec<Channel>> {
    let names: &[&str] = match name {
        "depth" => &["depth.Z"],
        "forward" => &["forward.u", "forward.v"],
        "backward" => &["backward.u", "backward.v"],
        "motion" => &["forward.u", "forward.v", "backward.u", "backward.v"],
        "mask" => &["mask.a"],
        _ => return None,
    };
    Some(names.iter().map(|n| Channel(n.to_string())).collect())
}

impl FromIterator<Channel> for ChannelSet {
    fn from_iter<I: IntoIterator<Item = Channel>>(iter: I) -> ChannelSet {
        let mut s = ChannelSet::new();
        for c in iter {
            s.insert(c);
        }
        s
    }
}

impl Image {
    /// A black (all-zero) image. Errors instead of allocating more than [`MAX_PIXELS`] per
    /// plane or [`MAX_CHANNELS`] planes.
    pub fn new(bbox: IRect, channels: ChannelSet) -> Result<Image> {
        let bbox = if bbox.is_empty() { IRect::EMPTY } else { bbox };
        if bbox.area() > MAX_PIXELS {
            return Err(RasterError::TooLarge { width: bbox.width(), height: bbox.height() });
        }
        if channels.len() > MAX_CHANNELS {
            return Err(RasterError::TooManyChannels(channels.len()));
        }
        let n = bbox.area() as usize;
        let planes = channels.iter().map(|_| vec![0.0; n]).collect();
        Ok(Image { bbox, channels, planes })
    }

    /// An image with every pixel of each channel set to `values[i]` (missing values are 0).
    pub fn filled(bbox: IRect, channels: ChannelSet, values: &[f32]) -> Result<Image> {
        let mut img = Image::new(bbox, channels)?;
        for (i, p) in img.planes.iter_mut().enumerate() {
            let v = values.get(i).copied().unwrap_or(0.0);
            p.fill(v);
        }
        Ok(img)
    }

    pub fn bbox(&self) -> IRect {
        self.bbox
    }

    pub fn channels(&self) -> &ChannelSet {
        &self.channels
    }

    pub fn width(&self) -> u32 {
        self.bbox.width()
    }

    pub fn height(&self) -> u32 {
        self.bbox.height()
    }

    /// The plane of a channel (rows bottom-up), if the image has it.
    pub fn plane(&self, c: &Channel) -> Option<&[f32]> {
        self.channels.index_of(c).and_then(|i| self.planes.get(i)).map(Vec::as_slice)
    }

    pub fn plane_mut(&mut self, c: &Channel) -> Option<&mut [f32]> {
        let i = self.channels.index_of(c)?;
        self.planes.get_mut(i).map(Vec::as_mut_slice)
    }

    /// Planes by index, in `channels()` order.
    pub fn plane_at(&self, i: usize) -> Option<&[f32]> {
        self.planes.get(i).map(Vec::as_slice)
    }

    pub fn plane_at_mut(&mut self, i: usize) -> Option<&mut [f32]> {
        self.planes.get_mut(i).map(Vec::as_mut_slice)
    }

    /// Adds a black channel if the image doesn't have it yet.
    pub fn add_channel(&mut self, c: Channel) -> Result<()> {
        if self.channels.contains(&c) {
            return Ok(());
        }
        if self.channels.len() >= MAX_CHANNELS {
            return Err(RasterError::TooManyChannels(self.channels.len() + 1));
        }
        self.channels.insert(c.clone());
        let i = self.channels.index_of(&c).unwrap_or(self.planes.len());
        let n = self.bbox.area() as usize;
        self.planes.insert(i.min(self.planes.len()), vec![0.0; n]);
        Ok(())
    }

    fn offset(&self, x: i32, y: i32) -> Option<usize> {
        if !self.bbox.contains(x, y) {
            return None;
        }
        let w = self.bbox.width() as usize;
        Some((y - self.bbox.y0) as usize * w + (x - self.bbox.x0) as usize)
    }

    /// The value at an integer pixel, repeating edge pixels outside the bbox. 0 for a channel
    /// the image lacks or an empty image.
    pub fn get(&self, c: &Channel, x: i32, y: i32) -> f32 {
        match self.channels.index_of(c) {
            Some(i) => self.get_at(i, x, y),
            None => 0.0,
        }
    }

    /// [`Image::get`] by plane index.
    pub fn get_at(&self, i: usize, x: i32, y: i32) -> f32 {
        if self.bbox.is_empty() {
            return 0.0;
        }
        let x = x.clamp(self.bbox.x0, self.bbox.x1 - 1);
        let y = y.clamp(self.bbox.y0, self.bbox.y1 - 1);
        self.offset(x, y).and_then(|o| self.planes.get(i).and_then(|p| p.get(o))).copied().unwrap_or(0.0)
    }

    /// The value at an integer pixel, 0 outside the bbox.
    pub fn get_or_zero_at(&self, i: usize, x: i32, y: i32) -> f32 {
        self.offset(x, y).and_then(|o| self.planes.get(i).and_then(|p| p.get(o))).copied().unwrap_or(0.0)
    }

    /// Sets a pixel; ignored outside the bbox or for a missing channel.
    pub fn set_at(&mut self, i: usize, x: i32, y: i32, v: f32) {
        if let Some(o) = self.offset(x, y)
            && let Some(px) = self.planes.get_mut(i).and_then(|p| p.get_mut(o))
        {
            *px = v;
        }
    }

    /// A copy covering `bbox`, reading through [`Image::get`] (edge repeat), with the given
    /// channels (missing channels are black).
    pub fn reframed(&self, bbox: IRect, channels: &ChannelSet) -> Result<Image> {
        let mut out = Image::new(bbox, channels.clone())?;
        let ob = out.bbox;
        for (oi, c) in channels.iter().enumerate() {
            let Some(si) = self.channels.index_of(c) else { continue };
            for y in ob.y0..ob.y1 {
                for x in ob.x0..ob.x1 {
                    out.set_at(oi, x, y, self.get_at(si, x, y));
                }
            }
        }
        Ok(out)
    }

    /// The image with a one-pixel black border added around its bbox, so that the repeated
    /// edge outside it is black ("black outside").
    pub fn with_black_border(&self) -> Result<Image> {
        if self.bbox.is_empty() {
            return Ok(self.clone());
        }
        let mut out = Image::new(self.bbox.expand(1, 1), self.channels.clone())?;
        for i in 0..self.planes.len() {
            for y in self.bbox.y0..self.bbox.y1 {
                for x in self.bbox.x0..self.bbox.x1 {
                    out.set_at(i, x, y, self.get_or_zero_at(i, x, y));
                }
            }
        }
        Ok(out)
    }

    /// Bilinear sample at a real position (pixel centres at `x + 0.5`), edge-repeating.
    pub fn sample_bilinear(&self, i: usize, x: f64, y: f64) -> f32 {
        if !x.is_finite() || !y.is_finite() {
            return 0.0;
        }
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
        let (x0, y0) = (clamp_i32(x0), clamp_i32(y0));
        let p = |dx: i32, dy: i32| self.get_at(i, x0.saturating_add(dx), y0.saturating_add(dy));
        let top = p(0, 1) * (1.0 - tx) + p(1, 1) * tx;
        let bottom = p(0, 0) * (1.0 - tx) + p(1, 0) * tx;
        bottom * (1.0 - ty) + top * ty
    }

    /// Every value is finite (no NaN or infinity).
    pub fn is_finite(&self) -> bool {
        self.planes.iter().all(|p| p.iter().all(|v| v.is_finite()))
    }
}

fn clamp_i32(v: f64) -> i32 {
    v.clamp(i32::MIN as f64 / 2.0, i32::MAX as f64 / 2.0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names() {
        assert_eq!(Channel::parse("r").unwrap(), Channel::red());
        assert_eq!(Channel::parse("depth.Z").unwrap().layer(), "depth");
        assert!(Channel::parse("bad name").is_err());
        assert!(Channel::parse("noLayer").is_err());
        let all: ChannelSet = [Channel::parse("depth.Z").unwrap(), Channel::alpha(), Channel::red()].into_iter().collect();
        let names: Vec<&str> = all.iter().map(Channel::name).collect();
        assert_eq!(names, ["rgba.red", "rgba.alpha", "depth.Z"]);
        assert_eq!(ChannelSet::parse("rgb", &all).unwrap(), ChannelSet::rgb());
        assert_eq!(ChannelSet::parse("depth", &all).unwrap().len(), 1);
        assert_eq!(ChannelSet::parse("all", &all).unwrap(), all);
        assert!(ChannelSet::parse("nosuchlayer", &all).is_err());
        let names: Vec<String> = ChannelSet::parse("motion", &ChannelSet::rgba()).unwrap().iter().map(|c| c.name().to_string()).collect();
        assert_eq!(names, ["backward.u", "backward.v", "forward.u", "forward.v"], "a built-in layer the image lacks");
        // Within any layer: red, green, blue, alpha, then the rest by name.
        let matte: ChannelSet = ["matte.alpha", "matte.u", "matte.red", "matte.blue", "matte.green"].iter().map(|c| Channel::parse(c).unwrap()).collect();
        let names: Vec<&str> = matte.iter().map(Channel::name).collect();
        assert_eq!(names, ["matte.red", "matte.green", "matte.blue", "matte.alpha", "matte.u"]);
    }

    #[test]
    fn edge_pixels_repeat_outside_the_bbox() {
        let mut img = Image::new(IRect::new(0, 0, 2, 2), ChannelSet::rgba()).unwrap();
        img.set_at(0, 1, 1, 5.0);
        assert_eq!(img.get_at(0, 1, 1), 5.0);
        assert_eq!(img.get_at(0, 100, 100), 5.0);
        assert_eq!(img.get_at(0, -100, -100), 0.0);
        assert_eq!(img.get_or_zero_at(0, 100, 100), 0.0);
        let bordered = img.with_black_border().unwrap();
        assert_eq!(bordered.bbox(), IRect::new(-1, -1, 3, 3));
        assert_eq!(bordered.get_at(0, 100, 100), 0.0);
        assert_eq!(bordered.get_at(0, 1, 1), 5.0);
    }

    #[test]
    fn oversized_images_are_errors() {
        assert!(matches!(Image::new(IRect::new(0, 0, 1 << 20, 1 << 20), ChannelSet::rgba()), Err(RasterError::TooLarge { .. })));
        let empty = Image::new(IRect::new(5, 5, 0, 0), ChannelSet::rgba()).unwrap();
        assert_eq!(empty.get_at(0, 0, 0), 0.0);
        assert_eq!(empty.sample_bilinear(0, f64::NAN, 1.0), 0.0);
    }

    #[test]
    fn bilinear_sampling_hits_pixel_centres() {
        let mut img = Image::new(IRect::new(0, 0, 2, 1), ChannelSet::rgb()).unwrap();
        img.set_at(0, 0, 0, 0.0);
        img.set_at(0, 1, 0, 1.0);
        assert!((img.sample_bilinear(0, 0.5, 0.5) - 0.0).abs() < 1e-6);
        assert!((img.sample_bilinear(0, 1.5, 0.5) - 1.0).abs() < 1e-6);
        assert!((img.sample_bilinear(0, 1.0, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn reframe_and_add_channel() {
        let img = Image::filled(IRect::new(0, 0, 2, 2), ChannelSet::rgb(), &[1.0, 2.0, 3.0]).unwrap();
        let out = img.reframed(IRect::new(-1, -1, 3, 3), &ChannelSet::rgba()).unwrap();
        assert_eq!(out.get(&Channel::blue(), -1, 2), 3.0);
        assert_eq!(out.get(&Channel::alpha(), 0, 0), 0.0);
        let mut img = img;
        img.add_channel(Channel::alpha()).unwrap();
        assert_eq!(img.channels(), &ChannelSet::rgba());
        assert_eq!(img.get(&Channel::green(), 0, 0), 2.0);
    }
}
