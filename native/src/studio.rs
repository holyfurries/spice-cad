use eframe::egui::{self, Color32, Pos2, Stroke};

const PAPER: Color32 = Color32::from_rgb(243, 240, 231);
const RED: Color32 = Color32::from_rgb(216, 66, 46);
const INK: Color32 = Color32::from_rgb(36, 39, 31);
const FOREST: Color32 = Color32::from_rgb(37, 44, 36);

pub struct Splash {
    artwork: Artwork,
    reduced_motion: bool,
}

enum Artwork {
    Unstarted,
    Static(egui::TextureHandle),
    Animated(crate::splash_frames::Player),
}

impl Default for Splash {
    fn default() -> Self {
        Self {
            artwork: Artwork::Unstarted,
            reduced_motion: std::env::var("HOLYFURRIES_REDUCED_MOTION")
                .is_ok_and(|value| value == "1"),
        }
    }
}

impl Splash {
    pub fn show(&mut self, context: &egui::Context) -> Result<bool, String> {
        let skip = context.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                || input.consume_key(egui::Modifiers::NONE, egui::Key::Space)
                || input.pointer.any_click()
        });
        if skip {
            return Ok(false);
        }
        if matches!(self.artwork, Artwork::Unstarted) {
            self.artwork = if self.reduced_motion {
                Artwork::Static(context.load_texture(
                    "holy-furries-splash",
                    splash_image()?,
                    egui::TextureOptions::LINEAR,
                ))
            } else {
                Artwork::Animated(crate::splash_frames::Player::new(context)?)
            };
        }
        let (background, image) = match &mut self.artwork {
            Artwork::Unstarted => unreachable!(),
            Artwork::Static(texture) => (
                FOREST,
                Some((
                    texture.id(),
                    egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                )),
            ),
            Artwork::Animated(player) => (
                player.background(),
                player.frame(context, std::time::Instant::now())?,
            ),
        };
        let rect = context.content_rect();
        let id = egui::Id::unique("startup");
        let area = egui::Modal::default_area(id).fade_in(false);
        let painter = context.layer_painter(area.layer());
        painter.rect_filled(rect, 0.0, background);
        let scale = (rect.width() / 480.0).min(rect.height() / 400.0).min(1.0);
        let artwork = egui::Rect::from_center_size(rect.center(), egui::vec2(480.0, 400.0) * scale);
        if let Some((texture, uv)) = image {
            painter.image(texture, artwork, uv, Color32::WHITE);
        }
        egui::Modal::new(id)
            .area(area)
            .backdrop_color(Color32::TRANSPARENT)
            .frame(egui::Frame::NONE)
            .show(context, |_| {});
        Ok(true)
    }
}

fn splash_image() -> Result<egui::ColorImage, String> {
    const SIZE: [usize; 2] = [1536, 1280];
    const SIZE_BYTES: usize = SIZE[0] * SIZE[1] * 4;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../assets/brand/poster.png"
    )));
    decoder.set_limits(png::Limits { bytes: SIZE_BYTES });
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    if [reader.info().width as usize, reader.info().height as usize] != SIZE {
        return Err("Shared splash has an unexpected canvas size".into());
    }
    if reader.output_buffer_size() != Some(SIZE_BYTES) {
        return Err("Shared splash has an unexpected pixel format".into());
    }
    let mut bytes = vec![0; SIZE_BYTES];
    let frame = reader
        .next_frame(&mut bytes)
        .map_err(|error| error.to_string())?;
    if frame.color_type != png::ColorType::Rgba || frame.bit_depth != png::BitDepth::Eight {
        return Err("Shared splash requires eight-bit RGBA pixels".into());
    }
    Ok(egui::ColorImage::from_rgba_unmultiplied(SIZE, &bytes))
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
        let mut splash = Splash {
            reduced_motion: true,
            ..Splash::default()
        };
        let mut texture_id = None;
        for size in [
            egui::vec2(1400.0, 900.0),
            egui::vec2(1100.0, 650.0),
            egui::vec2(320.0, 200.0),
        ] {
            let rect = egui::Rect::from_min_size(Pos2::ZERO, size);
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| assert!(splash.show(ui.ctx()).unwrap()),
            );
            let covers_window = output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Rect(background)
                    if background.rect == rect && background.fill == FOREST)
            });
            let Artwork::Static(texture) = &splash.artwork else {
                panic!("Reduced motion must use the static poster")
            };
            if let Some(texture_id) = texture_id {
                assert_eq!(texture.id(), texture_id);
                assert!(output.textures_delta.set.is_empty());
            } else {
                assert!(
                    output
                        .textures_delta
                        .set
                        .iter()
                        .any(|(id, _)| *id == texture.id())
                );
                texture_id = Some(texture.id());
            }
            let artwork = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture.id() => {
                        Some(mesh.calc_bounds())
                    }
                    _ => None,
                })
                .unwrap();
            assert!(rect.contains_rect(artwork));
            assert_eq!(artwork.center(), rect.center());
            assert!((artwork.width() / artwork.height() - 1.2).abs() < 0.0001);
            output.drop_without_applying_deltas();
            assert!(covers_window);
            assert!(context.memory(|memory| memory.top_modal_layer().is_some()));
        }
    }

    #[test]
    fn shared_poster_matches_manifest_and_retains_transparency() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../assets/brand/manifest.json")).unwrap();
        let image = splash_image().unwrap();
        assert_eq!(image.size, [1536, 1280]);
        assert_eq!(manifest["fallback_size"], serde_json::json!(image.size));
        assert_eq!(manifest["view_box"], serde_json::json!([0, 0, 480, 400]));
        assert_eq!(manifest["background"], "oklch(28.3468% 0.01742 141.66)");
        assert_eq!(image.pixels[0], Color32::TRANSPARENT);
        assert!(image.pixels.iter().any(|pixel| pixel.a() == 255));
    }

    #[test]
    fn splash_can_be_skipped_without_decoding_the_poster() {
        for key in [egui::Key::Escape, egui::Key::Space] {
            let context = egui::Context::default();
            let mut splash = Splash::default();
            let output = context.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ui| assert!(!splash.show(ui.ctx()).unwrap()),
            );
            output.drop_without_applying_deltas();
            assert!(matches!(splash.artwork, Artwork::Unstarted));
            assert!(context.memory(|memory| memory.top_modal_layer().is_none()));
        }
    }
}
