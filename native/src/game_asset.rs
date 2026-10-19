use crate::mesh::Mesh;
use glam::Vec3;
use serde_json::json;
use std::collections::BTreeMap;

pub fn glb(parts: &[crate::model::Part], meshes: &[Mesh]) -> Result<Vec<u8>, String> {
    assert_eq!(parts.len(), meshes.len());
    let count: usize = meshes.iter().map(|mesh| mesh.vertices.len()).sum();
    if parts.is_empty() || parts.len() > 32 || count > 300000 {
        return Err("Game scene exceeds its part or triangle budget".into());
    }
    let mut binary = Vec::with_capacity(count * 28);
    let mut nodes = Vec::with_capacity(parts.len());
    let mut materials = Vec::with_capacity(parts.len());
    let mut output_meshes = Vec::with_capacity(parts.len());
    let mut views = Vec::with_capacity(parts.len() * 2);
    let mut accessors = Vec::with_capacity(parts.len() * 3);
    for (part, mesh) in parts.iter().zip(meshes) {
        mesh.validate()?;
        let mut vertices = BTreeMap::new();
        let vertex_start = binary.len();
        let mut indices = Vec::with_capacity(mesh.vertices.len());
        let mut minimum = Vec3::splat(f32::INFINITY);
        let mut maximum = Vec3::splat(f32::NEG_INFINITY);
        for vertex in &mesh.vertices {
            let [x, y, z] = vertex.position;
            let position = Vec3::new(x, z, -y) * 0.001;
            let [x, y, z] = vertex.normal;
            let normal = Vec3::new(x, z, -y).normalize_or_zero();
            if !position.is_finite() || normal.length_squared() < 0.5 {
                return Err("Game mesh contains an invalid position or normal".into());
            }
            minimum = minimum.min(position);
            maximum = maximum.max(position);
            let values = [
                position.x, position.y, position.z, normal.x, normal.y, normal.z,
            ];
            let next = vertices.len() as u32;
            let key = values.map(|value| if value == 0.0 { 0 } else { value.to_bits() });
            let index = *vertices.entry(key).or_insert_with(|| {
                for value in values {
                    binary.extend_from_slice(&value.to_le_bytes());
                }
                next
            });
            indices.push(index);
        }
        let vertex_bytes = binary.len() - vertex_start;
        let index_start = binary.len();
        for index in &indices {
            binary.extend_from_slice(&index.to_le_bytes());
        }

        let index = nodes.len();
        let view = views.len();
        let accessor = accessors.len();
        nodes.push(json!({"name": part.name, "mesh": index}));
        output_meshes.push(json!({"name": part.name, "primitives": [{
            "attributes": {"POSITION": accessor, "NORMAL": accessor + 1},
            "indices": accessor + 2, "mode": 4, "material": index
        }]}));
        let [r, g, b] = part.color;
        materials.push(json!({"name": part.name, "pbrMetallicRoughness": {
            "baseColorFactor": [r, g, b, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.7
        }}));
        views.push(json!({"buffer": 0, "byteOffset": vertex_start,
            "byteLength": vertex_bytes, "byteStride": 24, "target": 34962}));
        views.push(json!({"buffer": 0, "byteOffset": index_start,
            "byteLength": indices.len() * 4, "target": 34963}));
        accessors.push(
            json!({"bufferView": view, "componentType": 5126, "count": vertices.len(),
            "type": "VEC3", "min": minimum.to_array(), "max": maximum.to_array()}),
        );
        accessors.push(
            json!({"bufferView": view, "byteOffset": 12, "componentType": 5126,
            "count": vertices.len(), "type": "VEC3"}),
        );
        accessors.push(json!({"bufferView": view + 1, "componentType": 5125,
            "count": indices.len(), "type": "SCALAR"}));
    }
    let scene_nodes: Vec<_> = (0..nodes.len()).collect();
    let document = json!({
        "asset": {"version": "2.0", "generator": "Spice CAD"},
        "scene": 0, "scenes": [{"nodes": scene_nodes}],
        "nodes": nodes, "meshes": output_meshes, "materials": materials,
        "buffers": [{"byteLength": binary.len()}],
        "bufferViews": views, "accessors": accessors
    });
    let mut json = serde_json::to_vec(&document).map_err(|error| error.to_string())?;
    json.resize(json.len().next_multiple_of(4), b' ');
    let length = 12 + 8 + json.len() + 8 + binary.len();
    let length_u32 = u32::try_from(length).map_err(|error| error.to_string())?;
    let mut output = Vec::with_capacity(length);
    output.extend_from_slice(b"glTF");
    output.extend_from_slice(&2_u32.to_le_bytes());
    output.extend_from_slice(&length_u32.to_le_bytes());
    output.extend_from_slice(&(json.len() as u32).to_le_bytes());
    output.extend_from_slice(b"JSON");
    output.extend_from_slice(&json);
    output.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    output.extend_from_slice(b"BIN\0");
    output.extend_from_slice(&binary);
    assert_eq!(output.len(), length);
    assert!(output.len().is_multiple_of(4));
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{export, kernel::Kernel, model};

    fn word(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("Complete word"))
    }

    #[test]
    fn assembly_exports_named_colored_objects_and_independent_print_parts() -> Result<(), String> {
        let source = include_str!("../../models/rocket-engine.luau");
        let model = model::compile(source)?;
        let volumes = {
            let kernel = Kernel::new()?;
            model
                .build(&kernel)?
                .iter()
                .map(|body| body.volume())
                .collect::<Result<Vec<_>, _>>()?
        };
        let folder =
            std::env::temp_dir().join(format!("spice-game-assembly-{}", std::process::id()));
        assert!(!folder.exists());
        export::model_files(source, &folder.join("game"), export::Target::Game)?;
        let bytes =
            std::fs::read(folder.join("game/model.glb")).map_err(|error| error.to_string())?;
        let json_size = word(&bytes, 12) as usize;
        let document: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_size])
            .map_err(|error| error.to_string())?;
        let binary = &bytes[28 + json_size..];
        assert_eq!(document["scenes"][0]["nodes"], json!([0, 1, 2]));
        assert_eq!(document["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(document["materials"].as_array().unwrap().len(), 3);
        for (index, part) in model.parts.iter().enumerate() {
            assert_eq!(document["nodes"][index]["name"], part.name);
            assert_eq!(document["nodes"][index]["mesh"], index);
            assert_eq!(
                document["meshes"][index]["primitives"][0]["material"],
                index
            );
            for channel in 0..3 {
                assert!(
                    (document["materials"][index]["pbrMetallicRoughness"]["baseColorFactor"]
                        [channel]
                        .as_f64()
                        .unwrap()
                        - f64::from(part.color[channel]))
                    .abs()
                        < 1e-7
                );
            }
            let vertex_view = &document["bufferViews"][index * 2];
            let index_view = &document["bufferViews"][index * 2 + 1];
            let vertex_start = vertex_view["byteOffset"].as_u64().unwrap() as usize;
            let index_start = index_view["byteOffset"].as_u64().unwrap() as usize;
            let index_size = index_view["byteLength"].as_u64().unwrap() as usize;
            let vertex_count = document["accessors"][index * 3]["count"].as_u64().unwrap() as usize;
            assert_eq!(index_start, vertex_start + vertex_count * 24);
            assert!(index_start + index_size <= binary.len());
            assert_eq!(vertex_start % 4, 0);
            assert_eq!(index_size % 12, 0);
            let mut volume = 0.0_f64;
            for triangle in binary[index_start..index_start + index_size]
                .as_chunks::<12>()
                .0
            {
                let points: [glam::DVec3; 3] = std::array::from_fn(|i| {
                    let vertex = word(triangle, i * 4) as usize;
                    assert!(vertex < vertex_count);
                    glam::DVec3::from_array(std::array::from_fn(|axis| {
                        f64::from(f32::from_bits(word(
                            binary,
                            vertex_start + vertex * 24 + axis * 4,
                        )))
                    }))
                });
                volume += points[0].dot(points[1].cross(points[2])) / 6.0;
            }
            assert!((volume * 1e9 - volumes[index]).abs() < volumes[index] * 0.02);
        }
        assert!(
            export::model_files(
                &source.replace("z = -chamber_bottom", "z = 1"),
                &folder.join("invalid-print"),
                export::Target::Print
            )
            .is_err()
        );
        assert!(!folder.join("invalid-print").exists());
        export::model_files(source, &folder.join("print"), export::Target::Print)?;
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(folder.join("print/parts.json")).unwrap())
                .map_err(|error| error.to_string())?;
        assert_eq!(manifest.as_array().unwrap().len(), 3);
        for (index, part) in model.parts.iter().enumerate() {
            assert_eq!(manifest[index]["name"], part.name);
            let step = manifest[index]["step"].as_str().unwrap();
            let shape = opencascade::primitives::Shape::read_step(folder.join("print").join(step))
                .map_err(|error| error.to_string())?;
            let mesh = shape.mesh().map_err(|error| error.to_string())?;
            let volume: f64 = mesh
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|triangle| {
                    mesh.vertices[triangle[0]]
                        .dot(mesh.vertices[triangle[1]].cross(mesh.vertices[triangle[2]]))
                        / 6.0
                })
                .sum();
            assert!((volume - volumes[index]).abs() < volumes[index] * 0.002);
            let stl = manifest[index]["stl"].as_str().unwrap();
            let bytes =
                std::fs::read(folder.join("print").join(stl)).map_err(|error| error.to_string())?;
            assert_eq!(bytes.len(), 84 + word(&bytes, 80) as usize * 50);
        }
        assert_eq!(
            std::fs::read_to_string(folder.join("print/model.luau")).unwrap(),
            source
        );
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn game_export_preserves_pivot_scale_winding_and_hard_edges() -> Result<(), String> {
        let source =
            "cad.show('game box',cad.box({width=300,depth=20,height=30,x=-150,y=40,z=-15}))";
        let folder = std::env::temp_dir().join(format!("spice-game-box-{}", std::process::id()));
        assert!(!folder.exists());
        export::model_files(source, &folder, export::Target::Game)?;
        let bytes = std::fs::read(folder.join("model.glb")).map_err(|error| error.to_string())?;
        assert_eq!(&bytes[..4], b"glTF");
        assert_eq!(word(&bytes, 4), 2);
        assert_eq!(word(&bytes, 8) as usize, bytes.len());
        let json_size = word(&bytes, 12) as usize;
        assert_eq!(json_size % 4, 0);
        assert_eq!(&bytes[16..20], b"JSON");
        let document: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_size])
            .map_err(|error| error.to_string())?;
        let binary_start = 28 + json_size;
        assert_eq!(&bytes[binary_start - 4..binary_start], b"BIN\0");
        assert_eq!(
            word(&bytes, binary_start - 8) as usize,
            bytes.len() - binary_start
        );
        assert_eq!(document["nodes"][0]["name"], "game box");
        assert_eq!(document["accessors"][0]["count"], 24);
        assert_eq!(document["accessors"][2]["count"], 36);
        let vertex_bytes = document["bufferViews"][0]["byteLength"].as_u64().unwrap() as usize;
        let binary = &bytes[binary_start..];
        let vector = |offset: usize| {
            Vec3::from_array(std::array::from_fn(|index| {
                f32::from_bits(word(binary, offset + index * 4))
            }))
        };
        let mut minimum = Vec3::splat(f32::INFINITY);
        let mut maximum = Vec3::splat(f32::NEG_INFINITY);
        let mut volume = 0.0_f32;
        for triangle in binary[vertex_bytes..].as_chunks::<12>().0 {
            let offsets: [usize; 3] = std::array::from_fn(|i| word(triangle, i * 4) as usize * 24);
            let points = offsets.map(vector);
            let normal = (points[1] - points[0]).cross(points[2] - points[0]);
            assert!(normal.length_squared() > 0.0);
            volume += points[0].dot(points[1].cross(points[2])) / 6.0;
            for offset in offsets {
                assert!(offset < vertex_bytes);
                let position = vector(offset);
                let supplied_normal = vector(offset + 12);
                assert!((supplied_normal.length() - 1.0).abs() < 1e-6);
                assert!(normal.normalize().dot(supplied_normal) > 0.999);
                minimum = minimum.min(position);
                maximum = maximum.max(position);
            }
        }
        assert!((minimum - Vec3::new(-0.15, -0.015, -0.06)).length() < 1e-6);
        assert!((maximum - Vec3::new(0.15, 0.015, -0.04)).length() < 1e-6);
        assert!((volume - 0.00018).abs() < 1e-9);
        for (key, expected) in [("min", minimum), ("max", maximum)] {
            let declared = Vec3::from_array(std::array::from_fn(|i| {
                document["accessors"][0][key][i].as_f64().unwrap() as f32
            }));
            assert_eq!(declared, expected);
        }
        assert_eq!(
            std::fs::read_to_string(folder.join("model.luau")).unwrap(),
            source
        );
        assert!(!folder.join("model.stl").exists());
        assert!(folder.join("GAME_ASSET.md").exists());
        assert!(export::model_files(source, &folder, export::Target::Game).is_err());
        assert_eq!(std::fs::read(folder.join("model.glb")).unwrap(), bytes);
        std::fs::remove_dir_all(&folder).map_err(|error| error.to_string())?;
        assert!(export::model_files(source, &folder, export::Target::Print).is_err());
        assert!(!folder.exists());
        Ok(())
    }

    #[test]
    fn game_tessellation_reduces_curved_mesh_without_changing_the_solid() -> Result<(), String> {
        let model = model::compile("cad.show('cylinder',cad.cylinder({radius=30,height=20}))")?;
        let kernel = Kernel::new()?;
        let mut body = model.build(&kernel)?.remove(0);
        let volume_before = body.volume()?;
        let print = body.mesh()?;
        let mut game = body.game_mesh()?;
        assert!(game.vertices.len() < print.vertices.len());
        game.validate()?;
        game.smooth_normals();
        for vertex in &game.vertices {
            assert!((Vec3::from_array(vertex.normal).length() - 1.0).abs() < 1e-5);
        }
        assert!(!glb(&model.parts, &[game])?.is_empty());
        assert!((body.volume()? - volume_before).abs() < 1e-6);
        Ok(())
    }
}
