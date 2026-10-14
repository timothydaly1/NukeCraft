//! The cases. Each is a tiny graph on a 64×64 format, built from Nuke's class and knob names, and
//! the values to measure on it. They follow the open questions in `docs/fidelity/README.md`
//! (F1…F20); F4, F5 and F11 concern the Viewer, Edit ▸ Delete and files on disk, and stay manual.

use serde_json::{Value, json};

use crate::{Case, KnobSample, NodeDef, Sample};

const W: u32 = 64;
const H: u32 = 64;
const RGBA: [&str; 4] = ["rgba.red", "rgba.green", "rgba.blue", "rgba.alpha"];

fn case(id: &str, area: &'static str, title: &str, nodes: Vec<NodeDef>, samples: Vec<Sample>) -> Case {
    Case { id: id.into(), area, title: title.into(), width: W, height: H, nodes, samples, knob_samples: Vec::new(), nukecraft: true, tolerance: 1e-4 }
}

fn sample(node: &str, points: Vec<(i32, i32)>, channels: &[&str]) -> Sample {
    Sample { node: node.into(), frame: 1.0, points, channels: channels.iter().map(|c| c.to_string()).collect() }
}

fn constant(name: &str, rgba: [f64; 4]) -> NodeDef {
    NodeDef::new(name, "Constant").knob("color", json!(rgba))
}

/// One white pixel at (31, 31), black everywhere else (a Constant cropped with black outside).
fn impulse() -> Vec<NodeDef> {
    vec![constant("Src", [1.0, 1.0, 1.0, 1.0]), NodeDef::new("Dot", "Crop").input(0, "Src").knob("box", json!([31, 31, 32, 32])).knob("crop", json!(true))]
}

/// A square of points around `(cx, cy)`, `r` pixels each way.
fn square(cx: i32, cy: i32, r: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for y in cy - r..=cy + r {
        for x in cx - r..=cx + r {
            v.push((x, y));
        }
    }
    v
}

/// Every `step`-th pixel of the format.
fn grid(step: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for y in (0..H as i32).step_by(step as usize) {
        for x in (0..W as i32).step_by(step as usize) {
            v.push((x, y));
        }
    }
    v
}

/// A coordinate image: red = x / 64, green = y / 64 (two linear Ramps added).
fn coordinates() -> Vec<NodeDef> {
    vec![
        NodeDef::new("RampX", "Ramp").knob("p0", json!([0, 0])).knob("p1", json!([64, 0])).knob("color", json!([1, 0, 0, 0])).knob("type", json!("linear")),
        NodeDef::new("RampY", "Ramp").knob("p0", json!([0, 0])).knob("p1", json!([0, 64])).knob("color", json!([0, 1, 0, 0])).knob("type", json!("linear")),
        NodeDef::new("Coords", "Merge2").input(0, "RampY").input(1, "RampX").knob("operation", json!("plus")),
    ]
}

pub fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    transform(&mut out);
    blur(&mut out);
    animation(&mut out);
    grade(&mut out);
    color_math(&mut out);
    masks(&mut out);
    channels(&mut out);
    draw_and_mix(&mut out);
    reformat(&mut out);
    time(&mut out);
    filters(&mut out);
    merge(&mut out);
    sources(&mut out);
    crop(&mut out);
    other(&mut out);
    out
}

/// F1, F2: Transform filters and matrix.
fn transform(out: &mut Vec<Case>) {
    for (filter, ours) in [
        ("Impulse", true),
        ("Cubic", true),
        ("Keys", true),
        ("Simon", true),
        ("Rifman", true),
        ("Mitchell", true),
        ("Parzen", true),
        ("Notch", false),
        ("Lanczos4", false),
        ("Lanczos6", false),
        ("Sinc4", false),
    ] {
        let mut nodes = impulse();
        nodes.push(
            NodeDef::new("T", "Transform")
                .input(0, "Dot")
                .knob("translate", json!([0.3, 0.6]))
                .knob("center", json!([32, 32]))
                .knob("filter", json!(filter))
                .knob("black_outside", json!(true)),
        );
        let mut c = case(
            &format!("transform.filter.{}", filter.to_lowercase()),
            "F1",
            &format!("A one-pixel impulse moved by (0.3, 0.6) through the {filter} filter: the kernel's footprint"),
            nodes,
            vec![sample("T", square(31, 31, 3), &["rgba.red"]), sample("Dot", vec![(31, 31), (30, 31), (32, 32)], &["rgba.red"])],
        );
        c.nukecraft = ours;
        out.push(c);
    }
    for (filter, scale) in [("Cubic", 0.25), ("Keys", 0.25), ("Cubic", 0.5)] {
        let mut nodes = impulse();
        nodes.push(
            NodeDef::new("T", "Transform").input(0, "Dot").knob("scale", json!([scale, scale])).knob("center", json!([32, 32])).knob("filter", json!(filter)),
        );
        out.push(case(
            &format!("transform.minify.{}.{}", filter.to_lowercase(), (scale * 100.0) as i32),
            "F1",
            &format!("The impulse scaled by {scale} with {filter}: how the kernel widens when minifying"),
            nodes,
            vec![sample("T", square(32, 32, 2), &["rgba.red"])],
        ));
    }
    for (id, knobs) in [
        ("rotate", json!({ "rotate": 30 })),
        ("scale", json!({ "scale": [1.5, 0.75] })),
        ("skew", json!({ "skewX": 0.3, "skewY": 0.15 })),
        ("combined", json!({ "translate": [5, -3], "rotate": 30, "scale": [1.5, 0.75], "skewX": 0.2 })),
    ] {
        let mut nodes = coordinates();
        let mut t = NodeDef::new("T", "Transform")
            .input(0, "Coords")
            .knob("center", json!([20, 40]))
            .knob("filter", json!("Impulse"))
            .knob("black_outside", json!(false));
        if let Value::Object(m) = knobs {
            for (k, v) in m {
                t = t.knob(&k, v);
            }
        }
        nodes.push(t);
        out.push(case(
            &format!("transform.matrix.{id}"),
            "F2",
            "A coordinate image (red = x/64, green = y/64) through the transform, Impulse filter: where each output pixel reads from",
            nodes,
            vec![sample("T", grid(8), &["rgba.red", "rgba.green"]), sample("Coords", vec![(0, 0), (31, 47), (63, 63)], &["rgba.red", "rgba.green"])],
        ));
    }
}

