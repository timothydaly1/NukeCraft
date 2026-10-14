//! Time: FrameHold, TimeOffset, FrameRange.
//!
//! FrameHold and TimeOffset read their input at another time than their own (the renderer asks
//! [`NodeSpec::input_time`]); FrameRange only changes the frame range the node reports.

use nukecraft_time::{FrameRange, FrameTime, MAX_FRAME};

use crate::{Ctx, ImageInfo, InputSpec, KnobDefault, KnobKind, KnobSpec, NodeSpec, Result, engine_passthrough, input_info, request_same};

const SOURCE: &[InputSpec] = &[InputSpec { name: "", mask: false }];

const fn knob(name: &'static str, label: &'static str, kind: KnobKind, default: KnobDefault, help: &'static str) -> KnobSpec {
    KnobSpec { name, label, kind, default, help }
}

const FRAME_HOLD_KNOBS: &[KnobSpec] = &[
    knob("first_frame", "first frame", KnobKind::Int, KnobDefault::CurrentFrame, "The frame held (the current frame when the node is made)."),
    knob("increment", "increment", KnobKind::Int, KnobDefault::Number(0.0), "Above 0: hold a new frame every this many frames."),
];

const TIME_OFFSET_KNOBS: &[KnobSpec] = &[
    knob("time_offset", "time offset (frames)", KnobKind::Float, KnobDefault::Number(0.0), "Frames to delay the input by (negative: earlier)."),
    knob("reverse_input", "reverse input", KnobKind::Bool, KnobDefault::Bool(false), "Play the input backwards over its frame range."),
];

const FRAME_RANGE_KNOBS: &[KnobSpec] = &[
    knob("first_frame", "frame range", KnobKind::Int, KnobDefault::Number(1.0), "First frame."),
    knob("last_frame", "to", KnobKind::Int, KnobDefault::Number(100.0), "Last frame."),
];

pub fn specs() -> Vec<NodeSpec> {
    vec![
        NodeSpec {
            class: "FrameHold",
            aliases: &[],
            label: "FrameHold",
            category: "Time",
            shortcut: None,
            help: "Shows one frame of the input at every frame (or every `increment` frames, a new one).",
            inputs: SOURCE,
            extra_inputs: false,
            knobs: FRAME_HOLD_KNOBS,
            info: info_at_input_time,
            request: request_same,
            engine: engine_passthrough,
            input_time: Some(hold_time),
        },
        NodeSpec {
            class: "TimeOffset",
            aliases: &[],
            label: "TimeOffset",
            category: "Time",
            shortcut: None,
            help: "Slides the input in time: frame f shows input frame f - time_offset.",
            inputs: SOURCE,
            extra_inputs: false,
            knobs: TIME_OFFSET_KNOBS,
            info: time_offset_info,
            request: request_same,
            engine: engine_passthrough,
            input_time: Some(offset_time),
        },
        NodeSpec {
            class: "FrameRange",
            aliases: &[],
            label: "FrameRange",
            category: "Time",
            shortcut: None,
            help: "Sets the frame range the image reports (the pixels pass through).",
            inputs: SOURCE,
            extra_inputs: false,
            knobs: FRAME_RANGE_KNOBS,
            info: frame_range_info,
            request: request_same,
            engine: engine_passthrough,
            input_time: None,
        },
    ]
}

/// A knob as a finite frame number within the frame limits.
fn frame_knob(ctx: &Ctx, name: &str) -> f64 {
    let v = ctx.knobs.float(name);
    if v.is_finite() { v.clamp(-(MAX_FRAME as f64), MAX_FRAME as f64) } else { 0.0 }
}

fn hold_time(ctx: &Ctx, _: usize, _: FrameRange) -> FrameTime {
    let first = frame_knob(ctx, "first_frame").round();
    let inc = frame_knob(ctx, "increment").round();
    if inc < 1.0 {
        return FrameTime::from_f64(first);
    }
    // Every `inc` frames from `first`, a new frame (also before `first`).
    let steps = ((ctx.time.as_f64() - first) / inc).floor();
    FrameTime::from_f64(first + steps * inc)
}

