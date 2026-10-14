//! Channel: Shuffle, Copy, ChannelMerge, Remove.
//!
//! These route channels rather than change values. Like the colour nodes they work per pixel:
//! output pixel `(x, y)` reads only pixel `(x, y)` of each input.

use nukecraft_geom::IRect;
use nukecraft_raster::{Channel, ChannelSet, Image};

use crate::color_nodes::{INVERT_MASK, MASK_CHANNEL, MIX};
use crate::{Ctx, ImageInfo, Input, InputSpec, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, input_image, input_info, mask_factors, request_same};

/// Where a classic Shuffle output channel comes from: the input layer's first..fourth channel,
/// or a constant.
const SOURCES: &[&str] = &["red", "green", "blue", "alpha", "black", "white"];

const fn source_knob(name: &'static str, help: &'static str) -> KnobSpec {
    KnobSpec { name, label: name, kind: KnobKind::Choice(SOURCES), default: KnobDefault::Text(name), help }
}

const fn chan(name: &'static str, label: &'static str, default: &'static str, help: &'static str) -> KnobSpec {
    KnobSpec { name, label, kind: KnobKind::Channels, default: KnobDefault::Text(default), help }
}

const SHUFFLE_KNOBS: &[KnobSpec] = &[
    chan("in", "in", "rgba", "The layer the channels come from."),
    chan("out", "out", "rgba", "The layer written (created if the input lacks it)."),
    source_knob("red", "Source of the out layer's first channel."),
    source_knob("green", "Source of the out layer's second channel."),
    source_knob("blue", "Source of the out layer's third channel."),
    source_knob("alpha", "Source of the out layer's fourth channel."),
];

