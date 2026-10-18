use eframe::egui::{self, Color32, FontId, Stroke, TextStyle};

pub const ACCENT: Color32 = Color32::from_rgb(121, 207, 187);
pub const ON_ACCENT: Color32 = Color32::from_rgb(15, 35, 30);
pub const MUTED: Color32 = Color32::from_rgb(139, 148, 158);
pub const TEXT: Color32 = Color32::from_rgb(218, 224, 231);
pub const PANEL: Color32 = Color32::from_rgb(25, 28, 32);
pub const SURFACE: Color32 = Color32::from_rgb(32, 36, 41);
pub const INPUT: Color32 = Color32::from_rgb(20, 23, 27);
pub const BORDER: Color32 = Color32::from_rgb(47, 53, 60);
pub const VIEWPORT: Color32 = Color32::from_rgb(43, 47, 53);
pub const WARNING: Color32 = Color32::from_rgb(220, 191, 126);
pub const ERROR: Color32 = Color32::from_rgb(238, 144, 144);
pub const SELECTION: Color32 = Color32::from_rgb(47, 76, 82);
pub const CONTROL_HEIGHT: f32 = 28.0;

pub fn configure(context: &egui::Context) {
    let mut style = (*context.style_of(egui::Theme::Dark)).clone();
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(13.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(13.0));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(17.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(11.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(14.0));
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = CONTROL_HEIGHT;
    style.animation_time = 0.0;
    let visuals = &mut style.visuals;
    *visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = Color32::from_rgb(29, 33, 38);
    visuals.extreme_bg_color = INPUT;
    visuals.faint_bg_color = SURFACE;
    visuals.override_text_color = Some(TEXT);
    visuals.warn_fg_color = WARNING;
    visuals.error_fg_color = ERROR;
    visuals.selection.bg_fill = SELECTION;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.window_corner_radius = 4.into();
    visuals.window_highlight_topmost = false;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.text_cursor.stroke = Stroke::new(1.5, ACCENT);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        widget.corner_radius = 4.into();
        widget.bg_stroke = Stroke::new(1.0, BORDER);
        widget.fg_stroke = Stroke::new(1.0, TEXT);
    }
    visuals.widgets.inactive.bg_fill = SURFACE;
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 52, 59);
    visuals.widgets.hovered.weak_bg_fill = visuals.widgets.hovered.bg_fill;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, MUTED);
    visuals.widgets.active.bg_fill = SELECTION;
    visuals.widgets.active.weak_bg_fill = SELECTION;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.open = visuals.widgets.inactive;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    context.set_theme(egui::Theme::Dark);
    context.set_style_of(egui::Theme::Dark, style);
}

pub fn panel_title(ui: &mut egui::Ui, title: &str) {
    ui.add_sized(
        egui::vec2(0.0, CONTROL_HEIGHT),
        egui::Label::new(egui::RichText::new(title).size(11.0).strong().color(MUTED)),
    );
}

pub fn display_settings(ui: &mut egui::Ui, edges: &mut bool, width_px: &mut f32) {
    ui.label("Viewport appearance");
    ui.separator();
    ui.checkbox(edges, "Surface edges");
    ui.horizontal(|ui| {
        ui.label("Edge width");
        ui.add_enabled(
            *edges,
            egui::DragValue::new(width_px)
                .range(0.75..=3.0)
                .speed(0.1)
                .suffix(" px"),
        );
    });
}

pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(egui::RichText::new(title).size(11.0).strong().color(MUTED));
    ui.add_space(2.0);
}

pub fn grid(
    painter: &egui::Painter,
    rect: egui::Rect,
    matrix: glam::Mat4,
    target: glam::Vec3,
    distance: f32,
) {
    let spacing = 10.0_f32.powf((distance / 12.0).log10().floor());
    let center = (target / spacing).floor() * spacing;
    for index in -10..=10 {
        let offset = index as f32 * spacing;
        for (first, second) in [
            (
                glam::vec3(center.x + offset, center.y - 10.0 * spacing, 0.0),
                glam::vec3(center.x + offset, center.y + 10.0 * spacing, 0.0),
            ),
            (
                glam::vec3(center.x - 10.0 * spacing, center.y + offset, 0.0),
                glam::vec3(center.x + 10.0 * spacing, center.y + offset, 0.0),
            ),
        ] {
            let first = matrix * first.extend(1.0);
            let second = matrix * second.extend(1.0);
            if first.w > 0.1 && second.w > 0.1 {
                let project = |point: glam::Vec4| {
                    rect.center()
                        + egui::vec2(
                            point.x / point.w * rect.width() * 0.5,
                            -point.y / point.w * rect.height() * 0.5,
                        )
                };
                painter.line_segment(
                    [project(first), project(second)],
                    Stroke::new(1.0, Color32::from_white_alpha(12)),
                );
            }
        }
    }
}

pub fn axes(painter: &egui::Painter, rect: egui::Rect, eye_direction: glam::Vec3) {
    let forward = -eye_direction.normalize();
    let right = forward.cross(glam::Vec3::Z).normalize_or_zero();
    let up = right.cross(forward);
    let origin = rect.right_bottom() - egui::vec2(58.0, 64.0);
    for (axis, label, color) in [
        (glam::Vec3::X, "X", Color32::from_rgb(220, 133, 133)),
        (glam::Vec3::Y, "Y", Color32::from_rgb(152, 195, 147)),
        (glam::Vec3::Z, "Z", Color32::from_rgb(136, 173, 223)),
    ] {
        let endpoint = origin + egui::vec2(axis.dot(right), -axis.dot(up)) * 34.0;
        painter.line_segment([origin, endpoint], Stroke::new(1.5, color));
        painter.circle_filled(endpoint, 9.0, Color32::from_rgb(31, 35, 40));
        painter.text(
            endpoint,
            egui::Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.0),
            color,
        );
    }
}

pub fn logo(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 26.0), egui::Sense::hover());
    let point = |x: f32, y: f32| rect.min + egui::vec2(x, y);
    for (first, second) in [
        ((11.0, 2.0), (21.0, 7.0)),
        ((21.0, 7.0), (21.0, 19.0)),
        ((21.0, 19.0), (11.0, 24.0)),
        ((11.0, 24.0), (1.0, 19.0)),
        ((1.0, 19.0), (1.0, 7.0)),
        ((1.0, 7.0), (11.0, 2.0)),
        ((1.0, 7.0), (11.0, 12.0)),
        ((21.0, 7.0), (11.0, 12.0)),
        ((11.0, 12.0), (11.0, 24.0)),
    ] {
        ui.painter().line_segment(
            [point(first.0, first.1), point(second.0, second.1)],
            Stroke::new(1.2, ACCENT),
        );
    }
}
