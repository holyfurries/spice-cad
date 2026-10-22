use crate::{Scene, kernel::Kernel, model, raster};
use eframe::egui::{Color32, ColorImage};
use glam::{Mat4, Vec3};
use std::{io::BufWriter, path::Path};

const WIDTH: usize = 800;
const HEIGHT: usize = 600;
const HEADER: usize = 40;
const PIXEL_VISITS_MAX: usize = 256_000_000;
const VIEWS: [(&str, Vec3, Vec3); 4] = [
    ("top", Vec3::Z, Vec3::Y),
    ("front", Vec3::NEG_Y, Vec3::Z),
    ("side", Vec3::X, Vec3::Z),
    ("isometric", Vec3::new(1.0, -1.0, 1.0), Vec3::Z),
];

#[cfg(test)]
pub fn write(source: &str, folder: &Path) -> Result<(), String> {
    write_project(source, None, folder)
}

pub fn write_project(
    source: &str,
    project: Option<&crate::project::Project>,
    folder: &Path,
) -> Result<(), String> {
    if folder.exists() {
        return Err(format!(
            "{}: choose a new render directory",
            folder.display()
        ));
    }
    let kernel = Kernel::new()?;
    let snapshot = crate::build_script_snapshot_project(
        source.into(),
        1,
        &mut model::Cache::new(&kernel),
        model::ThumbnailMode::Deferred,
        project,
    )?;
    let mut remaining = PIXEL_VISITS_MAX;
    let images = VIEWS
        .iter()
        .map(|(name, direction, up)| render(&snapshot.scene, name, *direction, *up, &mut remaining))
        .collect::<Result<Vec<_>, _>>()?;
    let mut sheet = ColorImage::filled([WIDTH * 2, HEIGHT * 2], crate::interface::VIEWPORT);
    for (index, image) in images.iter().enumerate() {
        let left = index % 2 * WIDTH;
        let top = index / 2 * HEIGHT;
        for y in 0..HEIGHT {
            let target = (top + y) * WIDTH * 2 + left;
            sheet.pixels[target..target + WIDTH]
                .copy_from_slice(&image.pixels[y * WIDTH..(y + 1) * WIDTH]);
        }
    }
    if let Some(parent) = folder
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let result = (|| -> Result<(), String> {
        for ((name, _, _), image) in VIEWS.iter().zip(&images) {
            write_png(&folder.join(format!("{name}.png")), image)?;
        }
        write_png(&folder.join("contact-sheet.png"), &sheet)?;
        std::fs::write(folder.join("model.luau"), source).map_err(|error| error.to_string())?;
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(cleanup) = std::fs::remove_dir_all(folder) {
            return Err(format!(
                "Render export failed: {error}; cleanup failed: {cleanup}"
            ));
        }
        return Err(format!("Render export failed: {error}"));
    }
    Ok(())
}

fn projection(bounds: [Vec3; 2], direction: Vec3, up: Vec3) -> Mat4 {
    assert!(bounds.iter().all(|point| point.is_finite()));
    assert!((bounds[1] - bounds[0]).min_element() >= 0.0);
    let center = (bounds[0] + bounds[1]) * 0.5;
    let radius = (bounds[1] - bounds[0]).length().max(0.001);
    let view = Mat4::look_at_rh(center + direction.normalize() * radius * 2.0, center, up);
    let mut extent = Vec3::ZERO;
    for index in 0..8 {
        let corner = Vec3::new(
            bounds[index & 1].x,
            bounds[(index >> 1) & 1].y,
            bounds[(index >> 2) & 1].z,
        );
        extent = extent.max(view.transform_point3(corner).abs());
    }
    let aspect = WIDTH as f32 / (HEIGHT - HEADER) as f32;
    let half_height = extent.y.max(extent.x / aspect).max(0.001) * 1.12;
    Mat4::orthographic_rh(
        -half_height * aspect,
        half_height * aspect,
        -half_height,
        half_height,
        radius * 0.1,
        radius * 4.0,
    ) * view
}

fn screen(matrix: Mat4, point: [f32; 3]) -> Vec3 {
    let projected = matrix.project_point3(Vec3::from_array(point));
    Vec3::new(
        (projected.x + 1.0) * WIDTH as f32 * 0.5,
        HEADER as f32 + (1.0 - projected.y) * (HEIGHT - HEADER) as f32 * 0.5,
        projected.z,
    )
}

fn render(
    scene: &Scene,
    name: &str,
    direction: Vec3,
    up: Vec3,
    remaining: &mut usize,
) -> Result<ColorImage, String> {
    assert_eq!(scene.display.parts.len(), scene.instances.len());
    assert!(*remaining <= PIXEL_VISITS_MAX);
    let matrix = projection(scene.bounds, direction, up);
    let mut image = ColorImage::filled([WIDTH, HEIGHT], crate::interface::VIEWPORT);
    let mut depth = vec![f32::INFINITY; WIDTH * HEIGHT];
    let light = (direction.normalize() + Vec3::new(-0.3, -0.4, 0.8)).normalize();
    for (part, instance) in scene.display.parts.iter().zip(scene.instances.iter()) {
        let placement = instance.matrix();
        let projection = matrix * placement;
        for triangle in part.geometry.mesh.vertices.as_chunks::<3>().0 {
            let points = triangle.map(|vertex| screen(projection, vertex.position));
            let normal = placement
                .transform_vector3(Vec3::from_array(triangle[0].normal))
                .normalize_or_zero();
            let shade = 0.45 + 0.55 * normal.dot(light).max(0.0);
            let [r, g, b] = instance.color().map(|value| value * shade);
            raster::triangle(
                &mut image,
                &mut depth,
                points,
                eframe::egui::Rgba::from_rgb(r, g, b).into(),
                remaining,
            )
            .ok_or_else(|| {
                format!("{name}: rendering exceeded its pixel budget or has invalid geometry")
            })?;
        }
    }
    for (part, instance) in scene.display.parts.iter().zip(scene.instances.iter()) {
        let projection = matrix * instance.matrix();
        for segment in &part.geometry.edges.segments {
            let start = screen(projection, segment.start);
            let end = screen(projection, segment.end);
            if !start.is_finite() || !end.is_finite() {
                return Err(format!("{name}: invalid edge projection"));
            }
            let count = (end - start).truncate().abs().max_element().ceil() as usize;
            let count = count.clamp(1, WIDTH.max(HEIGHT));
            let visits = (count + 1) * 9;
            if visits > *remaining {
                return Err(format!("{name}: rendering exceeded its pixel budget"));
            }
            *remaining -= visits;
            for step in 0..=count {
                let point = start.lerp(end, step as f32 / count as f32);
                let x = point.x.floor() as i32;
                let y = point.y.floor() as i32;
                if (0..WIDTH as i32).contains(&x) && (HEADER as i32..HEIGHT as i32).contains(&y) {
                    let visible = (-1..=1).any(|offset_y| {
                        (-1..=1).any(|offset_x| {
                            let near_x = (x + offset_x).clamp(0, WIDTH as i32 - 1) as usize;
                            let near_y =
                                (y + offset_y).clamp(HEADER as i32, HEIGHT as i32 - 1) as usize;
                            point.z <= depth[near_y * WIDTH + near_x] + 0.000001
                        })
                    });
                    if visible {
                        image.pixels[y as usize * WIDTH + x as usize] =
                            Color32::from_rgb(20, 24, 30);
                    }
                }
            }
        }
    }
    caption(&mut image, name);
    Ok(image)
}

fn caption(image: &mut ColorImage, name: &str) {
    assert_eq!(image.size, [WIDTH, HEIGHT]);
    assert!(name.len() <= 9);
    for (index, letter) in name.bytes().enumerate() {
        let rows: [u8; 7] = match letter {
            b't' => [31, 4, 4, 4, 4, 4, 4],
            b'o' => [14, 17, 17, 17, 17, 17, 14],
            b'p' => [30, 17, 17, 30, 16, 16, 16],
            b'f' => [31, 16, 16, 30, 16, 16, 16],
            b'r' => [30, 17, 17, 30, 20, 18, 17],
            b'n' => [17, 25, 25, 21, 19, 19, 17],
            b's' => [15, 16, 16, 14, 1, 1, 30],
            b'i' => [14, 4, 4, 4, 4, 4, 14],
            b'd' => [30, 17, 17, 17, 17, 17, 30],
            b'e' => [31, 16, 16, 30, 16, 16, 31],
            b'm' => [17, 27, 21, 21, 17, 17, 17],
            b'c' => [14, 17, 16, 16, 16, 17, 14],
            _ => unreachable!("Unknown view caption"),
        };
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    for y in 0..2 {
                        for x in 0..2 {
                            image.pixels
                                [(12 + row * 2 + y) * WIDTH + 20 + index * 12 + column * 2 + x] =
                                Color32::from_rgb(215, 222, 232);
                        }
                    }
                }
            }
        }
    }
}