const COPY_KNOBS: &[KnobSpec] = &[
    chan("from0", "copy", "rgba.alpha", "A channel of A ..."),
    chan("to0", "to", "rgba.alpha", "... copied into this channel."),
    chan("from1", "copy", "none", "A channel of A ..."),
    chan("to1", "to", "none", "... copied into this channel."),
    chan("from2", "copy", "none", "A channel of A ..."),
    chan("to2", "to", "none", "... copied into this channel."),
    chan("from3", "copy", "none", "A channel of A ..."),
    chan("to3", "to", "none", "... copied into this channel."),
    chan("channels", "layer copy", "none", "Channels copied from A under the same names."),
    KnobSpec {
        name: "bbox",
        label: "set bbox to",
        kind: KnobKind::Choice(&["union", "B side", "A side", "intersection"]),
        default: KnobDefault::Text("union"),
        help: "Which inputs' data windows the output covers.",
    },
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

const CHANNEL_MERGE_OPS: &[&str] = &["union", "intersect", "stencil", "absminus", "plus", "minus", "from", "multiply", "max", "min", "in", "out"];

const CHANNEL_MERGE_KNOBS: &[KnobSpec] = &[
    chan("A", "A", "rgba.alpha", "The channel of input A."),
    KnobSpec {
        name: "operation",
        label: "operation",
        kind: KnobKind::Choice(CHANNEL_MERGE_OPS),
        default: KnobDefault::Text("union"),
        help: "How the A and B channels combine.",
    },
    chan("B", "B", "rgba.alpha", "The channel of input B."),
    chan("output", "output", "rgba.alpha", "The channel the result is written to."),
    MASK_CHANNEL,
    INVERT_MASK,
    MIX,
];

const REMOVE_KNOBS: &[KnobSpec] = &[
    KnobSpec {
        name: "operation",
        label: "operation",
        kind: KnobKind::Choice(&["remove", "keep"]),
        default: KnobDefault::Text("remove"),
        help: "Remove the listed channels, or keep only them.",
    },
    chan("channels", "channels", "none", "Channels to remove or keep."),
    chan("channels2", "and", "none", "More channels."),
    chan("channels3", "and", "none", "More channels."),
    chan("channels4", "and", "none", "More channels."),
];

const B: usize = 0;
const A: usize = 1;
const MASK: usize = 2;
const AB_MASK: &[InputSpec] = &[InputSpec { name: "B", mask: false }, InputSpec { name: "A", mask: false }, InputSpec { name: "mask", mask: true }];

pub fn specs() -> Vec<NodeSpec> {
    vec![
        NodeSpec {
            class: "Shuffle",
            aliases: &[],
            label: "Shuffle",
            category: "Channel",
            shortcut: None,
            help: "Rearranges the channels of a layer into another layer.",
            inputs: &[InputSpec { name: "", mask: false }],
            extra_inputs: false,
            knobs: SHUFFLE_KNOBS,
            info: shuffle_info,
            request: request_same,
            engine: shuffle_engine,
            input_time: None,
        },
        NodeSpec {
            class: "Copy",
            aliases: &[],
            label: "Copy",
            category: "Channel",
            shortcut: Some("K"),
            help: "Copies channels of A into B.",
            inputs: AB_MASK,
            extra_inputs: false,
            knobs: COPY_KNOBS,
            info: copy_info,
            request: request_same,
            engine: copy_engine,
            input_time: None,
        },
        NodeSpec {
            class: "ChannelMerge",
            aliases: &[],
            label: "ChannelMerge",
            category: "Channel",
            shortcut: None,
            help: "Combines one channel of A with one channel of B.",
            inputs: AB_MASK,
            extra_inputs: false,
            knobs: CHANNEL_MERGE_KNOBS,
            info: channel_merge_info,
            request: request_same,
            engine: channel_merge_engine,
            input_time: None,
        },
        NodeSpec {
            class: "Remove",
            aliases: &[],
            label: "Remove",
            category: "Channel",
            shortcut: None,
            help: "Removes channels, or keeps only some.",
            inputs: &[InputSpec { name: "", mask: false }],
            extra_inputs: false,
            knobs: REMOVE_KNOBS,
            info: remove_info,
            request: request_same,
            engine: remove_engine,
            input_time: None,
        },
    ]
}

/// The channels a layer knob names, in order: a layer of `available` (`depth`), a set (`rgba`,
/// `rgb`, `alpha`), one channel (`depth.Z`), or a new layer, which gets red, green, blue and
/// alpha channels. Empty for `none` or something unreadable.
fn layer_channels(text: &str, available: &ChannelSet) -> Vec<Channel> {
    let text = text.trim();
    if text.is_empty() || text == "none" {
        return Vec::new();
    }
    if let Ok(set) = ChannelSet::parse(text, available) {
        return set.iter().cloned().collect();
    }
    if text.contains('.') || text.contains(' ') {
        return Vec::new();
    }
    ["red", "green", "blue", "alpha"].iter().filter_map(|c| Channel::parse(&format!("{text}.{c}")).ok()).collect()
}

/// One channel knob's channel, if it names exactly one readable channel.
fn one_channel(ctx: &Ctx, name: &str) -> Option<Channel> {
    let text = ctx.knobs.text(name).trim();
    if text.is_empty() || text == "none" {
        return None;
    }
    Channel::parse(text).ok()
}

/// Input `i`'s pixels covering `region` with `channels` (missing ones black).
fn framed(inputs: &[Option<Input>], i: usize, region: IRect, channels: &ChannelSet) -> Result<Image> {
    Ok(match input_image(inputs, i) {
        Some(inp) if inp.image.bbox() == region && inp.image.channels() == channels => inp.image.clone(),
        Some(inp) => inp.image.reframed(region, channels)?,
        None => Image::new(region, channels.clone())?,
    })
}

fn shuffle_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let mut info = input_info(ctx, inputs, 0);
    for c in layer_channels(ctx.knobs.text("out"), &info.channels) {
        info.channels.insert(c);
    }
    Ok(info)
}