/// F3: Blur kernels and edges.
fn blur(out: &mut Vec<Case>) {
    let row: Vec<(i32, i32)> = (24..=38).map(|x| (x, 31)).collect();
    let col: Vec<(i32, i32)> = (24..=38).map(|y| (31, y)).collect();
    for (filter, size) in [("box", 6.0), ("triangle", 6.0), ("quadratic", 6.0), ("gaussian", 6.0), ("gaussian", 2.5), ("box", 3.5)] {
        let mut nodes = impulse();
        nodes.push(NodeDef::new("B", "Blur").input(0, "Dot").knob("size", json!([size, size])).knob("filter", json!(filter)).knob("crop", json!(false)));
        out.push(case(
            &format!("blur.{filter}.{}", (size * 10.0) as i32),
            "F3",
            &format!("The impulse blurred by size {size} with the {filter} kernel"),
            nodes,
            vec![sample("B", row.clone(), &["rgba.red"]), sample("B", col.clone(), &["rgba.red"])],
        ));
    }
    out.push(case(
        "blur.edges",
        "F3",
        "A flat grey Constant blurred by 10: the edges (Nuke repeats edge pixels outside the bbox)",
        vec![constant("C", [0.5, 0.5, 0.5, 1.0]), NodeDef::new("B", "Blur").input(0, "C").knob("size", json!(10))],
        vec![sample("B", vec![(0, 0), (63, 63), (0, 32), (32, 0)], &RGBA)],
    ));
}

/// F6: default interpolation of new keys.
fn animation(out: &mut Vec<Case>) {
    let frames: Vec<f64> = (2..=60).map(|i| i as f64 * 0.5).collect();
    let mut c = case(
        "animation.default",
        "F6",
        "translate.x keyed 0 at 1, 100 at 10, 50 at 20 with the default interpolation, read every half frame",
        vec![NodeDef::new("T", "Transform").key("translate", 1.0, json!([0, 0])).key("translate", 10.0, json!([100, 0])).key(
            "translate",
            20.0,
            json!([50, 0]),
        )],
        Vec::new(),
    );
    c.knob_samples.push(KnobSample { node: "T".into(), knob: "translate".into(), components: 1, frames });
    out.push(c);
}

/// F7: Grade.
fn grade(out: &mut Vec<Case>) {
    let src = [-0.25, 0.04, 0.5, 0.7];
    for (id, knobs) in [
        ("gain_offset", json!({ "white": 2, "add": 0.1 })),
        ("gamma", json!({ "gamma": 2.2, "black_clamp": false })),
        ("gamma_clamped", json!({ "gamma": 0.45 })),
        ("points", json!({ "blackpoint": 0.1, "whitepoint": 0.8, "black": 0.05, "white": 1.2, "black_clamp": false })),
        ("reverse", json!({ "white": 1.7, "black": 0.05, "gamma": 1.4, "multiply": 0.9, "reverse": true, "black_clamp": false })),
        ("white_clamp", json!({ "white": 3, "white_clamp": true })),
        ("mix", json!({ "white": 2, "mix": 0.25 })),
    ] {
        let mut g = NodeDef::new("G", "Grade").input(0, "C");
        if let Value::Object(m) = knobs {
            for (k, v) in m {
                g = g.knob(&k, v);
            }
        }
        out.push(case(
            &format!("grade.{id}"),
            "F7",
            "A Constant (-0.25, 0.04, 0.5, 0.7) through a Grade",
            vec![constant("C", src), g],
            vec![sample("G", vec![(5, 5)], &RGBA)],
        ));
    }
    for s in [0.0, 0.5, 2.0] {
        out.push(case(
            &format!("saturation.{}", (s * 100.0) as i32),
            "F7",
            "A Constant (0.8, 0.3, 0.1) through Saturation (luminance weights)",
            vec![constant("C", [0.8, 0.3, 0.1, 1.0]), NodeDef::new("S", "Saturation").input(0, "C").knob("saturation", json!(s))],
            vec![sample("S", vec![(5, 5)], &RGBA)],
        ));
    }
}

/// A node with knobs from a JSON object.
fn with_knobs(mut n: NodeDef, knobs: Value) -> NodeDef {
    if let Value::Object(m) = knobs {
        for (k, v) in m {
            n = n.knob(&k, v);
        }
    }
    n
}

/// A horizontal Ramp from black at x = 0 to `color` at x = 64.
fn hramp(name: &str, color: [f64; 4]) -> NodeDef {
    NodeDef::new(name, "Ramp").knob("p0", json!([0, 0])).knob("p1", json!([64, 0])).knob("color", json!(color)).knob("type", json!("linear"))
}