fn write_png(path: &Path, image: &ColorImage) -> Result<(), String> {
    assert!(image.size[0] <= WIDTH * 2);
    assert!(image.size[1] <= HEIGHT * 2);
    let file = std::fs::File::create_new(path).map_err(|error| error.to_string())?;
    let mut encoder = png::Encoder::new(
        BufWriter::new(file),
        image.size[0] as u32,
        image.size[1] as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    let mut bytes = Vec::with_capacity(image.pixels.len() * 4);
    for pixel in &image.pixels {
        bytes.extend_from_slice(&pixel.to_array());
    }
    writer
        .write_image_data(&bytes)
        .map_err(|error| error.to_string())?;
    writer.finish().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "local box=cad.box({width=40,depth=20,height=30})\ncad.show('front',box,{material = cad.material('Surface', {base_color = {r=0,g=1,b=0}}),assembly={position={x=100,y=-30,z=50}}})\ncad.show('back',cad.box({width=20,depth=20,height=10,x=10,z=10}),{material = cad.material('Surface', {base_color = {r=1,g=0,b=0}}),assembly={position={x=100,y=30,z=50}}})\n";

    #[test]
    fn frames_all_corners_with_cad_view_orientations() {
        for bounds in [
            [
                Vec3::new(-150.0, 20.0, -400.0),
                Vec3::new(900.0, 22.0, -100.0),
            ],
            [Vec3::splat(-0.001), Vec3::splat(0.001)],
        ] {
            for (_, direction, up) in VIEWS {
                let matrix = projection(bounds, direction, up);
                for index in 0..8 {
                    let corner = [
                        bounds[index & 1].x,
                        bounds[(index >> 1) & 1].y,
                        bounds[(index >> 2) & 1].z,
                    ];
                    let point = screen(matrix, corner);
                    assert!(point.x > 0.0 && point.x < WIDTH as f32);
                    assert!(point.y > HEADER as f32 && point.y < HEIGHT as f32);
                    assert!((0.0..1.0).contains(&point.z));
                }
            }
        }
        let bounds = [Vec3::ZERO, Vec3::splat(40.0)];
        for (name, direction, up) in VIEWS {
            let matrix = projection(bounds, direction, up);
            let origin = screen(matrix, [0.0; 3]);
            let right = if name == "side" { Vec3::Y } else { Vec3::X };
            assert!(screen(matrix, right.to_array()).x > origin.x);
            assert!(screen(matrix, up.to_array()).y < origin.y);
            assert!(screen(matrix, direction.to_array()).z < origin.z);
        }
    }

    #[test]
    fn transformed_parts_occlude_regardless_of_draw_order_and_obey_budget() -> Result<(), String> {
        let snapshot = crate::build_script_snapshot(SOURCE, 1)?;
        let mut remaining = PIXEL_VISITS_MAX;
        let image = render(
            &snapshot.scene,
            "front",
            Vec3::NEG_Y,
            Vec3::Z,
            &mut remaining,
        )?;
        let center = image.pixels[(HEADER + (HEIGHT - HEADER) / 2) * WIDTH + WIDTH / 2];
        assert!(center.g() > 150);
        assert_eq!(center.r(), 0);
        for y in 140..500 {
            for x in 150..650 {
                let pixel = image.pixels[y * WIDTH + x];
                assert_eq!(pixel.r(), 0);
                assert!(pixel.g() > 150);
            }
        }
        assert!(remaining < PIXEL_VISITS_MAX);
        let mut exhausted = 0;
        assert!(
            render(
                &snapshot.scene,
                "front",
                Vec3::NEG_Y,
                Vec3::Z,
                &mut exhausted
            )
            .is_err()
        );
        assert_eq!(exhausted, 0);
        Ok(())
    }

    #[test]
    fn exports_decodable_views_preserves_source_and_rejects_invalid_or_existing_output()
    -> Result<(), String> {
        let folder = std::env::temp_dir().join(format!(
            "spice-views-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        let result = (|| -> Result<(), String> {
            let invalid = "local box=cad.box({width=40,depth=30,height=20})\nlocal a=cad.face(box,{name='bottom',normal='z-'})\nlocal b=cad.face(box,{name='top',normal='z+'})\ncad.measure('height',{from=a,to=b,min=30})\ncad.show('box',box)";
            assert!(write(invalid, &folder).is_err());
            assert!(!folder.exists());
            write(SOURCE, &folder)?;
            assert_eq!(
                std::fs::read_to_string(folder.join("model.luau"))
                    .map_err(|error| error.to_string())?,
                SOURCE
            );
            for name in ["top", "front", "side", "isometric", "contact-sheet"] {
                let file = std::fs::File::open(folder.join(format!("{name}.png")))
                    .map_err(|error| error.to_string())?;
                let mut decoder = png::Decoder::new(std::io::BufReader::new(file))
                    .read_info()
                    .map_err(|error| error.to_string())?;
                let mut bytes = vec![
                    0;
                    decoder
                        .output_buffer_size()
                        .ok_or("Missing PNG buffer size")?
                ];
                let info = decoder
                    .next_frame(&mut bytes)
                    .map_err(|error| error.to_string())?;
                let scale = if name == "contact-sheet" { 2 } else { 1 };
                assert_eq!(
                    [info.width, info.height],
                    [(WIDTH * scale) as u32, (HEIGHT * scale) as u32]
                );
                assert!(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .filter(|pixel| **pixel != crate::interface::VIEWPORT.to_array())
                        .count()
                        > 1000
                );
                assert!(bytes.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
            }
            let original =
                std::fs::read(folder.join("front.png")).map_err(|error| error.to_string())?;
            assert!(write("invalid source", &folder).is_err());
            assert_eq!(
                std::fs::read(folder.join("front.png")).map_err(|error| error.to_string())?,
                original
            );
            assert_eq!(
                std::fs::read_dir(&folder)
                    .map_err(|error| error.to_string())?
                    .count(),
                6
            );
            Ok(())
        })();
        if folder.exists() {
            std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        }
        result
    }
}
