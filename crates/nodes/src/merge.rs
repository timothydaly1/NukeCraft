//! Merge ▸ Merge (class `Merge2`): composites input A onto input B with one of Nuke's Merge
//! operations, through an optional mask.

use nukecraft_color::MergeOp;
use nukecraft_geom::IRect;
use nukecraft_raster::{Channel, ChannelSet, Image};

use crate::{Ctx, ImageInfo, Input, InputSpec, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, input_image, input_info, mask_factors, request_same};

const OPERATIONS: &[&str] = &[
    "atop",
    "average",
    "color-burn",
    "color-dodge",
    "conjoint-over",
    "copy",
    "difference",
    "disjoint-over",
    "divide",
    "exclusion",
    "from",
    "geometric",
    "hard-light",
    "hypot",
    "in",
    "mask",
    "matte",
    "max",
    "min",
    "minus",
    "multiply",
    "out",
    "over",
    "overlay",
    "plus",
    "screen",
    "stencil",
    "under",
    "xor",
];

const B: usize = 0;
const A: usize = 1;
const MASK: usize = 2;

pub fn specs() -> Vec<NodeSpec> {
    vec![NodeSpec {
        class: "Merge2",
        aliases: &["Merge"],
        label: "Merge",
        category: "Merge",
        shortcut: Some("M"),
        help: "Combines A with B using `operation` (A over B by default).",
        inputs: &[InputSpec { name: "B", mask: false }, InputSpec { name: "A", mask: false }, InputSpec { name: "mask", mask: true }],
        extra_inputs: false,
        knobs: &[
            KnobSpec {
                name: "operation",
                label: "operation",
                kind: KnobKind::Choice(OPERATIONS),
                default: KnobDefault::Text("over"),
                help: "How A and B combine.",
            },
            KnobSpec {
                name: "bbox",
                label: "set bbox to",
                kind: KnobKind::Choice(&["union", "intersection", "A", "B"]),
                default: KnobDefault::Text("union"),
                help: "Which inputs' data windows the output covers.",
            },
            KnobSpec {
                name: "output",
                label: "output",
                kind: KnobKind::Channels,
                default: KnobDefault::Text("rgba"),
                help: "Channels the operation writes; others come from B.",
            },
            KnobSpec {
                name: "maskChannelInput",
                label: "mask",
                kind: KnobKind::Channels,
                default: KnobDefault::Text("rgba.alpha"),
                help: "Channel of the mask input that limits the merge.",
            },
            crate::color_nodes::INVERT_MASK,
            KnobSpec { name: "mix", label: "mix", kind: KnobKind::Float, default: KnobDefault::Number(1.0), help: "Blend between B (0) and the result (1)." },
        ],
        info: merge_info,
        request: request_same,
        engine: merge_engine,
    }]
}

fn merge_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let b = input_info(ctx, inputs, B);
    let a = input_info(ctx, inputs, A);
    let bbox = match ctx.knobs.text("bbox") {
        "intersection" => a.bbox.intersect(b.bbox),
        "A" => a.bbox,
        "B" => b.bbox,
        _ => a.bbox.union(b.bbox),
    };
    Ok(ImageInfo { format: b.format.clone(), bbox, channels: a.channels.union(&b.channels), frames: b.frames })
}