/// Every fourth pixel along row 5.
fn row() -> Vec<(i32, i32)> {
    (0..W as i32).step_by(4).map(|x| (x, 5)).collect()
}

/// F15: the math nodes; F13: ColorCorrect.
fn color_math(out: &mut Vec<Case>) {
    let src = [-0.25, 0.4, 1.5, 0.7];
    for (id, class, knobs, ours) in [
        ("clamp.default", "Clamp", json!({}), true),
        ("clamp.range", "Clamp", json!({ "minimum": 0.1, "maximum": [0.3, 0.35, 1.2, 0.5] }), true),
        ("clamp.clamp_to", "Clamp", json!({ "MinClampTo_enable": true, "MinClampTo": 0.5, "MaxClampTo_enable": true, "MaxClampTo": 0.25 }), true),
        ("clamp.no_minimum", "Clamp", json!({ "minimum_enable": false }), true),
        ("invert.default", "Invert", json!({}), true),
        ("invert.rgba", "Invert", json!({ "channels": "rgba" }), true),
        ("multiply", "Multiply", json!({ "value": [2, 0.5, 3, 4] }), true),
        ("add", "Add", json!({ "value": [0.1, -0.2, 0.3, 0.4] }), true),
        ("gamma", "Gamma", json!({ "value": [2.2, 0.5, 1.8, 1] }), true),
        ("gamma.zero", "Gamma", json!({ "value": 0 }), true),
        ("exposure.stops", "EXPTool", json!({ "mode": "Stops", "red": 1, "green": -1.5, "blue": 0.25 }), true),
        ("exposure.densities", "EXPTool", json!({ "mode": "Densities", "red": 0.3, "green": -0.2, "blue": 0.1 }), false),
        ("exposure.cineon", "EXPTool", json!({ "mode": "Cineon", "red": 30, "green": -20, "blue": 10 }), false),
    ] {
        let mut c = case(
            id,
            "F15",
            "A Constant (-0.25, 0.4, 1.5, 0.7) through a colour math node",
            vec![constant("C", src), with_knobs(NodeDef::new("N", class).input(0, "C"), knobs)],
            vec![sample("N", vec![(5, 5)], &RGBA)],
        );
        c.nukecraft = ours;
        out.push(c);
    }
    // ColorCorrect over a ramp of colours from black to (1.6, 1.2, 0.6): every range is crossed.
    for (id, knobs) in [
        ("saturation", json!({ "saturation": 0.3 })),
        ("contrast", json!({ "contrast": 1.6 })),
        ("gamma", json!({ "gamma": 1.8 })),
        ("gain_offset", json!({ "gain": [1.2, 0.9, 1.1, 1], "offset": 0.05 })),
        ("shadows", json!({ "shadows.gain": 2, "shadows.offset": 0.02 })),
        ("midtones", json!({ "midtones.gamma": 1.5, "midtones.saturation": 0.5 })),
        ("highlights", json!({ "highlights.gain": 0.5, "highlights.contrast": 1.3 })),
        ("master_and_range", json!({ "gain": 1.5, "midtones.gain": 0.5, "contrast": 1.2, "midtones.contrast": 1.5 })),
    ] {
        out.push(case(
            &format!("colorcorrect.{id}"),
            "F13",
            "A ramp from black to (1.6, 1.2, 0.6) through ColorCorrect",
            vec![hramp("R", [1.6, 1.2, 0.6, 1.0]), with_knobs(NodeDef::new("CC", "ColorCorrect").input(0, "R"), knobs)],
            vec![sample("CC", row(), &RGBA)],
        ));
    }
    for mode in ["Ccir 601", "Average", "Maximum"] {
        out.push(case(
            &format!("saturation.{}", mode.to_lowercase().replace(' ', "_")),
            "F7",
            "A Constant (0.8, 0.3, 0.1) through Saturation 0.25 with another luminance formula",
            vec![constant("C", [0.8, 0.3, 0.1, 1.0]), NodeDef::new("S", "Saturation").input(0, "C").knob("saturation", json!(0.25)).knob("mode", json!(mode))],
            vec![sample("S", vec![(5, 5)], &RGBA)],
        ));
    }
}

