use bandwidth_limiter::model::Theme;
use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};
#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub on_accent: Color32,
}
pub fn palette(theme: Theme) -> Palette {
    match theme {
        Theme::Dark => Palette {
            background: Color32::from_rgb(19, 26, 34),
            surface: Color32::from_rgb(27, 36, 46),
            text: Color32::from_rgb(232, 239, 246),
            muted: Color32::from_rgb(163, 180, 197),
            accent: Color32::from_rgb(76, 214, 186),
            warning: Color32::from_rgb(255, 204, 128),
            error: Color32::from_rgb(255, 149, 155),
            on_accent: Color32::from_rgb(19, 26, 34),
        },
        Theme::Light => Palette {
            background: Color32::from_rgb(245, 248, 250),
            surface: Color32::WHITE,
            text: Color32::from_rgb(24, 40, 51),
            muted: Color32::from_rgb(77, 97, 112),
            accent: Color32::from_rgb(0, 112, 92),
            warning: Color32::from_rgb(134, 78, 0),
            error: Color32::from_rgb(172, 35, 49),
            on_accent: Color32::WHITE,
        },
    }
}
pub fn apply(ctx: &egui::Context, theme: Theme) {
    let p = palette(theme);
    let dark = theme == Theme::Dark;
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let mut style = (*ctx.style()).clone();
    style.visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    style.visuals.override_text_color = Some(p.text);
    style.visuals.weak_text_color = Some(p.muted);
    style.visuals.panel_fill = p.background;
    style.visuals.window_fill = p.surface;
    style.visuals.extreme_bg_color = p.background;
    style.visuals.faint_bg_color = p.surface;
    style.visuals.selection.bg_fill = if dark {
        Color32::from_rgb(25, 80, 75)
    } else {
        Color32::from_rgb(202, 234, 225)
    };
    style.visuals.selection.stroke = Stroke::new(1.0, p.accent);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.muted);
    style.visuals.widgets.inactive.bg_fill = p.surface;
    style.visuals.widgets.inactive.weak_bg_fill = p.surface;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(
        1.0,
        if dark {
            Color32::from_rgb(78, 102, 122)
        } else {
            Color32::from_rgb(154, 174, 186)
        },
    );
    style.visuals.widgets.hovered.bg_fill = if dark {
        Color32::from_rgb(40, 58, 70)
    } else {
        Color32::from_rgb(226, 237, 241)
    };
    style.visuals.widgets.hovered.weak_bg_fill = style.visuals.widgets.hovered.bg_fill;
    style.visuals.widgets.active.bg_fill = if dark {
        Color32::from_rgb(35, 89, 82)
    } else {
        Color32::from_rgb(202, 234, 225)
    };
    style.visuals.widgets.active.weak_bg_fill = style.visuals.widgets.active.bg_fill;
    style.visuals.warn_fg_color = p.warning;
    style.visuals.error_fg_color = p.error;
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
    ui.label(text.into().color(ui.visuals().weak_text_color()));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_appearance_survives_an_opposite_system_theme_event() {
        let ctx = egui::Context::default();
        for (theme, system_theme) in [
            (Theme::Light, egui::Theme::Dark),
            (Theme::Dark, egui::Theme::Light),
        ] {
            apply(&ctx, theme);
            let _ = ctx.run(
                egui::RawInput {
                    system_theme: Some(system_theme),
                    ..Default::default()
                },
                |_| {},
            );
            assert_eq!(ctx.style().visuals.dark_mode, theme == Theme::Dark);
            assert_eq!(ctx.style().visuals.panel_fill, palette(theme).background);
            assert_eq!(
                ctx.style().visuals.selection.stroke.color,
                palette(theme).accent
            );
        }
    }
}
