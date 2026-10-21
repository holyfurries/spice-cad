use eframe::egui::{self, Color32, FontId, Stroke, TextStyle};

pub const ACCENT: Color32 = Color32::from_rgb(118, 167, 249);
pub const ON_ACCENT: Color32 = Color32::from_rgb(14, 24, 42);
pub const MUTED: Color32 = Color32::from_rgb(152, 160, 173);
pub const TEXT: Color32 = Color32::from_rgb(228, 232, 239);
pub const PANEL: Color32 = Color32::from_rgb(24, 26, 29);
pub const SURFACE: Color32 = Color32::from_rgb(35, 38, 43);
pub const INPUT: Color32 = Color32::from_rgb(19, 21, 25);
pub const BORDER: Color32 = Color32::from_rgb(53, 58, 66);
pub const VIEWPORT: Color32 = Color32::from_rgb(40, 42, 46);
pub const WARNING: Color32 = Color32::from_rgb(220, 191, 126);
pub const ERROR: Color32 = Color32::from_rgb(238, 144, 144);
pub const SELECTION: Color32 = Color32::from_rgb(44, 63, 90);
pub const CONTROL_HEIGHT: f32 = 36.0;

pub fn vim_command(ui: &mut egui::Ui, command: &str) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 3.0, SELECTION);
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, ACCENT),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        "COMMAND",
        FontId::monospace(11.0),
        ACCENT,
    );
    let hint_width = if rect.width() > 420.0 {
        painter.text(
            rect.right_center() - egui::vec2(8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            "Enter run · Esc cancel",
            FontId::proportional(11.0),
            MUTED,
        );
        136.0
    } else {
        0.0
    };
    let content = egui::Rect::from_min_max(
        rect.min + egui::vec2(76.0, 3.0),
        rect.max - egui::vec2(hint_width + 8.0, 3.0),
    );
    let text = painter.layout_no_wrap(format!(":{command}"), FontId::monospace(14.0), TEXT);
    let left = content.left() - (text.size().x + 9.0 - content.width()).max(0.0);
    let position = egui::pos2(left, rect.center().y - text.size().y * 0.5);
    let cursor = egui::Rect::from_min_size(
        egui::pos2(left + text.size().x + 1.0, content.top()),
        egui::vec2(7.0, content.height()),
    );
    let painter = painter.with_clip_rect(content);
    painter.galley(position, text, TEXT);
    painter.rect_filled(cursor, 0.0, ACCENT);
    response
        .on_hover_text("Vim command line · Enter runs the command · Esc returns to normal mode");
}

pub fn configure(context: &egui::Context) {
    let mut style = (*context.style_of(egui::Theme::Dark)).clone();
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(18.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(14.0));
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.interact_size = egui::vec2(40.0, CONTROL_HEIGHT);
    style.spacing.menu_margin = egui::Margin::same(8);
    style.spacing.window_margin = egui::Margin::same(12);
    style.spacing.icon_width = 18.0;
    style.spacing.icon_width_inner = 10.0;
    style.spacing.icon_spacing = 8.0;
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
    visuals.window_corner_radius = 6.into();
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
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 54, 64);
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

pub fn menu_config() -> egui::containers::menu::MenuConfig {
    egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
}

pub fn menu_style(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = CONTROL_HEIGHT;
    style.visuals.button_frame = true;
    style.visuals.widgets.open = style.visuals.widgets.active;
}

