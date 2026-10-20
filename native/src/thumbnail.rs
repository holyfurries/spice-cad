use crate::mesh::Mesh;
use eframe::egui;
use eframe::egui::Color32;
use glam::Vec3;

const WIDTH: usize = 160;
const HEIGHT: usize = 100;
pub const PIXEL_VISITS_MAX: usize = 8_000_000;

pub fn render(mesh: &Mesh, bounds: [Vec3; 2], remaining: &mut usize) -> Option<egui::ColorImage> {
    assert!(*remaining <= PIXEL_VISITS_MAX);
    assert!(mesh.vertices.len() <= 300000);
    let mut camera = crate::viewport::Camera::rack((bounds[1] - bounds[0]).as_dvec3().to_array());
    camera.target = (bounds[0] + bounds[1]) / 2.0;
    let matrix = camera.matrix(WIDTH as f32 / HEIGHT as f32);
    let mut image = egui::ColorImage::filled([WIDTH, HEIGHT], crate::interface::VIEWPORT);
    let mut depth = [f32::INFINITY; WIDTH * HEIGHT];
    let light = Vec3::new(-0.4, -0.6, 0.8).normalize();
    for triangle in mesh.vertices.as_chunks::<3>().0 {
        let points = triangle.map(|vertex| {
            let clip = matrix * Vec3::from_array(vertex.position).extend(1.0);
            let point = clip.truncate() / clip.w;
            Vec3::new(
                (point.x + 1.0) * WIDTH as f32 / 2.0,
                (1.0 - point.y) * HEIGHT as f32 / 2.0,
                point.z,
            )
        });
        let normal = Vec3::from_array(triangle[0].normal);
        let shade = 0.30 + 0.70 * normal.dot(light).max(0.0);
        let [r, g, b_color, _] = triangle[0].color;
        let color: Color32 = egui::Rgba::from_rgb(
            r as f32 / 255.0 * shade,
            g as f32 / 255.0 * shade,
            b_color as f32 / 255.0 * shade,
        )
        .into();
        crate::raster::triangle(&mut image, &mut depth, points, color, remaining)?;
    }

    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inline_output::Content;
    #[test]
    fn runtime_outputs_attach_to_calls_and_thumbnails_are_bounded() -> Result<(), String> {
        let source = "local part = cad.box({width=40,depth=30,height=20})\nlocal bottom = cad.face(part,{name='bottom',normal='z-'})\nlocal top = cad.face(part,{name='top',normal='z+'})\ncad.measure('height',{from=bottom,to=top,min=10,max=25})\ncad.show('block',part)\n";
        let snapshot = crate::build_script_snapshot(source, 1)?;
        assert_eq!(snapshot.scene.outputs.len(), 2);
        assert_eq!(snapshot.scene.outputs[0].line, 3);
        assert_eq!(snapshot.scene.outputs[1].line, 4);
        assert!(matches!(
            snapshot.scene.outputs[0].content,
            Content::Measurement {
                distance_mm: 20.0,
                checked: true
            }
        ));
        let Content::Part {
            image: Some(image),
            dimensions,
            volume_mm3,
        } = &snapshot.scene.outputs[1].content
        else {
            panic!("Expected part thumbnail")
        };
        assert_eq!(image.size, [WIDTH, HEIGHT]);
        assert_eq!(*dimensions, Vec3::new(40.0, 30.0, 20.0));
        assert!((*volume_mm3 - 24000.0).abs() < 0.001);
        assert!(
            image
                .pixels
                .iter()
                .filter(|pixel| **pixel != crate::interface::VIEWPORT)
                .count()
                > 1000
        );
        let mut remaining = 0;
        assert!(
            render(
                &snapshot.scene.world_mesh(),
                snapshot.scene.bounds,
                &mut remaining
            )
            .is_none()
        );
        assert_eq!(remaining, 0);
        let moved = crate::build_script_snapshot(
            "local part=cad.box({width=40,depth=30,height=20});cad.show('moved',cad.translate(part,{x=100,y=-50,z=35}))",
            2,
        )?;
        let Content::Part {
            image: Some(moved_image),
            ..
        } = &moved.scene.outputs[0].content
        else {
            panic!("Expected translated thumbnail")
        };
        assert!(
            image
                .pixels
                .iter()
                .zip(&moved_image.pixels)
                .filter(|(a, b)| a != b)
                .count()
                < 50
        );
        let text =
            "local part=cad.box({width=10,depth=10,height=10})\ncad.show(\n'block',\npart\n)\n";
        let multiline = crate::model::compile(text)?;
        assert_eq!(multiline.parts[0].line, 1);
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_luau::LANGUAGE.into())
            .map_err(|error| error.to_string())?;
        let tree = parser.parse(text, None).ok_or("Missing call tree")?;
        assert_eq!(crate::syntax::call_end(&tree, multiline.parts[0].line), 4);
        Ok(())
    }
}