fn shuffle_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let empty = Image::new(IRect::EMPTY, ChannelSet::new())?;
    let src = input_image(inputs, 0).map_or(&empty, |i| &i.image);
    let mut out = framed(inputs, 0, region, &info.channels)?;
    let ins = layer_channels(ctx.knobs.text("in"), src.channels());
    let outs = layer_channels(ctx.knobs.text("out"), &info.channels);
    for (j, (to, knob)) in outs.iter().zip(["red", "green", "blue", "alpha"]).enumerate() {
        let Some(oi) = out.channels().index_of(to) else { continue };
        let pick = match ctx.knobs.text(knob) {
            "black" => Pick::Value(0.0),
            "white" => Pick::Value(1.0),
            name => {
                let k = SOURCES.iter().position(|s| *s == name).unwrap_or(j);
                match ins.get(k).and_then(|c| src.channels().index_of(c)) {
                    Some(si) => Pick::Plane(si),
                    None => Pick::Value(0.0),
                }
            }
        };
        for y in region.y0..region.y1 {
            for x in region.x0..region.x1 {
                let v = match pick {
                    Pick::Value(v) => v,
                    Pick::Plane(si) => src.get_at(si, x, y),
                };
                out.set_at(oi, x, y, v);
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Copy)]
enum Pick {
    Value(f32),
    Plane(usize),
}

/// The `bbox` knob of a two-input node.
fn ab_bbox(ctx: &Ctx, a: IRect, b: IRect) -> IRect {
    match ctx.knobs.text("bbox") {
        "B side" => b,
        "A side" => a,
        "intersection" => a.intersect(b),
        _ => a.union(b),
    }
}

/// Copy's `(from, to)` pairs and its layer copy, as channel pairs.
fn copy_pairs(ctx: &Ctx, a_channels: &ChannelSet) -> Vec<(Channel, Channel)> {
    let mut pairs = Vec::new();
    for i in 0..4 {
        if let (Some(from), Some(to)) = (one_channel(ctx, &format!("from{i}")), one_channel(ctx, &format!("to{i}"))) {
            pairs.push((from, to));
        }
    }
    for c in ChannelSet::parse(ctx.knobs.text("channels"), a_channels).unwrap_or_default().iter() {
        pairs.push((c.clone(), c.clone()));
    }
    pairs
}

fn copy_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let b = input_info(ctx, inputs, B);
    let a = input_info(ctx, inputs, A);
    let mut channels = b.channels.clone();
    for (_, to) in copy_pairs(ctx, &a.channels) {
        channels.insert(to);
    }
    Ok(ImageInfo { bbox: ab_bbox(ctx, a.bbox, b.bbox), channels, ..b })
}

fn copy_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let mut out = framed(inputs, B, region, &info.channels)?;
    let Some(a) = input_image(inputs, A) else {
        // Nothing to copy from: the copied channels are black.
        for (_, to) in copy_pairs(ctx, &ChannelSet::new()) {
            if let Some(p) = out.plane_mut(&to) {
                p.fill(0.0);
            }
        }
        return Ok(out);
    };
    let mask = mask_factors(ctx, inputs, MASK, region);
    let m = ctx.knobs.float("mix") as f32;
    let pairs = copy_pairs(ctx, a.image.channels());
    let base = out.clone();
    for (from, to) in pairs {
        let Some(oi) = out.channels().index_of(&to) else { continue };
        let fi = a.image.channels().index_of(&from);
        let mut i = 0usize;
        for y in region.y0..region.y1 {
            for x in region.x0..region.x1 {
                let k = mask.as_ref().map_or(1.0, |mk| mk.get(i).copied().unwrap_or(0.0)) * m;
                let v = fi.map_or(0.0, |fi| a.image.get_at(fi, x, y));
                out.set_at(oi, x, y, crate::mix(base.get_at(oi, x, y), v, k));
                i += 1;
            }
        }
    }
    Ok(out)
}

fn channel_merge_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let b = input_info(ctx, inputs, B);
    let a = input_info(ctx, inputs, A);
    let mut channels = b.channels.clone();
    if let Some(c) = one_channel(ctx, "output") {
        channels.insert(c);
    }
    Ok(ImageInfo { bbox: a.bbox.union(b.bbox), channels, ..b })
}

