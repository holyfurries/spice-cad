use bevy::{
    asset::{
        AssetPlugin, LoadState,
        io::{
            AssetSourceBuilder, AssetSourceId,
            memory::{Dir, MemoryAssetReader},
        },
    },
    color::ColorToComponents,
    gltf::{Gltf, GltfLoaderSettings, GltfMaterial, GltfMesh, GltfNode},
    mesh::{MeshPlugin, VertexAttributeValues},
    prelude::*,
    world_serialization::WorldSerializationPlugin,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).take(3).collect();
    let [bundle, report] = arguments.as_slice() else {
        return Err("Usage: spice-game-import BUNDLE_DIRECTORY NEW_REPORT.json".into());
    };
    let bundle = Path::new(bundle).canonicalize()?;
    let glb = read_bounded(&bundle.join("model.glb"), 32 * 1024 * 1024)?;
    let digest = format!("{:x}", Sha256::digest(&glb));
    let metadata: Value =
        serde_json::from_slice(&read_bounded(&bundle.join("asset.json"), 2 * 1024 * 1024)?)?;
    if metadata["schema_version"] != 1 || metadata["files"]["glb"]["sha256"] != digest {
        return Err("Asset contract version or GLB hash mismatch".into());
    }
    let sources = metadata["files"]["sources"]
        .as_array()
        .ok_or("Missing source inventory")?;
    if sources.is_empty() || sources.len() > 64 {
        return Err("Source inventory exceeds limits".into());
    }
    let mut source_bytes = 0;
    for source in sources {
        let name = Path::new(
            source["path"]
                .as_str()
                .ok_or("Missing bundled source path")?,
        );
        if name.components().count() != 1
            || name.extension().is_none_or(|extension| extension != "luau")
        {
            return Err("Invalid bundled source path".into());
        }
        let bytes = read_bounded(&bundle.join(name), 16 * 1024 * 1024)?;
        source_bytes += bytes.len();
        if source_bytes > 16 * 1024 * 1024 {
            return Err("Source bundle exceeds 16 MiB".into());
        }
        if source["sha256"] != format!("{:x}", Sha256::digest(&bytes)) {
            return Err("Bundled source hash mismatch".into());
        }
    }
    let length = u32::from_le_bytes(glb.get(12..16).ok_or("Truncated GLB")?.try_into()?) as usize;
    let document: Value =
        serde_json::from_slice(glb.get(20..20 + length).ok_or("Truncated GLB JSON")?)?;
    let mut app = App::new();
    let directory = Dir::default();
    directory.insert_asset(Path::new("model.glb"), glb);
    let reader = MemoryAssetReader { root: directory };
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(reader.clone())),
    );
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        WorldSerializationPlugin,
        MeshPlugin,
        bevy::gltf::GltfPlugin::default(),
    ));
    app.init_asset::<Image>();
    app.finish();
    app.cleanup();
    let handle: Handle<Gltf> = app
        .world()
        .resource::<AssetServer>()
        .load_builder()
        .with_settings(|settings: &mut GltfLoaderSettings| {
            settings.validate = true;
        })
        .load("model.glb");
    let started = Instant::now();
    let mut ready = false;
    for _ in 0..30000 {
        app.update();
        let server = app.world().resource::<AssetServer>();
        if let Some(LoadState::Failed(error)) = server.get_load_state(handle.id()) {
            return Err(format!("Strict Bevy import failed: {error}").into());
        }
        if server.is_loaded_with_dependencies(handle.id()) {
            ready = true;
            break;
        }
        if started.elapsed() > Duration::from_secs(30) {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if !ready {
        return Err("Strict Bevy import timed out after 30 seconds".into());
    }
    let world = app.world();
    let loaded = world
        .resource::<Assets<Gltf>>()
        .get(&handle)
        .ok_or("Missing loaded GLB")?;
    let expected_parts = metadata["counts"]["parts"]
        .as_u64()
        .ok_or("Missing part count")? as usize;
    if loaded.meshes.len() != expected_parts {
        return Err("Loaded mesh count differs from contract".into());
    }
    let expected_materials = document["materials"]
        .as_array()
        .ok_or("Missing material inventory")?;
    if loaded.materials.len() != expected_materials.len() {
        return Err("Loaded material count differs from GLB".into());
    }
    let materials = world.resource::<Assets<GltfMaterial>>();
    let mut material_report = Vec::with_capacity(loaded.materials.len());
    for (handle, expected) in loaded.materials.iter().zip(expected_materials) {
        let material = materials.get(handle).ok_or("Missing loaded material")?;
        let ior = expected["extensions"]["KHR_materials_ior"]["ior"]
            .as_f64()
            .unwrap_or(1.5) as f32;
        let reflectance = expected["extensions"]["KHR_materials_specular"]["specularFactor"]
            .as_f64()
            .unwrap_or(1.0) as f32
            * 0.5;
        if (material.ior - ior).abs() > 1e-6 || (material.reflectance - reflectance).abs() > 1e-6 {
            return Err("Loaded IOR or reflectance differs from authored material".into());
        }
        let pbr = &expected["pbrMetallicRoughness"];
        let color = material.base_color.to_linear().to_f32_array();
        for (axis, value) in color.iter().enumerate() {
            if (*value
                - pbr["baseColorFactor"][axis]
                    .as_f64()
                    .ok_or("Missing base color")? as f32)
                .abs()
                > 1e-6
            {
                return Err("Loaded base color differs from authored material".into());
            }
        }
        if (material.metallic - pbr["metallicFactor"].as_f64().ok_or("Missing metalness")? as f32)
            .abs()
            > 1e-6
            || (material.perceptual_roughness
                - pbr["roughnessFactor"].as_f64().ok_or("Missing roughness")? as f32)
                .abs()
                > 1e-6
        {
            return Err("Loaded metalness or roughness differs from authored material".into());
        }
        material_report.push(
            json!({"name":expected["name"],"ior":material.ior,"reflectance":material.reflectance}),
        );
    }
    let node_assets = world.resource::<Assets<GltfNode>>();
    let node_records = metadata["nodes"]
        .as_array()
        .ok_or("Missing contract nodes")?;
    if loaded.nodes.len() != node_records.len() || loaded.nodes.len() > 65 {
        return Err("Loaded node count differs from contract or exceeds limit".into());
    }
    let mut nodes = vec![None; loaded.nodes.len()];
    let mut parents = vec![None; loaded.nodes.len()];
    for handle in &loaded.nodes {
        let node = node_assets.get(handle).ok_or("Missing loaded node")?;
        if node.index >= nodes.len() {
            return Err("Invalid loaded node index".into());
        }
        nodes[node.index] = Some(node);
        for child in &node.children {
            let child = node_assets.get(child).ok_or("Missing child")?;
            let parent = parents.get_mut(child.index).ok_or("Invalid child index")?;
            if parent.replace(node.index).is_some() {
                return Err("Multiple loaded node parents".into());
            }
        }
    }
    let mut transforms = vec![None; nodes.len()];
    for _ in 0..nodes.len() {
        for (index, node) in nodes.iter().enumerate() {
            if transforms[index].is_some() {
                continue;
            }
            let node = node.ok_or("Missing node index")?;
            if node_records[index]["name"] != node.name {
                return Err("Loaded node name differs from contract".into());
            }
            let parent = match parents[index] {
                Some(parent) => match transforms[parent] {
                    Some(matrix) => matrix,
                    None => continue,
                },
                None => Mat4::IDENTITY,
            };
            if node_records[index]["parent_node"] != serde_json::to_value(parents[index])? {
                return Err("Loaded hierarchy differs from contract".into());
            }
            let local = node.transform.to_matrix();
            for (axis, value) in local.to_cols_array().iter().enumerate() {
                if (*value
                    - node_records[index]["local_transform"][axis]
                        .as_f64()
                        .ok_or("Missing local matrix")? as f32)
                    .abs()
                    > 1e-5
                {
                    return Err("Loaded attachment/local transform differs from contract".into());
                }
            }
            transforms[index] = Some(parent * local);
        }
        if transforms.iter().all(Option::is_some) {
            break;
        }
    }
    let mesh_assets = world.resource::<Assets<GltfMesh>>();
    let meshes = world.resource::<Assets<Mesh>>();
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    let mut triangle_count = 0;
    let mut vertex_count = 0;
    for (index, node) in nodes.iter().enumerate() {
        let node = node.ok_or("Missing loaded node")?;
        if let Some(handle) = &node.mesh {
            let mesh = mesh_assets.get(handle).ok_or("Missing loaded GLB mesh")?;
            for primitive in &mesh.primitives {
                let mesh = meshes
                    .get(&primitive.mesh)
                    .ok_or("Missing loaded mesh data")?;
                vertex_count += mesh.count_vertices();
                triangle_count += mesh.indices().ok_or("Missing loaded indices")?.len() / 3;
                let Some(VertexAttributeValues::Float32x3(vertices)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    return Err("Missing loaded positions".into());
                };
                for position in vertices {
                    let point = transforms[index]
                        .ok_or("Cyclic loaded hierarchy")?
                        .transform_point3(Vec3::from_array(*position));
                    minimum = minimum.min(point);
                    maximum = maximum.max(point);
                }
            }
        }
    }
    if metadata["counts"]["triangles"] != triangle_count
        || metadata["counts"]["gpu_vertices"] != vertex_count
    {
        return Err("Loaded triangle or GPU vertex counts differ from contract".into());
    }
    for (key, actual) in [("minimum", minimum), ("maximum", maximum)] {
        for (axis, value) in actual.to_array().iter().enumerate() {
            if !value.is_finite()
                || (*value
                    - metadata["bounds"][key][axis]
                        .as_f64()
                        .ok_or("Missing contract bounds")? as f32)
                    .abs()
                    > 1e-5
            {
                return Err("Loaded world bounds differ from contract".into());
            }
        }
    }
    let result = json!({"schema_version":1,"glb_sha256":digest,"asset_id":metadata["asset_id"],
        "loader":"bevy 0.19.1","validator":"gltf-json 1.4.1 with required-specular allowlist correction",
        "validation_enabled":true,"result":"passed","mesh_count":loaded.meshes.len(),"materials":material_report,
        "bounds":{"minimum":minimum.to_array(),"maximum":maximum.to_array()},
        "hierarchy_and_local_transforms":"passed","render_validation":"not_run"});
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)?;
    use std::io::Write;
    output.write_all(&serde_json::to_vec_pretty(&result)?)?;
    println!(
        "Strict Bevy import passed: {} meshes, {} materials",
        loaded.meshes.len(),
        loaded.materials.len()
    );
    Ok(())
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} exceeds {limit} bytes", path.display()).into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    #[test]
    fn strict_validator_accepts_specular_and_rejects_unknown_required_extensions() {
        assert!(bevy::gltf::GltfLoaderSettings::default().validate);
        let known = br#"{"asset":{"version":"2.0"},"extensionsUsed":["KHR_materials_specular"],"extensionsRequired":["KHR_materials_specular"]}"#;
        assert!(gltf::Gltf::from_slice(known).is_ok());
        let material = br#"{"asset":{"version":"2.0"},"extensionsUsed":["KHR_materials_specular"],"extensionsRequired":["KHR_materials_specular"],"materials":[{"extensions":{"KHR_materials_specular":{"specularFactor":0.0}}}]}"#;
        let parsed = gltf::Gltf::from_slice(material).expect("Valid required specular material");
        assert_eq!(
            parsed
                .materials()
                .next()
                .expect("Material")
                .extension_value("KHR_materials_specular")
                .expect("Raw data retained for Bevy")["specularFactor"],
            0.0
        );
        let unknown = br#"{"asset":{"version":"2.0"},"extensionsUsed":["SPICE_unknown_required"],"extensionsRequired":["SPICE_unknown_required"]}"#;
        assert!(gltf::Gltf::from_slice(unknown).is_err());
        let malformed = br#"{"asset":{"version":"2.0"},"accessors":[{"componentType":0,"count":1,"type":"INVALID"}]}"#;
        assert!(gltf::Gltf::from_slice(malformed).is_err());
    }
}
