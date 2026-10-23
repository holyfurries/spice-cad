use crate::mesh::Mesh;
use glam::Vec3;
use serde_json::json;
use std::collections::BTreeMap;

pub fn glb(model: &crate::model::Model, meshes: &[Mesh]) -> Result<Vec<u8>, String> {
    let parts = &model.parts;
    let group_worlds = crate::assembly::world_placements(&model.groups)?;
    assert_eq!(parts.len(), meshes.len());
    let count: usize = meshes.iter().map(|mesh| mesh.vertices.len()).sum();
    if parts.is_empty() || parts.len() > 32 || count > 300000 {
        return Err("Game scene exceeds its part or triangle budget".into());
    }
    let mut binary = Vec::with_capacity(count * 28);
    let mut nodes = Vec::with_capacity(parts.len() + model.groups.len());
    let mut materials = Vec::with_capacity(parts.len());
    let mut material_keys = Vec::with_capacity(parts.len());
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
        let placement = if let Some(parent) = part.parent {
            group_worlds[parent].matrix().inverse() * part.assembly.matrix()
        } else {
            part.assembly.matrix()
        };
        let mut node = placement_node(&part.name, placement);
        node["mesh"] = json!(index);
        nodes.push(node);
        let material_index = match material_keys
            .iter()
            .position(|material| *material == &part.material)
        {
            Some(index) => index,
            None => {
                let material = &part.material;
                let [r, g, b] = material.base_color;
                let mut definition = json!({"name": material.name, "pbrMetallicRoughness": {
                    "baseColorFactor": [r, g, b, 1.0], "metallicFactor": material.base_metalness, "roughnessFactor": material.specular_roughness
                }, "extras": {"spice_cad": {"openpbr_version": "1.1.1", "openpbr": {
                    "base_color": [r,g,b], "base_metalness": material.base_metalness,
                    "specular_roughness": material.specular_roughness, "specular_weight": material.specular_weight,
                    "specular_ior": material.specular_ior
                }}}});
                let mut extensions = serde_json::Map::new();
                if material.effective_ior() != 1.5 {
                    extensions.insert(
                        "KHR_materials_ior".into(),
                        json!({"ior": material.effective_ior()}),
                    );
                }
                if material.effective_ior() == 1.0 {
                    extensions.insert(
                        "KHR_materials_specular".into(),
                        json!({"specularFactor": 0.0}),
                    );
                }
                if !extensions.is_empty() {
                    definition["extensions"] = extensions.into();
                }
                materials.push(definition);
                material_keys.push(material);
                materials.len() - 1
            }
        };
        output_meshes.push(json!({"name": part.name, "primitives": [{
            "attributes": {"POSITION": accessor, "NORMAL": accessor + 1},
            "indices": accessor + 2, "mode": 4, "material": material_index
        }]}));
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
    for group in &model.groups {
        nodes.push(placement_node(&group.name, group.placement.matrix()));
    }
    let mut children = vec![Vec::<usize>::new(); nodes.len()];
    let mut scene_nodes = Vec::with_capacity(nodes.len());
    for (index, parent) in parts
        .iter()
        .map(|part| part.parent)
        .chain(model.groups.iter().map(|group| group.parent))
        .enumerate()
    {
        if let Some(parent) = parent {
            children[parts.len() + parent].push(index);
        } else {
            scene_nodes.push(index);
        }
    }
    for (node, children) in nodes.iter_mut().zip(children) {
        if !children.is_empty() {
            node["children"] = json!(children);
        }
    }
    let mut document = json!({
        "asset": {"version": "2.0", "generator": "Spice CAD"},
        "scene": 0, "scenes": [{"nodes": scene_nodes}],
        "nodes": nodes, "meshes": output_meshes, "materials": materials,
        "buffers": [{"byteLength": binary.len()}],
        "bufferViews": views, "accessors": accessors
    });
    let mut extensions = Vec::with_capacity(2);
    if material_keys
        .iter()
        .any(|material| material.effective_ior() != 1.5)
    {
        extensions.push("KHR_materials_ior");
    }
    if material_keys
        .iter()
        .any(|material| material.effective_ior() == 1.0)
    {
        extensions.push("KHR_materials_specular");
    }
    if !extensions.is_empty() {
        document["extensionsUsed"] = json!(extensions);
        document["extensionsRequired"] = json!(extensions);
    }
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

fn placement_node(name: &str, placement: glam::Mat4) -> serde_json::Value {
    assert!(placement.is_finite());
    assert!(!name.is_empty());
    let basis = glam::Mat4::from_cols(
        glam::Vec4::X,
        glam::vec4(0.0, 0.0, -1.0, 0.0),
        glam::Vec4::Y,
        glam::Vec4::W,
    );
    let placement = basis * placement * basis.transpose();
    let rotation = glam::Quat::from_mat4(&placement).normalize().to_array();
    let translation = (placement.w_axis.truncate() * 0.001).to_array();
    json!({"name": name, "rotation": rotation, "translation": translation})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{export, kernel::Kernel, model};

    fn word(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("Complete word"))
    }

    #[test]
    fn openpbr_exports_map_weight_to_ior_and_preserve_source_parameters() -> Result<(), String> {
        let folder = std::env::temp_dir().join(format!("spice-openpbr-{}", std::process::id()));
        for (index, (ior, weight, expected)) in [
            (1.5, 1.0, 1.5),
            (1.5, 0.25, 1.2222222222),
            (2.0, 0.25, 1.4),
            (1.5, 0.0, 1.0),
            (1.0, 1.0, 1.0),
        ]
        .into_iter()
        .enumerate()
        {
            let source = format!(
                "local m=cad.material('reference',{{specular_ior={ior},specular_weight={weight},specular_roughness=0.6}});cad.show('box',cad.box({{width=1,depth=1,height=1}}),{{material=m}})"
            );
            let target = folder.join(index.to_string());
            export::model_files(&source, &target, export::Target::Game)?;
            let bytes =
                std::fs::read(target.join("model.glb")).map_err(|error| error.to_string())?;
            let json_size = word(&bytes, 12) as usize;
            let document: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_size])
                .map_err(|error| error.to_string())?;
            let material = &document["materials"][0];
            let metadata = &material["extras"]["spice_cad"];
            assert_eq!(metadata["openpbr_version"], "1.1.1");
            assert_eq!(metadata["openpbr"]["specular_ior"], ior);
            assert_eq!(metadata["openpbr"]["specular_weight"], weight);
            assert!(
                (material["pbrMetallicRoughness"]["roughnessFactor"]
                    .as_f64()
                    .unwrap()
                    - 0.6)
                    .abs()
                    < 1e-6
            );
            if index == 0 {
                assert!(material["extensions"].is_null());
                assert!(document["extensionsUsed"].is_null());
            } else {
                let actual = material["extensions"]["KHR_materials_ior"]["ior"]
                    .as_f64()
                    .unwrap();
                assert!((actual - expected).abs() < 1e-6);
                assert_eq!(document["extensionsUsed"], document["extensionsRequired"]);
                assert_eq!(document["extensionsUsed"][0], "KHR_materials_ior");
            }
            if expected == 1.0 {
                assert_eq!(
                    material["extensions"]["KHR_materials_specular"]["specularFactor"],
                    0.0
                );
            } else {
                assert!(material["extensions"]["KHR_materials_specular"].is_null());
            }
            let notes = std::fs::read_to_string(target.join("GAME_ASSET.md"))
                .map_err(|error| error.to_string())?;
            assert!(notes.contains("Mapping is lossy"));
        }
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn farmstead_exports_four_colored_low_poly_parts_at_game_scale() -> Result<(), String> {
        let source = include_str!("../../../examples/farmstead.luau");
        let folder = std::env::temp_dir().join(format!("spice-farmstead-{}", std::process::id()));
        for scale in [0.25_f64, 1.0, 2.0] {
            let source = source.replace("cad.number(1,", &format!("cad.number({scale},"));
            let target = folder.join(scale.to_string());
            export::model_files(&source, &target, export::Target::Game)?;
            let bytes =
                std::fs::read(target.join("model.glb")).map_err(|error| error.to_string())?;
            let json_size = word(&bytes, 12) as usize;
            let document: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_size])
                .map_err(|error| error.to_string())?;
            let nodes = document["nodes"].as_array().unwrap();
            assert_eq!(nodes.len(), 4);
            assert_eq!(document["materials"].as_array().unwrap().len(), 3);
            assert_eq!(
                document["extensionsUsed"],
                json!(["KHR_materials_ior", "KHR_materials_specular"])
            );
            assert_eq!(document["meshes"][3]["primitives"][0]["material"], 0);
            for material in document["materials"].as_array().unwrap() {
                assert_eq!(material["pbrMetallicRoughness"]["roughnessFactor"], 1.0);
                assert_eq!(
                    material["extensions"]["KHR_materials_specular"]["specularFactor"],
                    0.0
                );
            }
            let mut triangles = 0;
            let mut minimum = [f64::INFINITY; 3];
            let mut maximum = [f64::NEG_INFINITY; 3];
            for (index, name) in [
                "Plaster body",
                "Terracotta roof",
                "Timber door",
                "Plaster chimney",
            ]
            .iter()
            .enumerate()
            {
                assert_eq!(nodes[index]["name"], *name);
                let accessor = &document["accessors"][index * 3];
                for axis in 0..3 {
                    minimum[axis] = minimum[axis].min(accessor["min"][axis].as_f64().unwrap());
                    maximum[axis] = maximum[axis].max(accessor["max"][axis].as_f64().unwrap());
                }
                triangles += document["accessors"][index * 3 + 2]["count"]
                    .as_u64()
                    .unwrap()
                    / 3;
            }
            assert_eq!(triangles, 44);
            for (axis, (low, high)) in [(-0.0575, 0.0575), (0.0, 0.1155), (-0.05, 0.05)]
                .into_iter()
                .enumerate()
            {
                assert!((minimum[axis] - low * scale).abs() < 1e-7);
                assert!((maximum[axis] - high * scale).abs() < 1e-7);
            }
            let plaster = document["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"][0]
                .as_f64()
                .unwrap();
            assert!((plaster - ((238.0_f64 / 255.0 + 0.055) / 1.055).powf(2.4)).abs() < 1e-6);
        }
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn assembly_exports_named_colored_objects_and_independent_print_parts() -> Result<(), String> {
        let source = include_str!("../../../examples/rocket-engine.luau");
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
            assert_eq!(document["materials"][index]["name"], part.material.name);
            for (field, expected) in [
                ("roughnessFactor", part.material.specular_roughness),
                ("metallicFactor", part.material.base_metalness),
            ] {
                let actual = document["materials"][index]["pbrMetallicRoughness"][field]
                    .as_f64()
                    .unwrap();
                assert!((actual - f64::from(expected)).abs() < 1e-7);
            }
            for channel in 0..3 {
                assert!(
                    (document["materials"][index]["pbrMetallicRoughness"]["baseColorFactor"]
                        [channel]
                        .as_f64()
                        .unwrap()
                        - f64::from(part.material.base_color[channel]))
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
            assert!(folder.join("print").join(step).is_file());
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
    fn infantry_pose_controls_reuse_geometry_and_equipment_variants_export() -> Result<(), String> {
        let source = include_str!("../../../examples/infantry.luau");
        {
            let kernel = Kernel::new()?;
            let mut cache = model::Cache::new(&kernel);
            let first = cache.compile(source)?;
            let initial = cache.prepare(&first)?;
            assert_eq!(first.parts.len(), 21);
            let display = cache.display(&initial.geometry)?;
            let posed_source = source
                .replace("cad.number(-12,", "cad.number(35,")
                .replace("cad.number(0,", "cad.number(25,");
            let posed = cache.compile(&posed_source)?;
            let prepared = cache.prepare(&posed)?;
            assert_eq!(prepared.hits, 21);
            assert_eq!(prepared.build_ms, 0.0);
            assert_eq!(prepared.tessellation_ms, 0.0);
            assert!(std::sync::Arc::ptr_eq(
                &display,
                &cache.display(&prepared.geometry)?
            ));
            for (index, geometry) in prepared.geometry.iter().enumerate() {
                assert!(std::sync::Arc::ptr_eq(geometry, &initial.geometry[index]));
                geometry.mesh.validate()?;
            }
            assert!(
                first
                    .parts
                    .iter()
                    .zip(&posed.parts)
                    .any(|(first, posed)| first.assembly != posed.assembly)
            );
        }
        let folder =
            std::env::temp_dir().join(format!("spice-infantry-export-{}", std::process::id()));
        for (index, size) in [0.8, 1.0, 1.4].into_iter().enumerate() {
            let source = source.replace("cad.number(1,", &format!("cad.number({size},"));
            for equipment in [true, false] {
                let source = if equipment {
                    source.clone()
                } else {
                    source.replace("cad.toggle(true,", "cad.toggle(false,")
                };
                let target = folder.join(format!("{index}-{equipment}"));
                export::model_files(&source, &target, export::Target::Game)?;
                let bytes =
                    std::fs::read(target.join("model.glb")).map_err(|error| error.to_string())?;
                let json_size = word(&bytes, 12) as usize;
                let document: serde_json::Value =
                    serde_json::from_slice(&bytes[20..20 + json_size])
                        .map_err(|error| error.to_string())?;
                assert_eq!(
                    document["nodes"].as_array().unwrap().len(),
                    if equipment { 21 } else { 13 }
                );
                let triangles: u64 = document["accessors"]
                    .as_array()
                    .unwrap()
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|accessors| accessors[2]["count"].as_u64().unwrap() / 3)
                    .sum();
                assert!(
                    triangles <= 750,
                    "{size}/{equipment}: {triangles} triangles"
                );
                assert!(document.get("extensionsRequired").is_none());
                assert_eq!(
                    std::fs::read_to_string(target.join("model.luau")).unwrap(),
                    source
                );
                eprintln!(
                    "Infantry head size {size}, equipment {equipment}: {triangles} triangles, {} bytes GLB",
                    bytes.len()
                );
            }
        }
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn game_poses_preserve_local_vertices_joint_origins_and_world_normals() -> Result<(), String> {
        let source = "cad.show('arm',cad.box({width=4,depth=6,height=25,x=-2,y=-3,z=-25}),{assembly={position={x=20,y=-10,z=60},rotation={x=ANGLE,y=20,z=-15}}})";
        let folder =
            std::env::temp_dir().join(format!("spice-joint-export-{}", std::process::id()));
        let mut previous_binary = None;
        for angle in [0, 35] {
            let source = source.replace("ANGLE", &angle.to_string());
            let target = folder.join(angle.to_string());
            export::model_files(&source, &target, export::Target::Game)?;
            let bytes =
                std::fs::read(target.join("model.glb")).map_err(|error| error.to_string())?;
            let json_size = word(&bytes, 12) as usize;
            let document: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_size])
                .map_err(|error| error.to_string())?;
            let binary = &bytes[28 + json_size..];
            if let Some(previous) = previous_binary {
                assert_eq!(previous, binary);
            }
            previous_binary = Some(binary.to_vec());
            let node = &document["nodes"][0];
            assert!(node.get("matrix").is_none());
            let translation = Vec3::from_array(std::array::from_fn(|index| {
                node["translation"][index].as_f64().unwrap() as f32
            }));
            let rotation = glam::Quat::from_array(std::array::from_fn(|index| {
                node["rotation"][index].as_f64().unwrap() as f32
            }));
            assert!(translation.distance(glam::vec3(0.02, 0.06, 0.01)) < 1e-7);
            assert!((rotation.length() - 1.0).abs() < 1e-6);
            let model = model::compile(&source)?;
            let kernel = Kernel::new()?;
            let mesh = model.meshes(&mut model.build(&kernel)?, true)?.remove(0);
            let index_start = document["bufferViews"][1]["byteOffset"].as_u64().unwrap() as usize;
            let basis = |point: Vec3| Vec3::new(point.x, point.z, -point.y);
            for (index, vertex) in mesh.vertices.iter().enumerate() {
                let vertex_index = word(binary, index_start + index * 4) as usize;
                let vector = |offset: usize| {
                    Vec3::from_array(std::array::from_fn(|component| {
                        f32::from_bits(word(binary, vertex_index * 24 + offset + component * 4))
                    }))
                };
                let position = vector(0);
                let normal = vector(12);
                let original = Vec3::from_array(vertex.position);
                assert!(position.distance(basis(original) * 0.001) < 1e-7);
                let matrix = model.parts[0].assembly.matrix();
                let expected = basis(matrix.transform_point3(original)) * 0.001;
                assert!((rotation * position + translation).distance(expected) < 1e-7);
                let expected = basis(matrix.transform_vector3(Vec3::from_array(vertex.normal)));
                assert!((rotation * normal).distance(expected) < 1e-6);
            }
        }
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
        let game = body.game_mesh()?;
        assert!((print.volume_mm3 - volume_before).abs() < 1e-6);
        assert!((game.volume_mm3 - volume_before).abs() < 1e-6);
        assert!((game.mesh.volume_mm3() - game.volume_mm3).abs() > 0.1);
        let print = print.mesh;
        let mut game = game.mesh;
        assert!(game.vertices.len() < print.vertices.len());
        game.validate()?;
        game.smooth_normals();
        for vertex in &game.vertices {
            assert!((Vec3::from_array(vertex.normal).length() - 1.0).abs() < 1e-5);
        }
        assert!(!glb(&model, &[game])?.is_empty());
        assert!((body.volume()? - volume_before).abs() < 1e-6);
        Ok(())
    }
}