/// F14: masks and unpremult on colour nodes and Merge.
fn masks(out: &mut Vec<Case>) {
    let src = constant("C", [0.5, 0.25, 0.1, 0.5]);
    let mask = || hramp("M", [1.0, 1.0, 1.0, 1.0]);
    out.push(case(
        "mask.grade",
        "F14",
        "Grade (gain 2) through an alpha ramp on its mask input",
        vec![src.clone(), mask(), NodeDef::new("G", "Grade").input(0, "C").input(1, "M").knob("white", json!(2)).knob("maskChannelInput", json!("rgba.alpha"))],
        vec![sample("G", row(), &RGBA)],
    ));
    out.push(case(
        "mask.grade_inverted",
        "F14",
        "Grade (gain 2) through an inverted alpha ramp, mix 0.5",
        vec![
            src.clone(),
            mask(),
            NodeDef::new("G", "Grade")
                .input(0, "C")
                .input(1, "M")
                .knob("white", json!(2))
                .knob("maskChannelInput", json!("rgba.alpha"))
                .knob("invert_mask", json!(true))
                .knob("mix", json!(0.5)),
        ],
        vec![sample("G", row(), &RGBA)],
    ));
    out.push(case(
        "mask.merge_inverted",
        "F14",
        "Merge plus through an inverted alpha ramp",
        vec![
            src.clone(),
            constant("A", [0.2, 0.2, 0.2, 0.2]),
            mask(),
            NodeDef::new("Mg", "Merge2")
                .input(0, "C")
                .input(1, "A")
                .input(2, "M")
                .knob("operation", json!("plus"))
                .knob("maskChannelInput", json!("rgba.alpha"))
                .knob("invert_mask", json!(true)),
        ],
        vec![sample("Mg", row(), &RGBA)],
    ));
    for (id, class, knobs) in [
        ("unpremult.grade", "Grade", json!({ "add": 0.1, "white": 1.5, "unpremult": "rgba.alpha" })),
        ("unpremult.grade_rgba", "Grade", json!({ "channels": "rgba", "add": 0.1, "unpremult": "rgba.alpha" })),
        ("unpremult.colorcorrect", "ColorCorrect", json!({ "saturation": 0.5, "gamma": 1.4, "unpremult": "rgba.alpha" })),
    ] {
        out.push(case(
            id,
            "F14",
            "A Constant (0.5, 0.25, 0.1, 0.5) through a colour node with unpremult by alpha",
            vec![src.clone(), with_knobs(NodeDef::new("N", class).input(0, "C"), knobs)],
            vec![sample("N", vec![(5, 5)], &RGBA)],
        ));
    }
    out.push(case(
        "unpremult.zero_alpha",
        "F14",
        "Grade (add 0.1) with unpremult by a zero alpha",
        vec![constant("Z", [0.3, 0.2, 0.1, 0.0]), NodeDef::new("G", "Grade").input(0, "Z").knob("add", json!(0.1)).knob("unpremult", json!("rgba.alpha"))],
        vec![sample("G", vec![(5, 5)], &RGBA)],
    ));
}

/// F16: Shuffle, Copy, ChannelMerge, Remove.
fn channels(out: &mut Vec<Case>) {
    let src = [0.1, 0.2, 0.3, 0.4];
    for (id, knobs) in [
        ("alpha_to_rgb", json!({ "red": "alpha", "green": "alpha", "blue": "alpha", "alpha": "alpha" })),
        ("black_white", json!({ "red": "blue", "green": "black", "blue": "white", "alpha": "red" })),
    ] {
        out.push(case(
            &format!("shuffle.{id}"),
            "F16",
            "A Constant (0.1, 0.2, 0.3, 0.4) through a (classic) Shuffle",
            vec![constant("C", src), with_knobs(NodeDef::new("S", "Shuffle").input(0, "C"), knobs)],
            vec![sample("S", vec![(5, 5)], &RGBA)],
        ));
    }
    out.push(case(
        "shuffle.to_depth",
        "F16",
        "Shuffle alpha into depth.Z",
        vec![constant("C", src), NodeDef::new("S", "Shuffle").input(0, "C").knob("out", json!("depth")).knob("red", json!("alpha"))],
        vec![sample("S", vec![(5, 5)], &["rgba.red", "rgba.alpha", "depth.Z"])],
    ));
    // B covers the format; A is a Constant cropped to (16, 16)-(48, 48) without a black border.
    let ab = || {
        vec![
            constant("B", [0.5, 0.5, 0.5, 1.0]),
            constant("Ac", [0.1, 0.2, 0.3, 0.25]),
            NodeDef::new("A", "Crop").input(0, "Ac").knob("box", json!([16, 16, 48, 48])).knob("crop", json!(false)),
        ]
    };
    for (id, knobs) in [
        ("alpha", json!({ "bbox": "union" })),
        ("b_side", json!({ "bbox": "B side" })),
        ("to_depth", json!({ "from0": "rgba.red", "to0": "depth.Z", "bbox": "union" })),
        ("layer", json!({ "from0": "none", "to0": "none", "channels": "rgb", "bbox": "union" })),
        ("mix", json!({ "mix": 0.5, "bbox": "union" })),
    ] {
        let mut nodes = ab();
        nodes.push(with_knobs(NodeDef::new("Cp", "Copy").input(0, "B").input(1, "A"), knobs));
        out.push(case(
            &format!("copy.{id}"),
            "F16",
            "Copy from A (cropped to 16..48) into B (the whole format)",
            nodes,
            vec![sample("Cp", vec![(5, 5), (30, 30), (60, 60)], &["rgba.red", "rgba.green", "rgba.alpha", "depth.Z"])],
        ));
    }
    for op in ["union", "intersect", "stencil", "absminus", "plus", "minus", "from", "multiply", "max", "min", "in", "out"] {
        out.push(case(
            &format!("channelmerge.{op}"),
            "F16",
            "ChannelMerge of A alpha 0.25 and B alpha 0.6 into alpha",
            vec![
                constant("B", [0.0, 0.0, 0.0, 0.6]),
                constant("A", [0.0, 0.0, 0.0, 0.25]),
                NodeDef::new("CM", "ChannelMerge")
                    .input(0, "B")
                    .input(1, "A")
                    .knob("operation", json!(op))
                    .knob("A", json!("rgba.alpha"))
                    .knob("B", json!("rgba.alpha"))
                    .knob("output", json!("rgba.alpha")),
            ],
            vec![sample("CM", vec![(5, 5)], &["rgba.alpha"])],
        ));
    }
    out.push(case(
        "remove.keep",
        "F16",
        "Remove keeping only alpha",
        vec![constant("C", src), NodeDef::new("R", "Remove").input(0, "C").knob("operation", json!("keep")).knob("channels", json!("alpha"))],
        vec![sample("R", vec![(5, 5)], &RGBA)],
    ));
    out.push(case(
        "remove.rgb",
        "F16",
        "Remove rgb",
        vec![constant("C", src), NodeDef::new("R", "Remove").input(0, "C").knob("channels", json!("rgb"))],
        vec![sample("R", vec![(5, 5)], &RGBA)],
    ));
}