/// ChannelMerge's operations on the A and B values.
fn channel_op(op: &str, a: f32, b: f32) -> f32 {
    match op {
        "intersect" | "multiply" | "in" => a * b,
        "stencil" => b * (1.0 - a),
        "absminus" => (a - b).abs(),
        "plus" => a + b,
        "minus" => a - b,
        "from" => b - a,
        "max" => a.max(b),
        "min" => a.min(b),
        "out" => a * (1.0 - b),
        _ => a + b - a * b,
    }
}

fn channel_merge_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let mut out = framed(inputs, B, region, &info.channels)?;
    let Some(oi) = one_channel(ctx, "output").and_then(|c| out.channels().index_of(&c)) else { return Ok(out) };
    let empty = Image::new(IRect::EMPTY, ChannelSet::new())?;
    let a_img = input_image(inputs, A).map_or(&empty, |i| &i.image);
    let b_img = input_image(inputs, B).map_or(&empty, |i| &i.image);
    let ai = one_channel(ctx, "A").and_then(|c| a_img.channels().index_of(&c));
    let bi = one_channel(ctx, "B").and_then(|c| b_img.channels().index_of(&c));
    let op = ctx.knobs.text("operation");
    let mask = mask_factors(ctx, inputs, MASK, region);
    let m = ctx.knobs.float("mix") as f32;
    let mut i = 0usize;
    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            let k = mask.as_ref().map_or(1.0, |mk| mk.get(i).copied().unwrap_or(0.0)) * m;
            let a = ai.map_or(0.0, |p| a_img.get_at(p, x, y));
            let b = bi.map_or(0.0, |p| b_img.get_at(p, x, y));
            let orig = out.get_at(oi, x, y);
            out.set_at(oi, x, y, crate::mix(orig, channel_op(op, a, b), k));
            i += 1;
        }
    }
    Ok(out)
}

fn remove_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let mut info = input_info(ctx, inputs, 0);
    let mut listed = ChannelSet::new();
    for knob in ["channels", "channels2", "channels3", "channels4"] {
        listed = listed.union(&ChannelSet::parse(ctx.knobs.text(knob), &info.channels).unwrap_or_default());
    }
    info.channels = if ctx.knobs.text("operation") == "keep" {
        info.channels.intersect(&listed)
    } else {
        info.channels.iter().filter(|c| !listed.contains(c)).cloned().collect()
    };
    Ok(info)
}

fn remove_engine(_: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    framed(inputs, 0, region, &info.channels)
}

#[cfg(test)]
mod tests {
    use crate::testutil::*;
    use nukecraft_geom::IRect;
    use nukecraft_raster::{Channel, ChannelSet, Image};

    fn src() -> Option<Image> {
        Some(solid(IRect::new(0, 0, 4, 4), [0.1, 0.2, 0.3, 0.4]))
    }

    fn get(img: &Image, c: &str) -> f32 {
        img.get(&Channel::parse(c).unwrap(), 1, 1)
    }

    #[test]
    fn shuffle_rearranges_and_creates_layers() {
        let (_, img) =
            run("Shuffle", vec![("red", text("alpha")), ("green", text("black")), ("blue", text("white")), ("alpha", text("red"))], vec![src()], None);
        assert_eq!([0, 1, 2, 3].map(|c| img.get_at(c, 1, 1)), [0.4, 0.0, 1.0, 0.1]);
        let (info, img) = run("Shuffle", vec![("out", text("matte"))], vec![src()], None);
        assert_eq!(info.channels.len(), 8, "rgba plus the new matte layer");
        assert_eq!(get(&img, "matte.blue"), 0.3);
        assert_eq!(get(&img, "rgba.red"), 0.1, "other channels pass through");
        let (info, img) = run("Shuffle", vec![("out", text("depth")), ("red", text("alpha"))], vec![src()], None);
        assert_eq!(info.channels.len(), 5, "the built-in depth layer is depth.Z alone");
        assert_eq!(get(&img, "depth.Z"), 0.4, "a one-channel out layer takes the red source");
        let (_, img) = run("Shuffle", vec![("in", text("depth"))], vec![src()], None);
        assert_eq!(get(&img, "rgba.red"), 0.0, "a layer the input lacks reads black");
    }

