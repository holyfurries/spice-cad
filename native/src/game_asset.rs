use crate::mesh::Mesh;
use glam::Vec3;
use serde_json::json;
use std::collections::BTreeMap;

pub fn glb(name: &str, mesh: &Mesh) -> Result<Vec<u8>, String> {
    mesh.validate()?;
    let mut vertices = BTreeMap::new();
    let mut binary = Vec::with_capacity(mesh.vertices.len() * 24);
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
    let vertex_bytes = binary.len();
    for index in &indices {
        binary.extend_from_slice(&index.to_le_bytes());
    }
    let document = json!({
        "asset": {"version": "2.0", "generator": "Spice CAD"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": name, "mesh": 0}],
        "meshes": [{"name": name, "primitives": [{
            "attributes": {"POSITION": 0, "NORMAL": 1}, "indices": 2, "mode": 4,
            "material": 0
        }]}],
        "materials": [{"name": "default", "pbrMetallicRoughness": {
            "baseColorFactor": [0.65, 0.68, 0.72, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.7
        }}],
        "buffers": [{"byteLength": binary.len()}],
        "bufferViews": [
            {"buffer": 0, "byteLength": vertex_bytes, "byteStride": 24, "target": 34962},
            {"buffer": 0, "byteOffset": vertex_bytes, "byteLength": indices.len() * 4, "target": 34963}
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": vertices.len(), "type": "VEC3",
             "min": minimum.to_array(), "max": maximum.to_array()},
            {"bufferView": 0, "byteOffset": 12, "componentType": 5126, "count": vertices.len(), "type": "VEC3"},
            {"bufferView": 1, "componentType": 5125, "count": indices.len(), "type": "SCALAR"}
        ]
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
        let mut body = model.build(&kernel)?;
        let volume_before = body.volume()?;
        let print = body.mesh()?;
        let mut game = body.game_mesh()?;
        assert!(game.vertices.len() < print.vertices.len());
        game.validate()?;
        game.smooth_normals();
        for vertex in &game.vertices {
            assert!((Vec3::from_array(vertex.normal).length() - 1.0).abs() < 1e-5);
        }
        assert!(!glb(&model.name, &game)?.is_empty());
        assert!((body.volume()? - volume_before).abs() < 1e-6);
        Ok(())
    }
}