/// Points across a horizontal line, `x0..x1` at row `y`.
fn span(x0: i32, x1: i32, y: i32) -> Vec<(i32, i32)> {
    (x0..x1).map(|x| (x, y)).collect()
}

/// F17: Rectangle, Radial, Keymix, Dissolve.
fn draw_and_mix(out: &mut Vec<Case>) {
    let area = json!([10.3, 12.0, 41.6, 40.5]);
    for (id, knobs) in [
        ("hard", json!({})),
        ("soft", json!({ "softness": 6 })),
        ("opacity", json!({ "opacity": 0.4, "color": [1, 0.5, 0.25, 1] })),
        ("invert", json!({ "invert": true })),
    ] {
        out.push(case(
            &format!("rectangle.{id}"),
            "F17",
            "Rectangle over (10.3, 12)-(41.6, 40.5), no background",
            vec![with_knobs(NodeDef::new("R", "Rectangle").knob("area", area.clone()), knobs)],
            vec![sample("R", [span(0, 20, 20), span(36, 48, 20), (5..20).map(|y| (20, y)).collect(), (36..46).map(|y| (20, y)).collect()].concat(), &RGBA)],
        ));
    }
    for (id, knobs) in
        [("over", json!({ "opacity": 0.5, "color": [1, 0, 0, 1] })), ("replace", json!({ "opacity": 0.5, "color": [1, 0, 0, 1], "replace": true }))]
    {
        out.push(case(
            &format!("rectangle.{id}"),
            "F17",
            "Rectangle on a Constant (0, 0, 0.5, 1) background",
            vec![constant("Bg", [0.0, 0.0, 0.5, 1.0]), with_knobs(NodeDef::new("R", "Rectangle").input(0, "Bg").knob("area", area.clone()), knobs)],
            vec![sample("R", vec![(5, 5), (20, 20), (10, 20)], &RGBA)],
        ));
    }
    for (id, knobs) in [
        ("default", json!({})),
        ("hard", json!({ "softness": 0 })),
        ("half", json!({ "softness": 0.5 })),
        ("plinear", json!({ "plinear": true })),
        ("ellipse", json!({ "area": [4, 20, 60, 44], "softness": 0.3 })),
    ] {
        out.push(case(
            &format!("radial.{id}"),
            "F17",
            "Radial over (8, 8)-(56, 56) unless stated, no background",
            vec![with_knobs(NodeDef::new("R", "Radial").knob("area", json!([8, 8, 56, 56])), knobs)],
            vec![sample("R", [span(0, 64, 32), (0..32).map(|y| (32, y)).collect()].concat(), &["rgba.red", "rgba.alpha"])],
        ));
    }
    let ab = || vec![constant("B", [0.0, 0.0, 1.0, 1.0]), constant("A", [1.0, 0.5, 0.0, 0.5]), hramp("M", [1.0, 1.0, 1.0, 1.0])];
    for (id, knobs, mask) in [
        ("mask", json!({ "maskChannel": "rgba.alpha" }), true),
        ("invert", json!({ "maskChannel": "rgba.alpha", "invertMask": true, "mix": 0.5 }), true),
        ("no_mask", json!({}), false),
    ] {
        let mut k = NodeDef::new("K", "Keymix").input(0, "B").input(1, "A");
        if mask {
            k = k.input(2, "M");
        }
        let mut nodes = ab();
        nodes.push(with_knobs(k, knobs));
        out.push(case(
            &format!("keymix.{id}"),
            "F17",
            "Keymix of A (1, 0.5, 0, 0.5) into B (0, 0, 1, 1) through an alpha ramp",
            nodes,
            vec![sample("K", row(), &RGBA)],
        ));
    }
    for which in [0.25, 0.5, 1.5, 2.0] {
        out.push(case(
            &format!("dissolve.{}", (which * 100.0) as i32),
            "F17",
            "Dissolve between three Constants (red, green, blue)",
            vec![
                constant("C0", [1.0, 0.0, 0.0, 1.0]),
                constant("C1", [0.0, 1.0, 0.0, 1.0]),
                constant("C2", [0.0, 0.0, 1.0, 0.5]),
                NodeDef::new("D", "Dissolve").input(0, "C0").input(1, "C1").input(2, "C2").knob("which", json!(which)),
            ],
            vec![sample("D", vec![(5, 5)], &RGBA)],
        ));
    }
}

/// Every `step`-th pixel of a `w`×`h` area.
fn grid_of(w: i32, h: i32, step: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            v.push((x, y));
        }
    }
    v
}

