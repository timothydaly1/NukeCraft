//! Colours and style. NukeCraft's own palette: dark neutral panels for judging images, and a
//! colour per node category in the Node Graph.

use egui::{Color32, Stroke, Visuals};

pub const PANEL: Color32 = Color32::from_rgb(38, 38, 40);
pub const PANEL_DARK: Color32 = Color32::from_rgb(28, 28, 30);
pub const GRAPH_BG: Color32 = Color32::from_rgb(46, 47, 50);
pub const GRAPH_GRID: Color32 = Color32::from_rgb(54, 55, 58);
pub const VIEWER_BG: Color32 = Color32::from_rgb(22, 22, 24);
pub const TEXT: Color32 = Color32::from_rgb(214, 214, 218);
pub const TEXT_DIM: Color32 = Color32::from_rgb(150, 150, 156);
pub const ACCENT: Color32 = Color32::from_rgb(242, 168, 46);
pub const SELECTED: Color32 = Color32::from_rgb(255, 214, 92);
pub const PIPE: Color32 = Color32::from_rgb(170, 170, 176);
pub const PIPE_MASK: Color32 = Color32::from_rgb(120, 170, 210);
pub const ERROR: Color32 = Color32::from_rgb(235, 96, 86);
pub const FORMAT_LINE: Color32 = Color32::from_rgb(120, 120, 126);
pub const BBOX_LINE: Color32 = Color32::from_rgb(200, 200, 205);
pub const KEY: Color32 = Color32::from_rgb(92, 186, 255);
pub const ANIMATED: Color32 = Color32::from_rgb(120, 170, 200);

/// The tile colour of a node category.
pub fn category_color(category: &str) -> Color32 {
    match category {
        "Image" => Color32::from_rgb(112, 112, 118),
        "Draw" => Color32::from_rgb(176, 150, 214),
        "Time" => Color32::from_rgb(150, 130, 96),
        "Channel" => Color32::from_rgb(160, 96, 96),
        "Color" => Color32::from_rgb(104, 160, 116),
        "Filter" => Color32::from_rgb(210, 138, 72),
        "Keyer" => Color32::from_rgb(92, 178, 92),
        "Merge" => Color32::from_rgb(86, 126, 196),
        "Transform" => Color32::from_rgb(150, 120, 196),
        _ => Color32::from_rgb(130, 130, 136),
    }
}

/// The order categories appear in the Nodes menu.
pub const CATEGORY_ORDER: &[&str] = &["Image", "Draw", "Time", "Channel", "Color", "Filter", "Keyer", "Merge", "Transform", "Other"];

/// Dark visuals with NukeCraft's colours.
pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = PANEL_DARK;
    v.selection.bg_fill = Color32::from_rgb(150, 100, 30);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.override_text_color = Some(TEXT);
    ctx.set_visuals(v);
}

/// A colour with its alpha scaled (for dimmed or translucent drawing).
pub fn faded(c: Color32, alpha: f32) -> Color32 {
    let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// Text that reads on a tile of colour `fill`.
pub fn text_on(fill: Color32) -> Color32 {
    let l = 0.299 * fill.r() as f32 + 0.587 * fill.g() as f32 + 0.114 * fill.b() as f32;
    if l > 140.0 { Color32::from_rgb(20, 20, 22) } else { Color32::from_rgb(240, 240, 242) }
}
