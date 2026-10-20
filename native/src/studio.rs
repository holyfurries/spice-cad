use eframe::egui::{self, Color32, Pos2, Stroke};

const PAPER: Color32 = Color32::from_rgb(243, 240, 231);
const RED: Color32 = Color32::from_rgb(216, 66, 46);
const INK: Color32 = Color32::from_rgb(36, 39, 31);
const FOREST: Color32 = Color32::from_rgb(37, 44, 36);

pub fn loading(context: &egui::Context) {
    let rect = context.content_rect();
    let id = egui::Id::unique("startup");
    let area = egui::Modal::default_area(id).fade_in(false);
    context
        .layer_painter(area.layer())
        .rect_filled(rect, 0.0, PAPER);
    egui::Modal::new(id)
        .area(area)
        .backdrop_color(Color32::TRANSPARENT)
        .frame(egui::Frame::NONE)
        .show(context, |ui| {
            ui.set_width(360.0_f32.min(rect.width()));
            identity(ui, "Preparing your model…");
        });
}

pub fn identity(ui: &mut egui::Ui, status: &str) {
    egui::Frame::NONE
        .fill(PAPER)
        .corner_radius(6.0)
        .inner_margin(28.0)
        .show(ui, |ui| {
            ui.set_min_width((ui.available_width() - 56.0).max(240.0));
            ui.vertical_centered(|ui| {
                product_mark(ui, 88.0);
                ui.add_space(14.0);
                ui.label(
                    egui::RichText::new("Spice CAD")
                        .size(34.0)
                        .strong()
                        .color(INK),
                );
                ui.add_space(8.0);
                ui.label(egui::RichText::new(status).size(13.0).color(FOREST));
                ui.add_space(32.0);
                mark(ui, 40.0);
                ui.label(
                    egui::RichText::new("holy furries")
                        .size(14.0)
                        .strong()
                        .color(INK),
                );
                ui.label(
                    egui::RichText::new("S T U D I O")
                        .monospace()
                        .size(9.0)
                        .color(FOREST),
                );
                ui.add_space(4.0);
            });
        });
}

pub fn product_mark(ui: &mut egui::Ui, size: f32) {
    assert!(size.is_finite());
    assert!((16.0..=200.0).contains(&size));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let point = |x: f32, y: f32| {
        let diagonal = std::f32::consts::FRAC_1_SQRT_2;
        rect.min
            + egui::vec2(50.0 + (x - y) * diagonal, 50.0 + (x + y - 100.0) * diagonal) * size
                / 100.0
    };
    ui.painter().add(egui::Shape::convex_polygon(
        [
            [34.0, 4.0],
            [66.0, 4.0],
            [70.0, 8.0],
            [70.0, 17.0],
            [30.0, 17.0],
            [30.0, 8.0],
        ]
        .map(|[x, y]| point(x, y))
        .to_vec(),
        RED,
        Stroke::NONE,
    ));
    ui.painter().add(egui::Shape::closed_line(
        [
            [36.0, 24.0],
            [36.0, 30.0],
            [30.0, 38.0],
            [30.0, 88.0],
            [36.0, 94.0],
            [64.0, 94.0],
            [70.0, 88.0],
            [70.0, 38.0],
            [64.0, 30.0],
            [64.0, 24.0],
        ]
        .map(|[x, y]| point(x, y))
        .to_vec(),
        Stroke::new(6.0 * size / 100.0, RED),
    ));
}

fn mark(ui: &mut egui::Ui, size: f32) {
    assert!(size.is_finite());
    assert!((24.0..=200.0).contains(&size));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let point = |x: f32, y: f32| rect.min + egui::vec2(x, y) * size / 200.0;
    let outline = [
        [31.0, 56.0],
        [82.0, 86.0],
        [119.0, 86.0],
        [171.0, 53.0],
        [157.0, 125.0],
        [101.0, 177.0],
        [45.0, 126.0],
    ];
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(point(101.0, 110.0), RED);
    for [x, y] in outline {
        mesh.colored_vertex(point(x, y), RED);
    }
    for index in 0..7 {
        mesh.add_triangle(0, index + 1, (index + 1) % 7 + 1);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    let red = Stroke::new(7.0 * size / 200.0, RED);
    let cream = Stroke::new(7.0 * size / 200.0, PAPER);
    ui.painter().add(egui::Shape::closed_line(
        outline.map(|[x, y]| point(x, y)).to_vec(),
        red,
    ));
    let angle = -12.0_f32.to_radians();
    let halo: Vec<Pos2> = (0..48)
        .map(|index| {
            let theta = index as f32 * std::f32::consts::TAU / 48.0;
            let x = 43.0 * theta.cos();
            let y = 12.0 * theta.sin();
            point(
                104.0 + x * angle.cos() - y * angle.sin(),
                27.0 + x * angle.sin() + y * angle.cos(),
            )
        })
        .collect();
    ui.painter().add(egui::Shape::closed_line(halo, red));
    for points in [
        [[54.0, 81.0], [60.0, 108.0], [80.0, 101.0]],
        [[147.0, 78.0], [140.0, 107.0], [122.0, 101.0]],
    ] {
        ui.painter().add(egui::Shape::line(
            points.map(|[x, y]| point(x, y)).to_vec(),
            cream,
        ));
    }
    for [[a, b], [c, d]] in [
        [[64.0, 127.0], [86.0, 135.0]],
        [[135.0, 126.0], [114.0, 135.0]],
    ] {
        ui.painter().line_segment([point(a, b), point(c, d)], cream);
    }
    ui.painter().add(egui::Shape::convex_polygon(
        vec![point(92.0, 151.0), point(101.0, 159.0), point(110.0, 151.0)],
        PAPER,
        cream,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splash_covers_the_application_after_resizing() {
        let context = egui::Context::default();
        for size in [egui::vec2(1400.0, 900.0), egui::vec2(1100.0, 650.0)] {
            let rect = egui::Rect::from_min_size(Pos2::ZERO, size);
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| loading(ui.ctx()),
            );
            let covers_window = output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Rect(background)
                    if background.rect == rect && background.fill == PAPER)
            });
            output.drop_without_applying_deltas();
            assert!(covers_window);
            assert!(context.memory(|memory| memory.top_modal_layer().is_some()));
        }
    }
}