pub fn panel_title(ui: &mut egui::Ui, title: &str) {
    ui.add_sized(
        egui::vec2(0.0, CONTROL_HEIGHT),
        egui::Label::new(egui::RichText::new(title).size(12.0).strong().color(MUTED)),
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
    ui.add_space(10.0);
    ui.label(egui::RichText::new(title).size(13.0).strong().color(TEXT));
    ui.add_space(2.0);
}

pub fn grid(
    painter: &egui::Painter,
    rect: egui::Rect,
    matrix: glam::Mat4,
    target: glam::Vec3,
    distance: f32,
    view_index: usize,
) {
    let spacing = 10.0_f32.powf((distance / 12.0).log10().floor());
    let (horizontal, vertical) = match view_index {
        2 => (glam::Vec3::X, glam::Vec3::Z),
        3 => (glam::Vec3::Y, glam::Vec3::Z),
        _ => (glam::Vec3::X, glam::Vec3::Y),
    };
    let center = horizontal * (target.dot(horizontal) / spacing).floor() * spacing
        + vertical * (target.dot(vertical) / spacing).floor() * spacing;
    for index in -10..=10 {
        let offset = index as f32 * spacing;
        for (first, second) in [
            (
                center + horizontal * offset - vertical * (10.0 * spacing),
                center + horizontal * offset + vertical * (10.0 * spacing),
            ),
            (
                center - horizontal * (10.0 * spacing) + vertical * offset,
                center + horizontal * (10.0 * spacing) + vertical * offset,
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
    let vertical = if forward.z.abs() > 0.9999 {
        glam::Vec3::Y
    } else {
        glam::Vec3::Z
    };
    let right = forward.cross(vertical).normalize_or_zero();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Menus {
        file: Option<egui::Rect>,
        view: Option<egui::Rect>,
        toggle: Option<egui::Rect>,
        appearance: Option<egui::Rect>,
        edge: Option<egui::Rect>,
        file_open: bool,
        view_open: bool,
        appearance_open: bool,
    }

    fn render(context: &egui::Context, values: &mut [bool; 2], events: Vec<egui::Event>) -> Menus {
        let mut result = Menus::default();
        context
            .run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 650.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::Panel::top("menus").show(ui, |ui| {
                        egui::MenuBar::new()
                            .style(menu_style)
                            .config(menu_config())
                            .ui(ui, |ui| {
                                let file = ui.menu_button("File", |ui| {
                                    let _ = ui.button("Open");
                                });
                                result.file = Some(file.response.rect);
                                result.file_open = file.inner.is_some();
                                let view = ui.menu_button("View", |ui| {
                                    result.toggle =
                                        Some(ui.checkbox(&mut values[0], "Properties").rect);
                                    let appearance = ui.menu_button("Appearance", |ui| {
                                        result.edge =
                                            Some(ui.checkbox(&mut values[1], "Surface edges").rect);
                                    });
                                    result.appearance = Some(appearance.response.rect);
                                    result.appearance_open = appearance.inner.is_some();
                                });
                                result.view = Some(view.response.rect);
                                result.view_open = view.inner.is_some();
                            });
                    });
                },
            )
            .drop_without_applying_deltas();
        result
    }

    fn click(context: &egui::Context, values: &mut [bool; 2], position: egui::Pos2) {
        for pressed in [true, false] {
            render(
                context,
                values,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn settings_stay_open_after_toggles_and_hovered_submenus_remain_stable() {
        let context = egui::Context::default();
        configure(&context);
        let mut values = [true, true];
        render(&context, &mut values, Vec::new());
        let initial = render(&context, &mut values, Vec::new());
        click(
            &context,
            &mut values,
            initial.view.expect("View menu").center(),
        );
        let opened = render(&context, &mut values, Vec::new());
        assert!(opened.view_open);
        click(
            &context,
            &mut values,
            opened.toggle.expect("Properties toggle").center(),
        );
        let toggled = render(&context, &mut values, Vec::new());
        assert!(!values[0]);
        assert!(toggled.view_open);
        render(
            &context,
            &mut values,
            vec![egui::Event::PointerMoved(
                initial.file.expect("File menu").center(),
            )],
        );
        assert!(render(&context, &mut values, Vec::new()).file_open);
        render(
            &context,
            &mut values,
            vec![egui::Event::PointerMoved(
                initial.view.expect("View menu").center(),
            )],
        );
        let view = render(&context, &mut values, Vec::new());
        assert!(view.view_open);
        render(
            &context,
            &mut values,
            vec![egui::Event::PointerMoved(
                view.appearance.expect("Appearance submenu").center(),
            )],
        );
        for _ in 0..16 {
            let next = render(&context, &mut values, Vec::new());
            assert!(next.view_open);
            assert!(next.appearance_open);
            assert!(!next.file_open);
            assert_eq!(next.view, initial.view);
        }
        let appearance = render(&context, &mut values, Vec::new());
        click(
            &context,
            &mut values,
            appearance.edge.expect("Edge toggle").center(),
        );
        let toggled = render(&context, &mut values, Vec::new());
        assert!(!values[1]);
        assert!(toggled.view_open);
        assert!(toggled.appearance_open);
        click(&context, &mut values, egui::pos2(800.0, 500.0));
        assert!(!render(&context, &mut values, Vec::new()).view_open);
        click(
            &context,
            &mut values,
            initial.view.expect("View menu").center(),
        );
        render(
            &context,
            &mut values,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!render(&context, &mut values, Vec::new()).view_open);
    }
}
