//! The cases. Each is a tiny graph on a 64×64 format, built from Nuke's class and knob names, and
//! the values to measure on it. They follow the open questions in `docs/fidelity/README.md`
//! (F1…F12); F4, F5 and F11 concern the Viewer, Edit ▸ Delete and files on disk, and stay manual.

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
                .nuke_knob("color2", json!([0.1, 0.1, 0.1, 1]))
                .nuke_knob("color3", json!([0.9, 0.9, 0.9, 1]))
                .nuke_knob("linewidth", json!(0))
                .nuke_knob("centerlinewidth", json!(0)),
        ],
        vec![sample("C", grid(4), &["rgba.red"])],
    ));
    let mut c = case(
        "checkerboard.default",
        "F9",
        "CheckerBoard with Nuke's defaults (four colours, lines)",
        vec![NodeDef::new("C", "CheckerBoard2")],
        vec![sample("C", grid(4), &RGBA)],
    );
    c.nukecraft = false;
    out.push(c);
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
