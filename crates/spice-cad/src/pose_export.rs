use crate::{
    export, game_asset, game_contract, kernel::Kernel, model, poses::Pose, project::Project,
};
use serde_json::{Value, json};
use std::{fs, io::Read, path::Path};

pub fn export(source: &str, project: Option<&Project>, folder: &Path) -> Result<(), String> {
    let kernel = Kernel::new()?;
    let mut cache = model::Cache::new(&kernel);
    export_cached(source, project, folder, &mut cache)
}

pub fn export_cached(
    source: &str,
    project: Option<&Project>,
    folder: &Path,
    cache: &mut model::Cache<'_>,
) -> Result<(), String> {
    let initial = cache.compile_project(source, project)?;
    if initial.poses.is_empty() {
        return Err("Model declares no cad.pose entries".into());
    }
    let poses = initial.poses;
    if let Some(parent) = folder.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::create_dir(folder)
        .map_err(|e| format!("{}: {e}; choose a new export directory", folder.display()))?;
    let mut entries = Vec::with_capacity(poses.len());
    for pose in &poses {
        let model = cache.compile_pose(source, project, &pose.name)?;
        let prepared = cache.prepare(&model)?;
        let mut bodies = prepared
            .bodies
            .iter()
            .map(|body| body.try_clone())
            .collect::<Result<Vec<_>, _>>()?;
        let meshes = prepared
            .geometry
            .iter()
            .map(|geometry| &geometry.mesh)
            .collect::<Vec<_>>();
        export::write_model(
            source,
            project,
            &folder.join(&pose.name),
            export::Target::Game,
            &model,
            &mut bodies,
            &meshes,
        )?;
        let contract: Value = serde_json::from_slice(
            &fs::read(folder.join(&pose.name).join("asset.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        entries.push(json!({"name":pose.name,"parameters":pose.parameters,"directory":pose.name,
            "glb_sha256":contract["files"]["glb"]["sha256"],"geometry_cache_hits":prepared.hits,
            "parts":model.parts.len(),"build_ms":prepared.build_ms,"tessellation_ms":prepared.tessellation_ms}));
    }
    let library = library(folder, &poses)?;
    let manifest = json!({"schema_version":1,"source_sha256":game_contract::hash(source.as_bytes()),"poses":entries,
        "animation_library":library.as_ref().map(|bytes| json!({"path":"model.glb","sha256":game_contract::hash(bytes)})),
        "animation_library_omission":if library.is_none() {Some("poses change geometry, materials, or hierarchy; separate GLBs retain each evaluated result")} else {None}});
    if let Some(bytes) = library {
        let first = folder.join(&poses[0].name);
        let mut metadata: Value =
            serde_json::from_slice(&fs::read(first.join("asset.json")).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        metadata["files"]["glb"]["sha256"] = json!(game_contract::hash(&bytes));
        metadata["pose"] = Value::Null;
        metadata["features"]["animation_clips"] = json!(true);
        for source in metadata["files"]["sources"]
            .as_array()
            .ok_or("Missing source inventory")?
        {
            let name = source["path"].as_str().ok_or("Missing source path")?;
            fs::copy(first.join(name), folder.join(name)).map_err(|e| e.to_string())?;
        }
        fs::write(folder.join("model.glb"), bytes).map_err(|e| e.to_string())?;
        fs::write(
            folder.join("asset.json"),
            serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let notes = "# Named pose library\n\nImport model.glb with a standard glTF 2.0 loader. Named single-key STEP animation clips assign complete node transforms to select static rigid poses; they do not define a timed walk cycle or require skinning. All clips share geometry, materials and hierarchy. Coordinates are right-handed metres, +Y up. Retain hierarchy and export-root transforms without applying them twice. Named references in extras.spice_cad describe the default scene pose. Optional KHR_xmp_json_ld carries provenance; retain required material extensions. asset.json and batch.json contain source integrity and build verification records, not prerequisites for ordinary GLB loading. Editable source is bundled unchanged. UVs, textures, collision meshes, LODs and skeletal rigs are not generated.\n";
        fs::write(folder.join("GAME_ASSET.md"), notes).map_err(|e| e.to_string())?;
    }
    fs::write(
        folder.join("batch.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn read_glb(path: &Path) -> Result<(Value, Vec<u8>), String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Pose GLB exceeds 32 MiB".into());
    }
    let length = u32::from_le_bytes(
        bytes
            .get(12..16)
            .ok_or("Truncated GLB")?
            .try_into()
            .map_err(|_| "Invalid GLB")?,
    ) as usize;
    let document = serde_json::from_slice(bytes.get(20..20 + length).ok_or("Truncated JSON")?)
        .map_err(|e| e.to_string())?;
    let binary = bytes.get(28 + length..).ok_or("Truncated binary")?.to_vec();
    Ok((document, binary))
}

fn library(folder: &Path, poses: &[Pose]) -> Result<Option<Vec<u8>>, String> {
    assert!(!poses.is_empty());
    assert!(poses.len() <= 32);
    let (mut document, mut binary) = read_glb(&folder.join(&poses[0].name).join("model.glb"))?;
    let base_nodes = document["nodes"].as_array().ok_or("Missing nodes")?.clone();
    let mut snapshots = Vec::with_capacity(poses.len());
    for pose in poses {
        let (snapshot, data) = read_glb(&folder.join(&pose.name).join("model.glb"))?;
        if data != binary
            || ["meshes", "materials", "accessors", "bufferViews", "scenes"]
                .iter()
                .any(|key| snapshot[*key] != document[*key])
        {
            return Ok(None);
        }
        let nodes = snapshot["nodes"].as_array().ok_or("Missing pose nodes")?;
        if nodes.len() != base_nodes.len() {
            return Ok(None);
        }
        for (base, node) in base_nodes.iter().zip(nodes) {
            if ["name", "children", "mesh"]
                .iter()
                .any(|key| base[*key] != node[*key])
            {
                return Ok(None);
            }
        }
        snapshots.push(nodes.clone());
    }
    let time_view = document["bufferViews"]
        .as_array()
        .ok_or("Missing views")?
        .len();
    document["bufferViews"]
        .as_array_mut()
        .ok_or("Missing views")?
        .push(json!({"buffer":0,"byteOffset":binary.len(),"byteLength":4}));
    binary.extend_from_slice(&0_f32.to_le_bytes());
    let time = document["accessors"]
        .as_array()
        .ok_or("Missing accessors")?
        .len();
    document["accessors"].as_array_mut().ok_or("Missing accessors")?.push(json!({"bufferView":time_view,"componentType":5126,"count":1,"type":"SCALAR","min":[0],"max":[0]}));
    let mut animations = Vec::with_capacity(poses.len());
    for (pose, nodes) in poses.iter().zip(snapshots) {
        let mut channels = Vec::with_capacity(nodes.len() * 3);
        let mut samplers = Vec::with_capacity(nodes.len() * 3);
        for (node_index, node) in nodes.iter().enumerate() {
            for (path, default) in [
                ("translation", json!([0, 0, 0])),
                ("rotation", json!([0, 0, 0, 1])),
                ("scale", json!([1, 1, 1])),
            ] {
                let values = node
                    .get(path)
                    .unwrap_or(&default)
                    .as_array()
                    .ok_or("Invalid pose transform")?;
                let start = binary.len();
                for value in values {
                    binary.extend_from_slice(
                        &(value.as_f64().ok_or("Invalid transform number")? as f32).to_le_bytes(),
                    );
                }
                let view = document["bufferViews"]
                    .as_array()
                    .ok_or("Missing views")?
                    .len();
                document["bufferViews"]
                    .as_array_mut()
                    .ok_or("Missing views")?
                    .push(json!({"buffer":0,"byteOffset":start,"byteLength":binary.len()-start}));
                let output = document["accessors"]
                    .as_array()
                    .ok_or("Missing accessors")?
                    .len();
                document["accessors"].as_array_mut().ok_or("Missing accessors")?.push(json!({"bufferView":view,"componentType":5126,"count":1,"type":if path=="rotation" {"VEC4"} else {"VEC3"}}));
                channels.push(
                    json!({"sampler":samplers.len(),"target":{"node":node_index,"path":path}}),
                );
                samplers.push(json!({"input":time,"output":output,"interpolation":"STEP"}));
            }
        }
        animations.push(json!({"name":pose.name,"channels":channels,"samplers":samplers}));
    }
    document["animations"] = json!(animations);
    document["extras"]["spice_cad"]["pose"] = Value::Null;
    document["buffers"][0]["byteLength"] = json!(binary.len());
    Ok(Some(game_asset::encode(&document, &binary)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infantry_batch_reuses_geometry_and_exports_portable_pose_clips() -> Result<(), String> {
        let output = std::env::temp_dir().join(format!("spice-named-poses-{}", std::process::id()));
        let source = include_str!("../../../examples/infantry.luau");
        export(source, None, &output)?;
        let manifest: Value = serde_json::from_slice(
            &fs::read(output.join("batch.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let entries = manifest["poses"].as_array().ok_or("Missing poses")?;
        assert_eq!(entries.len(), 3);
        for entry in &entries[1..] {
            assert_eq!(entry["geometry_cache_hits"], 13);
            assert_eq!(entry["build_ms"], 0.0);
            assert_eq!(entry["tessellation_ms"], 0.0);
        }
        let (library, _) = read_glb(&output.join("model.glb"))?;
        assert_eq!(library["meshes"].as_array().unwrap().len(), 13);
        assert_eq!(library["animations"].as_array().unwrap().len(), 3);
        assert_eq!(library["animations"][1]["name"], "step-left");
        for name in ["idle", "step-left", "step-right"] {
            let (document, _) = read_glb(&output.join(name).join("model.glb"))?;
            let reference = &document["extras"]["spice_cad"]["references"]["body"];
            assert!((reference["size"][1].as_f64().unwrap() - 0.0546).abs() < 1e-7);
            assert_eq!(reference["minimum"][1], 0.0);
            assert_eq!(document["extras"]["spice_cad"]["pose"], name);
            assert_eq!(
                fs::read_to_string(output.join(name).join("model.luau"))
                    .map_err(|e| e.to_string())?,
                source
            );
        }
        assert!(export(source, None, &output).is_err());
        fs::remove_dir_all(output).map_err(|e| e.to_string())?;
        Ok(())
    }

    #[test]
    fn geometry_changing_variants_remain_static_assets() -> Result<(), String> {
        let output =
            std::env::temp_dir().join(format!("spice-pose-geometry-{}", std::process::id()));
        let source = "cad.pose('small',{width=1}) cad.pose('large',{width=2}) local width=cad.number(1,{id='width'}) cad.show('body',cad.box({width=width,depth=3,height=4}))";
        export(source, None, &output)?;
        let manifest: Value = serde_json::from_slice(
            &fs::read(output.join("batch.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        assert!(manifest["animation_library"].is_null());
        assert!(!output.join("model.glb").exists());
        assert!(output.join("small/model.glb").is_file());
        assert!(output.join("large/model.glb").is_file());
        fs::remove_dir_all(output).map_err(|e| e.to_string())?;
        Ok(())
    }
}