/// F18: Reformat, Mirror2, Position, CornerPin2D, measured on the coordinate image.
fn reformat(out: &mut Vec<Case>) {
    let with = |id: &str, title: &str, node: NodeDef, points: Vec<(i32, i32)>| {
        let mut nodes = coordinates();
        nodes.push(node.input(0, "Coords"));
        case(id, "F18", title, nodes, vec![sample("N", points, &["rgba.red", "rgba.green"])])
    };
    for resize in ["none", "width", "height", "fit", "fill", "distort"] {
        let node = NodeDef::new("N", "Reformat")
            .knob("type", json!("to box"))
            .knob("box_width", json!(32))
            .knob("box_height", json!(48))
            .knob("box_fixed", json!(true))
            .knob("resize", json!(resize))
            .knob("filter", json!("Impulse"));
        out.push(with(&format!("reformat.box.{resize}"), "The coordinate image (64x64) reformatted to a 32x48 box", node, grid_of(32, 48, 3)));
    }
    let cases = [
        ("reformat.box.cubic", json!({ "type": "to box", "box_width": 32, "box_height": 48, "box_fixed": true, "resize": "width", "filter": "Cubic" })),
        (
            "reformat.box.uncentred",
            json!({ "type": "to box", "box_width": 32, "box_height": 48, "box_fixed": true, "resize": "width", "center": false, "filter": "Impulse" }),
        ),
        ("reformat.box.unfixed", json!({ "type": "to box", "box_width": 40, "resize": "width", "filter": "Impulse" })),
        ("reformat.scale.half", json!({ "type": "scale", "scale": 0.5, "filter": "Impulse" })),
        ("reformat.scale.half_cubic", json!({ "type": "scale", "scale": 0.5 })),
        ("reformat.scale.xy", json!({ "type": "scale", "scale": [1.5, 0.75], "filter": "Impulse" })),
        ("reformat.flip_flop", json!({ "type": "scale", "flip": true, "flop": true, "filter": "Impulse" })),
        ("reformat.turn", json!({ "type": "scale", "scale": [0.5, 1], "turn": true, "filter": "Impulse" })),
    ];
    for (id, knobs) in cases {
        out.push(with(id, "The coordinate image (64x64) through Reformat", with_knobs(NodeDef::new("N", "Reformat"), knobs), grid_of(64, 64, 5)));
    }
    for (id, knobs) in
        [("mirror.flip", json!({ "flip": true })), ("mirror.flop", json!({ "flop": true })), ("mirror.both", json!({ "flip": true, "flop": true }))]
    {
        out.push(with(id, "The coordinate image through Mirror2", with_knobs(NodeDef::new("N", "Mirror2"), knobs), grid_of(64, 64, 7)));
    }
    out.push(with(
        "position.fractional",
        "The coordinate image through Position (7.4, -3.6)",
        NodeDef::new("N", "Position").knob("translate", json!([7.4, -3.6])),
        grid_of(64, 64, 7),
    ));
    let quad =
        json!({ "to1": [6, 4], "to2": [58, 10], "to3": [50, 60], "to4": [2, 52], "from1": [0, 0], "from2": [64, 0], "from3": [64, 64], "from4": [0, 64] });
    for (id, extra) in [
        ("cornerpin.impulse", json!({ "filter": "Impulse" })),
        ("cornerpin.cubic", json!({ "filter": "Cubic" })),
        ("cornerpin.disabled", json!({ "filter": "Impulse", "enable3": false })),
        ("cornerpin.invert", json!({ "filter": "Impulse", "invert": true })),
    ] {
        let node = with_knobs(with_knobs(NodeDef::new("N", "CornerPin2D"), quad.clone()), extra);
        out.push(with(id, "The coordinate image through CornerPin2D onto a quad", node, grid_of(64, 64, 5)));
    }
}

/// F19: FrameHold and TimeOffset on a Constant whose red is the frame number (keyed on frames
/// 1 to 12 and 89 to 100).
fn time(out: &mut Vec<Case>) {
    let frames = || {
        let mut c = NodeDef::new("C", "Constant");
        for f in (1..=12).chain(89..=100) {
            c = c.key("color", f as f64, json!([f, 0, 0, 1]));
        }
        c
    };
    let at = |frames: &[f64]| -> Vec<Sample> {
        frames.iter().map(|f| Sample { node: "N".into(), frame: *f, points: vec![(3, 3)], channels: vec!["rgba.red".into()] }).collect()
    };
    for (id, node, sampled) in [
        ("framehold.hold", NodeDef::new("N", "FrameHold").knob("first_frame", json!(5)), vec![1.0, 5.0, 9.0, 95.0]),
        (
            "framehold.increment",
            NodeDef::new("N", "FrameHold").knob("first_frame", json!(2)).knob("increment", json!(3)),
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 8.0, 10.0],
        ),
        ("timeoffset.offset", NodeDef::new("N", "TimeOffset").knob("time_offset", json!(3)), vec![5.0, 9.0, 12.0]),
        ("timeoffset.negative", NodeDef::new("N", "TimeOffset").knob("time_offset", json!(-4)), vec![1.0, 6.0, 90.0]),
        ("timeoffset.reverse", NodeDef::new("N", "TimeOffset").knob("reverse_input", json!(true)), vec![1.0, 5.0, 95.0, 100.0]),
        ("timeoffset.reverse_offset", NodeDef::new("N", "TimeOffset").knob("reverse_input", json!(true)).knob("time_offset", json!(2)), vec![3.0, 7.0, 97.0]),
    ] {
        let mut c = case(id, "F19", "A Constant whose red is the frame number, through a time node", vec![frames(), node.input(0, "C")], Vec::new());
        c.samples = at(&sampled);
        out.push(c);
    }
}

