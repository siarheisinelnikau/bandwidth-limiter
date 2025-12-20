use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};
pub const BACKGROUND: Color32 = Color32::from_rgb(19, 26, 34);
pub const SURFACE: Color32 = Color32::from_rgb(27, 36, 46);
pub const TEXT: Color32 = Color32::from_rgb(232, 239, 246);
pub const MUTED: Color32 = Color32::from_rgb(163, 180, 197);
pub const ACCENT: Color32 = Color32::from_rgb(76, 214, 186);
pub const WARNING: Color32 = Color32::from_rgb(255, 204, 128);
pub const ERROR: Color32 = Color32::from_rgb(255, 149, 155);
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = BACKGROUND;
    style.visuals.faint_bg_color = SURFACE;
    style.visuals.selection.bg_fill = Color32::from_rgb(25, 80, 75);
    style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(78, 102, 122));
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(40, 58, 70);
    style.visuals.widgets.hovered.weak_bg_fill = style.visuals.widgets.hovered.bg_fill;
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(35, 89, 82);
    style.visuals.widgets.active.weak_bg_fill = style.visuals.widgets.active.bg_fill;
    style.visuals.warn_fg_color = WARNING;
    style.visuals.error_fg_color = ERROR;
    style.spacing.item_spacing = egui::vec2(12.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 30.0;
    style.animation_time = 0.0;
    for (kind, size) in [
        (TextStyle::Heading, 25.0),
        (TextStyle::Body, 14.0),
        (TextStyle::Button, 14.0),
        (TextStyle::Small, 12.0),
        (TextStyle::Monospace, 13.0),
    ] {
        let family = if kind == TextStyle::Monospace {
            FontFamily::Monospace
        } else {
            FontFamily::Proportional
        };
        style.text_styles.insert(kind, FontId::new(size, family));
    }
    ctx.set_style(style);
}
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(17.0).strong());
}
pub fn note(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) {
    ui.label(text.into().color(MUTED));
}