fn offset_time(ctx: &Ctx, _: usize, frames: FrameRange) -> FrameTime {
    let t = ctx.time - FrameTime::from_f64(frame_knob(ctx, "time_offset"));
    if ctx.knobs.boolean("reverse_input") { FrameTime::from_frame(frames.first.saturating_add(frames.last)) - t } else { t }
}

/// Info functions see the input's info at the time the input is read, so a pass-through works.
fn info_at_input_time(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    Ok(input_info(ctx, inputs, 0))
}

fn time_offset_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let mut info = input_info(ctx, inputs, 0);
    let off = frame_knob(ctx, "time_offset").round() as i64;
    info.frames = FrameRange::new(info.frames.first.saturating_add(off), info.frames.last.saturating_add(off));
    Ok(info)
}

fn frame_range_info(ctx: &Ctx, inputs: &[Option<ImageInfo>]) -> Result<ImageInfo> {
    let mut info = input_info(ctx, inputs, 0);
    info.frames = FrameRange::new(frame_knob(ctx, "first_frame").round() as i64, frame_knob(ctx, "last_frame").round() as i64);
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use crate::{Knobs, Registry};
    use nukecraft_graph::Value;

    fn at(class: &str, knobs: Vec<(&'static str, Value)>, frame: f64, frames: FrameRange) -> f64 {
        let reg = Registry::builtin();
        let spec = reg.get(class).unwrap();
        let root = root();
        let mut stored = std::collections::BTreeMap::new();
        for (k, v) in knobs {
            let kv = match v {
                Value::Bool(b) => nukecraft_graph::KnobValue::Bool(b),
                Value::Numbers(n) => nukecraft_graph::KnobValue::numbers(&n),
                Value::Text(t) => nukecraft_graph::KnobValue::text(t),
            };
            stored.insert(k.to_string(), kv);
        }
        let t = FrameTime::from_f64(frame);
        let knobs = Knobs::evaluate(spec.knobs, &stored, t, &root.format);
        let ctx = Ctx { knobs: &knobs, root: &root, time: t, node_name: "T1" };
        (spec.input_time.unwrap())(&ctx, 0, frames).as_f64()
    }

    #[test]
    fn frame_hold_holds_and_steps() {
        let r = FrameRange::new(1, 100);
        assert_eq!(at("FrameHold", vec![("first_frame", num(12.0))], 40.0, r), 12.0);
        assert_eq!(at("FrameHold", vec![("first_frame", num(12.0))], 3.5, r), 12.0);
        let step = || vec![("first_frame", num(10.0)), ("increment", num(4.0))];
        assert_eq!(at("FrameHold", step(), 10.0, r), 10.0);
        assert_eq!(at("FrameHold", step(), 13.0, r), 10.0);
        assert_eq!(at("FrameHold", step(), 14.0, r), 14.0);
        assert_eq!(at("FrameHold", step(), 7.0, r), 6.0, "before first_frame too");
        assert_eq!(at("FrameHold", vec![("first_frame", num(f64::NAN)), ("increment", num(1e300))], 5.0, r), 0.0);
    }

    #[test]
    fn time_offset_slides_and_reverses() {
        let r = FrameRange::new(1, 20);
        assert_eq!(at("TimeOffset", vec![("time_offset", num(5.0))], 12.0, r), 7.0);
        assert_eq!(at("TimeOffset", vec![("time_offset", num(-2.5))], 12.0, r), 14.5);
        assert_eq!(at("TimeOffset", vec![("reverse_input", Value::Bool(true))], 1.0, r), 20.0);
        assert_eq!(at("TimeOffset", vec![("reverse_input", Value::Bool(true)), ("time_offset", num(3.0))], 5.0, r), 19.0);
        let (info, _) = run("TimeOffset", vec![("time_offset", num(5.0))], vec![Some(solid(nukecraft_geom::IRect::new(0, 0, 2, 2), [1.0; 4]))], None);
        assert_eq!(info.frames, FrameRange::new(6, 105), "the range moves with the image");
        let (info, _) = run("FrameRange", vec![("first_frame", num(30.0)), ("last_frame", num(10.0))], vec![None], None);
        assert_eq!(info.frames, FrameRange::new(10, 30));
    }
}