/// F20: Sharpen, Erode (fast) and (filter), Glow, Median, and Blur through a mask, on a
/// 16x16 square of (1.5, 1, 0.5, 1) at (24, 24) on black.
fn filters(out: &mut Vec<Case>) {
    let square =
        || vec![constant("Sq", [1.5, 1.0, 0.5, 1.0]), NodeDef::new("S", "Crop").input(0, "Sq").knob("box", json!([24, 24, 40, 40])).knob("crop", json!(true))];
    let points = || [span(14, 50, 32), (14..32).map(|y| (32, y)).collect()].concat();
    for (id, class, knobs) in [
        ("sharpen.default", "Sharpen", json!({})),
        ("sharpen.strong", "Sharpen", json!({ "amount": 2, "size": 6 })),
        ("erode_fast.grow", "Dilate", json!({ "size": 2 })),
        ("erode_fast.shrink", "Dilate", json!({ "size": -1 })),
        ("erode_fast.xy", "Dilate", json!({ "size": [3, 1] })),
        ("erode_filter.default", "FilterErode", json!({})),
        ("erode_filter.half", "FilterErode", json!({ "size": 0.5 })),
        ("erode_filter.grow", "FilterErode", json!({ "size": -1.5 })),
        ("glow.default", "Glow", json!({})),
        ("glow.tolerance", "Glow", json!({ "tolerance": 0.75, "brightness": 2 })),
        ("glow.effect_only", "Glow", json!({ "effect_only": true, "saturation": 0.25, "size": 6 })),
        ("median.3", "Median", json!({ "size": 3 })),
        ("median.5", "Median", json!({ "size": 5 })),
    ] {
        let mut nodes = square();
        nodes.push(with_knobs(NodeDef::new("N", class).input(0, "S"), knobs));
        out.push(case(id, "F20", "A 16x16 square of (1.5, 1, 0.5, 1) on black through a filter", nodes, vec![sample("N", points(), &RGBA)]));
    }
    let mut nodes = square();
    nodes.push(hramp("M", [1.0, 1.0, 1.0, 1.0]));
    nodes.push(NodeDef::new("N", "Blur").input(0, "S").input(1, "M").knob("size", json!(8)).knob("maskChannelInput", json!("rgba.alpha")));
    out.push(case("blur.mask", "F20", "Blur 8 through an alpha ramp mask", nodes, vec![sample("N", points(), &RGBA)]));
}

/// F8: every Merge operation, in range and out of range.
fn merge(out: &mut Vec<Case>) {
    let ops = [
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
        "soft-light",
    ];
    for op in ops {
        for (variant, a, b) in [("", [0.6, 0.3, 0.9, 0.5], [0.2, 0.7, 0.4, 0.8]), (".hdr", [1.5, -0.2, 0.5, 1.0], [0.5, 2.0, -0.1, 0.4])] {
            let mut c = case(
                &format!("merge.{op}{variant}"),
                "F8",
                &format!("Constant A {a:?} {op} Constant B {b:?}"),
                vec![constant("B", b), constant("A", a), NodeDef::new("M", "Merge2").input(0, "B").input(1, "A").knob("operation", json!(op))],
                vec![sample("M", vec![(5, 5)], &RGBA)],
            );
            c.nukecraft = op != "soft-light";
            out.push(c);
        }
    }
    out.push(case(
        "merge.mask_mix",
        "F8",
        "over with mix 0.5 through a mask input of alpha 0.25",
        vec![
            constant("B", [0.2, 0.7, 0.4, 0.8]),
            constant("A", [0.6, 0.3, 0.9, 0.5]),
            constant("K", [0.0, 0.0, 0.0, 0.25]),
            NodeDef::new("M", "Merge2").input(0, "B").input(1, "A").input(2, "K").knob("mix", json!(0.5)),
        ],
        vec![sample("M", vec![(5, 5)], &RGBA)],
    ));
    out.push(case(
        "merge.bbox_edges",
        "F8",
        "A small A over a full B: A's edge pixels repeat outside its bbox",
        vec![
            constant("B", [0.1, 0.1, 0.1, 1.0]),
            constant("Asrc", [1.0, 0.0, 0.0, 1.0]),
            NodeDef::new("A", "Crop").input(0, "Asrc").knob("box", json!([20, 20, 40, 40])).knob("crop", json!(false)),
            NodeDef::new("M", "Merge2").input(0, "B").input(1, "A"),
        ],
        vec![sample("M", vec![(5, 5), (30, 30), (60, 30)], &RGBA)],
    ));
}