    #[test]
    fn copy_moves_channels_from_a() {
        let b = Some(solid(IRect::new(0, 0, 4, 4), [0.5, 0.5, 0.5, 1.0]));
        let a = Some(solid(IRect::new(2, 2, 6, 6), [0.1, 0.2, 0.3, 0.25]));
        let (info, img) = run("Copy", vec![], vec![b.clone(), a.clone()], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 6, 6));
        assert_eq!(img.get_at(3, 3, 3), 0.25, "alpha from A");
        assert_eq!(img.get_at(0, 3, 3), 0.5, "red from B");
        let (info, _) = run("Copy", vec![("bbox", text("B side"))], vec![b.clone(), a.clone()], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 4, 4));
        let (_, img) = run("Copy", vec![("from0", text("rgba.red")), ("to0", text("depth.Z")), ("channels", text("rgb"))], vec![b.clone(), a.clone()], None);
        assert_eq!(get(&img, "depth.Z"), 0.1);
        assert_eq!(img.get_at(1, 3, 3), 0.2, "rgb layer copy");
        assert_eq!(img.get_at(3, 3, 3), 1.0, "alpha no longer copied");
        let mut mask = solid(IRect::new(0, 0, 4, 4), [0.0; 4]);
        mask.set_at(3, 3, 3, 0.5);
        let (_, img) = run("Copy", vec![], vec![b.clone(), a, Some(mask)], None);
        assert_eq!((img.get_at(3, 3, 3), img.get_at(3, 2, 2)), (0.625, 1.0), "masked");
        let (_, img) = run("Copy", vec![], vec![b, None], None);
        assert_eq!(img.get_at(3, 1, 1), 0.0, "copying from nothing gives black");
    }

    #[test]
    fn channel_merge_ops() {
        let b = Some(solid(IRect::new(0, 0, 4, 4), [0.0, 0.0, 0.0, 0.5]));
        let a = Some(solid(IRect::new(0, 0, 4, 4), [0.0, 0.0, 0.0, 0.25]));
        for (op, want) in
            [("union", 0.625), ("plus", 0.75), ("minus", -0.25), ("from", 0.25), ("multiply", 0.125), ("stencil", 0.375), ("out", 0.125), ("max", 0.5)]
        {
            let (_, img) = run("ChannelMerge", vec![("operation", text(op))], vec![b.clone(), a.clone()], None);
            assert_eq!(img.get_at(3, 1, 1), want, "{op}");
        }
        let (info, img) = run("ChannelMerge", vec![("A", text("rgba.alpha")), ("output", text("depth.Z"))], vec![b.clone(), a], None);
        assert!(info.channels.contains(&Channel::parse("depth.Z").unwrap()));
        assert_eq!((get(&img, "depth.Z"), img.get_at(3, 1, 1)), (0.625, 0.5), "written to Z; B's alpha unchanged");
        let (_, img) = run("ChannelMerge", vec![], vec![b, None], None);
        assert_eq!(img.get_at(3, 1, 1), 0.5, "union with nothing");
    }

    #[test]
    fn remove_and_keep() {
        let (info, _) = run("Remove", vec![], vec![src()], None);
        assert_eq!(info.channels, ChannelSet::rgba(), "nothing listed, nothing removed");
        let (info, img) = run("Remove", vec![("channels", text("alpha"))], vec![src()], None);
        assert_eq!(info.channels, ChannelSet::rgb());
        assert_eq!(img.channels(), &ChannelSet::rgb());
        let (info, _) = run("Remove", vec![("operation", text("keep")), ("channels", text("alpha")), ("channels2", text("rgba.red"))], vec![src()], None);
        assert_eq!(info.channels.len(), 2);
        let (info, _) = run("Remove", vec![("channels", text("nosuchlayer"))], vec![src()], None);
        assert_eq!(info.channels, ChannelSet::rgba(), "an unknown layer removes nothing");
    }
}