fn merge_engine(ctx: &Ctx, info: &ImageInfo, inputs: &[Option<Input>], region: IRect) -> Result<Image> {
    let op_name = ctx.knobs.text("operation");
    let op = MergeOp::from_name(op_name).ok_or_else(|| ctx.unsupported(format!("merge operation `{op_name}` is not implemented")))?;
    let empty = Image::new(IRect::EMPTY, ChannelSet::rgba())?;
    let a_img = input_image(inputs, A).map_or(&empty, |i| &i.image);
    let b_img = input_image(inputs, B).map_or(&empty, |i| &i.image);
    let mask = mask_factors(ctx, inputs, MASK, region);
    let output = ctx.channels_knob("output", &info.channels);
    let m = ctx.knobs.float("mix") as f32;

    let mut out = Image::new(region, info.channels.clone())?;
    let (aa, ba) = (a_img.channels().index_of(&Channel::alpha()), b_img.channels().index_of(&Channel::alpha()));
    for (oi, c) in info.channels.iter().enumerate() {
        let ai = a_img.channels().index_of(c);
        let bi = b_img.channels().index_of(c);
        let merged = output.contains(c);
        for y in region.y0..region.y1 {
            for x in region.x0..region.x1 {
                let cb = bi.map_or(0.0, |i| b_img.get_at(i, x, y));
                let v = if merged {
                    let ca = ai.map_or(0.0, |i| a_img.get_at(i, x, y));
                    let a = aa.map_or(0.0, |i| a_img.get_at(i, x, y));
                    let b = ba.map_or(0.0, |i| b_img.get_at(i, x, y));
                    let at = (y - region.y0) as usize * region.width() as usize + (x - region.x0) as usize;
                    let k = mask.as_ref().map_or(1.0, |mk| mk.get(at).copied().unwrap_or(0.0)) * m;
                    crate::mix(cb, op.apply(ca, cb, a, b), k)
                } else {
                    cb
                };
                out.set_at(oi, x, y, v);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use crate::testutil::*;
    use nukecraft_geom::IRect;

    #[test]
    fn every_listed_operation_is_implemented() {
        let names: Vec<&str> = nukecraft_color::MergeOp::ALL.iter().map(|m| m.name()).collect();
        assert_eq!(names, super::OPERATIONS);
    }

    #[test]
    fn a_over_b() {
        let b = solid(IRect::new(0, 0, 8, 8), [0.0, 0.0, 1.0, 1.0]);
        let a = solid(IRect::new(2, 2, 4, 4), [0.5, 0.0, 0.0, 0.5]);
        let (info, img) = run("Merge", vec![], vec![Some(b), Some(a)], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 8, 8));
        // Inside A: 0.5 + 0·(1−0.5) red, 0 + 1·0.5 blue, alpha 0.5 + 1·0.5.
        assert_eq!(img.get_at(0, 3, 3), 0.5);
        assert_eq!(img.get_at(2, 3, 3), 0.5);
        assert_eq!(img.get_at(3, 3, 3), 1.0);
        // Outside A's bbox its edge pixels repeat, as in Nuke.
        assert_eq!(img.get_at(0, 7, 7), 0.5);
    }

    #[test]
    fn bbox_modes_and_mix() {
        let b = solid(IRect::new(0, 0, 8, 8), [0.0, 0.0, 0.0, 1.0]);
        let a = solid(IRect::new(4, 4, 12, 12), [1.0, 1.0, 1.0, 1.0]);
        let (info, _) = run("Merge", vec![("bbox", text("intersection"))], vec![Some(b.clone()), Some(a.clone())], None);
        assert_eq!(info.bbox, IRect::new(4, 4, 8, 8));
        let (info, img) = run("Merge", vec![("operation", text("plus")), ("mix", num(0.5))], vec![Some(b), Some(a)], None);
        assert_eq!(info.bbox, IRect::new(0, 0, 12, 12));
        assert_eq!(img.get_at(0, 5, 5), 0.5);
    }

    #[test]
    fn mask_limits_the_merge() {
        let b = solid(IRect::new(0, 0, 4, 4), [0.0, 0.0, 0.0, 1.0]);
        let a = solid(IRect::new(0, 0, 4, 4), [1.0, 1.0, 1.0, 1.0]);
        let mask = solid(IRect::new(0, 0, 4, 4), [0.0, 0.0, 0.0, 0.25]);
        let (_, img) = run("Merge", vec![], vec![Some(b), Some(a), Some(mask)], None);
        assert_eq!(img.get_at(0, 1, 1), 0.25);
    }

    #[test]
    fn missing_inputs_merge_with_black() {
        let a = solid(IRect::new(0, 0, 2, 2), [1.0, 0.0, 0.0, 1.0]);
        let (_, img) = run("Merge", vec![], vec![None, Some(a)], None);
        assert_eq!(img.get_at(0, 0, 0), 1.0);
        let (info, _) = run("Merge", vec![], vec![None, None], None);
        assert!(info.bbox.is_empty());
    }
}