/// F9, F10: CheckerBoard and Ramp.
fn sources(out: &mut Vec<Case>) {
    let column: Vec<(i32, i32)> = (0..64).step_by(3).map(|y| (10, y)).collect();
    for ty in ["linear", "smooth0", "smooth1", "smooth"] {
        out.push(case(
            &format!("ramp.{ty}"),
            "F10",
            &format!("A vertical Ramp (p0 at y=0, p1 at y=64), type {ty}"),
            vec![NodeDef::new("R", "Ramp").knob("p0", json!([0, 0])).knob("p1", json!([0, 64])).knob("color", json!([1, 1, 1, 1])).knob("type", json!(ty))],
            vec![sample("R", column.clone(), &RGBA)],
        ));
    }
    out.push(case(
        "checkerboard.plain",
        "F9",
        "CheckerBoard with 16-pixel boxes of 0.1 and 0.9, lines off: where the boxes fall",
        vec![
            NodeDef::new("C", "CheckerBoard2")
                .knob("boxsize", json!([16, 16]))
                .knob("color0", json!([0.1, 0.1, 0.1, 1]))
                .knob("color1", json!([0.9, 0.9, 0.9, 1]))
                .knob("color2", json!([0.1, 0.1, 0.1, 1]))
                .knob("color3", json!([0.9, 0.9, 0.9, 1]))
                .knob("linewidth", json!(0))
                .knob("centerlinewidth", json!(0)),
        ],
        vec![sample("C", grid(4), &["rgba.red"])],
    ));
    out.push(case(
        "checkerboard.default",
        "F9",
        "CheckerBoard with Nuke's defaults (four colours, lines)",
        vec![NodeDef::new("C", "CheckerBoard2")],
        vec![sample("C", grid(4), &RGBA)],
    ));
    out.push(case(
        "checkerboard.four",
        "F9",
        "CheckerBoard with four distinct colours and no lines: which box gets which colour",
        vec![
            NodeDef::new("C", "CheckerBoard2")
                .knob("boxsize", json!([16, 16]))
                .knob("color0", json!([0.1, 0, 0, 1]))
                .knob("color1", json!([0.2, 0, 0, 1]))
                .knob("color2", json!([0.3, 0, 0, 1]))
                .knob("color3", json!([0.4, 0, 0, 1]))
                .knob("linewidth", json!(0))
                .knob("centerlinewidth", json!(0)),
        ],
        vec![sample("C", grid(8), &["rgba.red"])],
    ));
    out.push(case(
        "checkerboard.lines",
        "F9",
        "CheckerBoard lines: width 5 between 16-pixel boxes, a 3-pixel centre line",
        vec![
            NodeDef::new("C", "CheckerBoard2")
                .knob("boxsize", json!([16, 16]))
                .knob("linewidth", json!(5))
                .knob("linecolor", json!([0, 1, 0, 1]))
                .knob("centerlinewidth", json!(3))
                .knob("centerlinecolor", json!([0, 0, 1, 1])),
        ],
        vec![sample("C", [span(10, 36, 5), (0..12).map(|y| (5, y + 10)).collect()].concat(), &RGBA)],
    ));
    for (id, knobs) in [("default", json!({})), ("full", json!({ "barintensity": 1 }))] {
        out.push(case(
            &format!("colorbars.{id}"),
            "F9",
            "ColorBars: the middle of each bar and strip",
            vec![with_knobs(NodeDef::new("B", "ColorBars"), knobs)],
            vec![sample(
                "B",
                [
                    (4..64).step_by(9).map(|x| (x, 50)).collect::<Vec<_>>(),
                    (4..64).step_by(9).map(|x| (x, 18)).collect(),
                    (1..64).step_by(3).map(|x| (x, 5)).collect(),
                ]
                .concat(),
                &RGBA,
            )],
        ));
    }
}

/// F12: Crop.
fn crop(out: &mut Vec<Case>) {
    let mut edges = Vec::new();
    for x in 8..=13 {
        edges.push((x, 20));
    }
    for x in 28..=33 {
        edges.push((x, 20));
    }
    for y in 8..=13 {
        edges.push((20, y));
    }
    for y in 38..=43 {
        edges.push((20, y));
    }
    edges.extend([(0, 0), (63, 63)]);
    out.push(case(
        "crop.fractional",
        "F12",
        "A white Constant cropped to (10.5, 10.25, 30.5, 40.75), black outside: edge pixels and the border",
        vec![constant("C", [1.0, 1.0, 1.0, 1.0]), NodeDef::new("K", "Crop").input(0, "C").knob("box", json!([10.5, 10.25, 30.5, 40.75]))],
        vec![sample("K", edges, &["rgba.red"])],
    ));
    out.push(case(
        "crop.reformat",
        "F12",
        "The coordinate image cropped to (8, 16, 40, 48) with reformat: the new origin",
        {
            let mut n = coordinates();
            n.push(NodeDef::new("K", "Crop").input(0, "Coords").knob("box", json!([8, 16, 40, 48])).knob("reformat", json!(true)).knob("crop", json!(false)));
            n
        },
        vec![sample("K", vec![(0, 0), (5, 9), (31, 31)], &["rgba.red", "rgba.green"])],
    ));
}

fn other(out: &mut Vec<Case>) {
    out.push(case(
        "premult",
        "F7",
        "Premult and Unpremult of (0.5, 0.4, 0.2, 0.5); Unpremult of zero alpha",
        vec![
            constant("C", [0.5, 0.4, 0.2, 0.5]),
            NodeDef::new("P", "Premult").input(0, "C"),
            NodeDef::new("U", "Unpremult").input(0, "C"),
            constant("Z", [0.3, 0.2, 0.1, 0.0]),
            NodeDef::new("UZ", "Unpremult").input(0, "Z"),
        ],
        vec![sample("P", vec![(1, 1)], &RGBA), sample("U", vec![(1, 1)], &RGBA), sample("UZ", vec![(1, 1)], &RGBA)],
    ));
    out.push(case(
        "switch.which",
        "F7",
        "Switch with which past the last input",
        vec![
            constant("C0", [0.1, 0.0, 0.0, 1.0]),
            constant("C1", [0.9, 0.0, 0.0, 1.0]),
            NodeDef::new("S", "Switch").input(0, "C0").input(1, "C1").knob("which", json!(5)),
        ],
        vec![sample("S", vec![(1, 1)], &["rgba.red"])],
    ));
}
