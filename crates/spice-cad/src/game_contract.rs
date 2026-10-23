use glam::{DMat4, DQuat, DVec3};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const MATERIAL_WARNING: &str = "Zero-specular materials require KHR_materials_specular. Bevy 0.19.1 with unpatched gltf-json 1.4.1 rejects this required extension; use a corrected validator, not a validation bypass.";

pub struct Reference {
    pub name: String,
    pub parts: Vec<String>,
}

pub fn register_references(
    lua: &mlua::Lua,
    cad: &mlua::Table,
) -> mlua::Result<std::rc::Rc<std::cell::RefCell<Vec<Reference>>>> {
    let state = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Reference>::new()));
    let target = std::rc::Rc::clone(&state);
    cad.set(
        "reference",
        lua.create_function(move |_, (name, options): (String, mlua::Table)| {
            crate::model::options_fields(&options, &["parts"])?;
            let mut state = target.borrow_mut();
            if !crate::poses::identifier(&name)
                || state.len() >= 16
                || state.iter().any(|reference| reference.name == name)
            {
                return Err(mlua::Error::runtime(
                    "References need unique portable identifiers; at most 16 are supported",
                ));
            }
            let table: mlua::Table = options.get("parts")?;
            let mut parts = Vec::new();
            for entry in table.pairs::<usize, String>() {
                let (index, part) = entry?;
                if index == 0
                    || index > 32
                    || parts.len() >= 32
                    || part.is_empty()
                    || part.len() > 80
                    || parts.contains(&part)
                {
                    return Err(mlua::Error::runtime(
                        "References need 1–32 distinct part names in an array",
                    ));
                }
                parts.push(part);
            }
            if parts.is_empty() || table.raw_len() != parts.len() {
                return Err(mlua::Error::runtime("Reference cannot be empty"));
            }
            parts.sort();
            state.push(Reference { name, parts });
            Ok(())
        })?,
    )?;
    Ok(state)
}

#[derive(Default)]
pub struct Asset {
    id: Option<String>,
    author: Option<String>,
    source: Option<String>,
    license: Option<String>,
    approval: Option<String>,
    display_width: Option<f64>,
}

pub fn register(
    lua: &mlua::Lua,
    cad: &mlua::Table,
) -> mlua::Result<std::rc::Rc<std::cell::RefCell<Option<Asset>>>> {
    let state = std::rc::Rc::new(std::cell::RefCell::new(None));
    let target = std::rc::Rc::clone(&state);
    cad.set("asset", lua.create_function(move |_, options: mlua::Table| {
        crate::model::options_fields(&options, &["id", "author", "source", "license", "approval", "display_width"])?;
        let text = |key: &str| -> mlua::Result<Option<String>> {
            let value: Option<String> = options.get(key)?;
            if value.as_ref().is_some_and(|text| text.is_empty() || text.len() > 512 || text.chars().any(char::is_control)) {
                return Err(mlua::Error::runtime(format!("Asset {key} must contain 1–512 bytes without control characters")));
            }
            Ok(value)
        };
        let approval = text("approval")?;
        if approval.as_deref().is_some_and(|value| !["unreviewed", "trial", "approved"].contains(&value)) {
            return Err(mlua::Error::runtime("Asset approval must be unreviewed, trial, or approved"));
        }
        let display_width: Option<f64> = options.get("display_width")?;
        if display_width.is_some_and(|value| !value.is_finite() || !(0.000001..=1000000.0).contains(&value)) {
            return Err(mlua::Error::runtime("Asset display_width must be finite, positive, and between 0.000001 and 1000000 metres"));
        }
        let asset = Asset { id: text("id")?, author: text("author")?, source: text("source")?, license: text("license")?, approval, display_width };
        let mut target = target.borrow_mut();
        if target.is_some() { return Err(mlua::Error::runtime("Declare cad.asset only once per model")); }
        *target = Some(asset);
        Ok(())
    })?)?;
    Ok(state)
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn generate(
    glb: &mut Vec<u8>,
    sources: &[(String, std::borrow::Cow<'_, str>)],
    input_hashes: &[String],
    model: &crate::model::Model,
) -> Result<(Vec<u8>, String), String> {
    let options = model.options;
    let asset = &model.asset;
    assert!(!sources.is_empty());
    assert_eq!(sources.len(), input_hashes.len());
    let json_length = u32::from_le_bytes(
        glb.get(12..16)
            .ok_or("Missing GLB header")?
            .try_into()
            .map_err(|_| "Invalid GLB JSON length")?,
    ) as usize;
    let mut document: Value =
        serde_json::from_slice(glb.get(20..20 + json_length).ok_or("Missing GLB JSON")?)
            .map_err(|error| error.to_string())?;
    let binary = glb.get(28 + json_length..).ok_or("Missing GLB binary")?;
    let nodes = document["nodes"].as_array().ok_or("Missing GLB nodes")?;
    let materials = document["materials"]
        .as_array()
        .ok_or("Missing GLB materials")?;
    if nodes.len() > 65 {
        return Err("Game contract exceeds node budget".into());
    }
    let mut parents = vec![None; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        if let Some(children) = node["children"].as_array() {
            for child in children {
                let child = number(child)?;
                let parent = parents.get_mut(child).ok_or("Invalid GLB child")?;
                if parent.replace(index).is_some() {
                    return Err("GLB node has multiple parents".into());
                }
            }
        }
    }
    let root = nodes
        .iter()
        .position(|node| node["extras"]["spice_cad"]["preset"].is_string());
    let mut worlds = vec![None; nodes.len()];
    let mut paths: Vec<Option<Vec<String>>> = vec![None; nodes.len()];
    for _ in 0..nodes.len() {
        for (index, node) in nodes.iter().enumerate() {
            if worlds[index].is_some() {
                continue;
            }
            let parent_world = match parents[index] {
                Some(parent) => match worlds[parent] {
                    Some(world) => world,
                    None => continue,
                },
                None => DMat4::IDENTITY,
            };
            worlds[index] = Some(parent_world * transform(node)?);
            let mut path = parents[index]
                .and_then(|parent| paths[parent].clone())
                .unwrap_or_default();
            if Some(index) != root {
                path.push(format!(
                    "{}:{}",
                    if node["mesh"].is_number() {
                        "part"
                    } else {
                        "frame"
                    },
                    node["name"].as_str().ok_or("Unnamed game node")?
                ));
            }
            paths[index] = Some(path);
        }
        if worlds.iter().all(Option::is_some) {
            break;
        }
    }
    let mut minimum = DVec3::splat(f64::INFINITY);
    let mut maximum = DVec3::splat(f64::NEG_INFINITY);
    let mut triangle_count = 0;
    let mut vertex_count = 0;
    let mut records = Vec::with_capacity(nodes.len());
    let mut identity = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        let world = worlds[index].ok_or("Cyclic game hierarchy")?;
        let path = paths[index].as_ref().ok_or("Missing node identity")?;
        let id = if Some(index) == root {
            "export-root".to_owned()
        } else {
            hash(&serde_json::to_vec(path).map_err(|e| e.to_string())?)
        };
        if Some(index) != root {
            identity.push(id.clone());
        }
        let mut part_minimum = DVec3::splat(f64::INFINITY);
        let mut part_maximum = DVec3::splat(f64::NEG_INFINITY);
        if let Some(mesh) = node["mesh"].as_u64() {
            let primitive = &document["meshes"][mesh as usize]["primitives"][0];
            let accessor = &document["accessors"][number(&primitive["attributes"]["POSITION"])?];
            let view = &document["bufferViews"][number(&accessor["bufferView"])?];
            let count = number(&accessor["count"])?;
            if count > 300000 {
                return Err("Game contract exceeds vertex budget".into());
            }
            let start = number(&view["byteOffset"])?
                + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
            let stride = number(&view["byteStride"])?;
            if stride != 24 {
                return Err("Unexpected game vertex stride".into());
            }
            for vertex in 0..count {
                let offset = start + vertex * stride;
                let bytes = binary
                    .get(offset..offset + 12)
                    .ok_or("Truncated game vertex")?;
                let mut p = [0.0; 3];
                for (axis, value) in p.iter_mut().enumerate() {
                    *value = f32::from_le_bytes(
                        bytes[axis * 4..axis * 4 + 4]
                            .try_into()
                            .map_err(|_| "Invalid vertex")?,
                    ) as f64;
                }
                let p = world.transform_point3(DVec3::from_array(p));
                if !p.is_finite() {
                    return Err("Non-finite game bounds".into());
                }
                minimum = minimum.min(p);
                maximum = maximum.max(p);
                part_minimum = part_minimum.min(p);
                part_maximum = part_maximum.max(p);
            }
            vertex_count += count;
            triangle_count +=
                number(&document["accessors"][number(&primitive["indices"])?]["count"])? / 3;
        }
        records.push(json!({"id":id,"node":index,"name":node["name"],"path":path,
            "kind":if Some(index)==root {"export_root"} else if node["mesh"].is_number() {"part"} else {"transform_frame"},
            "attachment_role":node["extras"]["spice_cad"]["attachment_role"],"parent_node":parents[index],
            "bounds":if part_minimum.is_finite() {json!({"minimum":part_minimum.to_array(),"maximum":part_maximum.to_array()})} else {Value::Null},
            "local_transform":transform(node)?.to_cols_array(),"world_transform":world.to_cols_array()}));
    }
    if !minimum.is_finite() {
        return Err("Empty game bounds".into());
    }
    let mut references = serde_json::Map::new();
    for reference in &model.references {
        let mut minimum = DVec3::splat(f64::INFINITY);
        let mut maximum = DVec3::splat(f64::NEG_INFINITY);
        let mut members = Vec::with_capacity(reference.parts.len());
        for name in &reference.parts {
            let mut found = records
                .iter()
                .filter(|record| record["kind"] == "part" && record["name"] == *name);
            let record = found.next().ok_or_else(|| {
                format!("Reference '{}' has missing part '{name}'", reference.name)
            })?;
            if found.next().is_some() {
                return Err(format!("Ambiguous reference part: {name}"));
            }
            let vector = |key: &str| -> Result<DVec3, String> {
                let mut values = [0.0; 3];
                for (axis, value) in values.iter_mut().enumerate() {
                    *value = record["bounds"][key][axis]
                        .as_f64()
                        .ok_or("Missing reference bounds")?;
                }
                Ok(DVec3::from_array(values))
            };
            minimum = minimum.min(vector("minimum")?);
            maximum = maximum.max(vector("maximum")?);
            members.push(record["node"].clone());
        }
        references.insert(reference.name.clone(), json!({"space":"exported world","units":"metres","nodes":members,"minimum":minimum.to_array(),"maximum":maximum.to_array(),"size":(maximum-minimum).to_array()}));
    }
    identity.sort();
    let asset_id = asset.id.clone().unwrap_or(hash(
        &serde_json::to_vec(&identity).map_err(|e| e.to_string())?,
    ));
    let source_records: Vec<_> = sources.iter().zip(input_hashes).map(|((path, bundled), original)| {
        json!({"path":path,"sha256":hash(bundled.as_bytes()),"input_sha256":original})
    }).collect();
    let source_hash = hash(&serde_json::to_vec(&source_records).map_err(|e| e.to_string())?);
    let required = document["extensionsRequired"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let warning = required
        .iter()
        .any(|value| value == "KHR_materials_specular");
    let size = maximum - minimum;
    if size.x <= 0.0 {
        return Err("Game asset needs positive width".into());
    }
    let additional_scale = asset.display_width.map_or(1.0, |width| {
        let scale = width / size.x;
        if (scale - 1.0).abs() < 1e-6 {
            1.0
        } else {
            scale
        }
    });
    let root_transform = root
        .map(|index| &nodes[index])
        .cloned()
        .unwrap_or_else(|| json!({"translation":[0,0,0],"rotation":[0,0,0,1],"scale":[1,1,1]}));
    let material_records: Vec<_> = materials
        .iter()
        .enumerate()
        .map(|(index, material)| {
            let mut users: Vec<_> = nodes
                .iter()
                .enumerate()
                .filter_map(|(node_index, node)| {
                    let mesh = node["mesh"].as_u64()? as usize;
                    (document["meshes"][mesh]["primitives"][0]["material"].as_u64()? as usize
                        == index)
                        .then(|| records[node_index]["id"].clone())
                })
                .collect();
            users.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            json!({"id":hash(json!([material["name"], users]).to_string().as_bytes()),
            "material":index,"name":material["name"],"openpbr":material["extras"]["spice_cad"]})
        })
        .collect();
    let mut metadata = json!({
        "schema":"https://holyfurries.com/spice-cad/game-asset","schema_version":1,
        "asset_id":asset_id,"identity_basis":if asset.id.is_some() {"authored cad.asset id"} else {"sorted semantic hierarchy paths; renaming or reparenting changes identity"},
        "files":{"glb":{"path":"model.glb","sha256":hash(glb)},"sources":source_records,"source_bundle_sha256":source_hash},
        "exporter":{"name":"spice-cad","version":env!("CARGO_PKG_VERSION"),"revision":option_env!("SPICE_BUILD_REVISION"),
            "source_dirty":option_env!("SPICE_BUILD_DIRTY").map(|value| value == "true"),"build_inputs_sha256":env!("SPICE_BUILD_INPUTS_SHA256")},
        "coordinates":{"units":"metres","handedness":"right","up":"y+","forward":"z+",
            "source_units":"millimetres","source_up":"z+","source_to_gltf":"(x,z,-y)/1000",
            "model_rotation_degrees_xyz":options.rotation,"selected_model_forward":options.game.forward.name(),"selected_model_up":options.game.up.name(),
            "selected_axes_space":"assembled CAD axes after model rotation","preset":if options.game.crowns_adrift {"crowns_adrift"} else {"gltf"},
            "root":{"node":root,"translation":root_transform["translation"],"rotation_xyzw":root_transform["rotation"],"scale":root_transform["scale"]},
            "transforms_already_applied":true,"import_wrapper":"identity; retain all GLB hierarchy transforms"},
        "bounds":{"space":"exported world","minimum":minimum.to_array(),"maximum":maximum.to_array(),"size":size.to_array(),
            "ground_anchor":[0.0,minimum.y,0.0],"grounding_applied":options.declared && options.game.ground},
        "display":{"exported_dimensions":size.to_array(),"intended_dimensions":asset.display_width.map(|_| (size*additional_scale).to_array()),"gameplay_tile_size":null,"additional_scale":additional_scale,
            "approval":asset.approval.as_deref().unwrap_or("unreviewed"),"author":asset.author,"source":asset.source,"license":asset.license},
        "counts":{"parts":document["meshes"].as_array().map_or(0,Vec::len),"materials":materials.len(),"triangles":triangle_count,"gpu_vertices":vertex_count},
        "features":{"uvs":false,"textures":false,"skeletons":false,"animation_clips":false,
            "transform_frames":nodes.iter().enumerate().filter(|(index,node)| Some(*index)!=root && node["mesh"].is_null()).count(),
            "attachment_frames":nodes.iter().filter(|node| node["extras"]["spice_cad"]["attachment_role"].is_string()).count()},
        "material_profile":"openpbr-1.1.1-supported-subset-to-gltf-pbr","extensions_required":required,
        "compatibility":{"target_loader_validation":"not_run","warnings":if warning {vec![MATERIAL_WARNING]} else {vec![]}},
        "nodes":records,"materials":material_records,"references":references,"pose":model.pose,
        "poses":model.poses.iter().map(|pose| json!({"name":pose.name,"parameters":pose.parameters})).collect::<Vec<_>>()
    });
    document["extras"]["spice_cad"] = json!({"schema_version":1,"asset_id":asset_id,"references":metadata["references"],"pose":model.pose,"poses":metadata["poses"]});
    for record in &records {
        let index = record["node"].as_u64().ok_or("Missing node index")? as usize;
        document["nodes"][index]["extras"]["spice_cad"]["id"] = record["id"].clone();
    }
    let mut packet = json!({"@context":{"dc":"http://purl.org/dc/elements/1.1/","rdf":"http://www.w3.org/1999/02/22-rdf-syntax-ns#"},"dc:identifier":asset_id});
    if let Some(author) = &asset.author {
        packet["dc:creator"] = json!({"@list":[author]});
    }
    if let Some(source) = &asset.source {
        packet["dc:source"] = json!(source);
    }
    if let Some(license) = &asset.license {
        packet["dc:rights"] =
            json!({"@type":"rdf:Alt","rdf:_1":{"@language":"x-default","@value":license}});
    }
    document["extensions"]["KHR_xmp_json_ld"] = json!({"packets":[packet]});
    document["asset"]["extensions"]["KHR_xmp_json_ld"] = json!({"packet":0});
    if document["extensionsUsed"].is_null() {
        document["extensionsUsed"] = json!([]);
    }
    document["extensionsUsed"]
        .as_array_mut()
        .ok_or("Invalid extension list")?
        .push(json!("KHR_xmp_json_ld"));
    *glb = crate::game_asset::encode(&document, binary)?;
    metadata["files"]["glb"]["sha256"] = json!(hash(glb));
    let notes = format!(
        "\n## This export\n\nAsset: `{asset_id}`. Preset: `{}`. Model forward/up: `{}` / `{}` (assembled CAD axes after model rotation). Model rotation XYZ: {:?} degrees.\n\nFinal world dimensions: {:.9} × {:.9} × {:.9} metres. Game scale: {}. Grounding: {}. Preserve the GLB hierarchy: all stored transforms are already applied. An identity import wrapper retains these exported dimensions. Gameplay tile size and art approval are not inferred.\n\nAdditional uniform display scale: {additional_scale:.6}. This is separate from the baked export-root transform and uses the authored display width when present; without a declared width it is 1.\n\n`asset.json` records exact transforms, bounds, source and GLB SHA-256 hashes, semantic identities, and material requirements. Target-loader validation has not been run by this export.{}\n",
        if options.game.crowns_adrift {
            "crowns_adrift"
        } else {
            "gltf"
        },
        options.game.forward.name(),
        options.game.up.name(),
        options.rotation,
        size.x,
        size.y,
        size.z,
        options.game.scale,
        options.declared && options.game.ground,
        if warning {
            format!("\n\nCompatibility warning: {MATERIAL_WARNING}")
        } else {
            String::new()
        }
    );
    Ok((
        serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
        notes,
    ))
}

fn number(value: &Value) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| "Missing game contract integer".into())
}

fn transform(node: &Value) -> Result<DMat4, String> {
    let vector = |key: &str, default: [f64; 3]| -> Result<DVec3, String> {
        let Some(values) = node[key].as_array() else {
            return Ok(DVec3::from_array(default));
        };
        if values.len() != 3 {
            return Err("Invalid game transform vector".into());
        }
        let mut result = [0.0; 3];
        for (target, value) in result.iter_mut().zip(values) {
            *target = value.as_f64().ok_or("Invalid transform number")?;
        }
        Ok(DVec3::from_array(result))
    };
    let rotation = if let Some(values) = node["rotation"].as_array() {
        if values.len() != 4 {
            return Err("Invalid game quaternion".into());
        }
        let mut result = [0.0; 4];
        for (target, value) in result.iter_mut().zip(values) {
            *target = value.as_f64().ok_or("Invalid quaternion number")?;
        }
        DQuat::from_array(result)
    } else {
        DQuat::IDENTITY
    };
    Ok(DMat4::from_scale_rotation_translation(
        vector("scale", [1.0; 3])?,
        rotation,
        vector("translation", [0.0; 3])?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(source: &str) -> Result<(Value, Vec<u8>), String> {
        let model = crate::model::compile(source)?;
        let kernel = crate::kernel::Kernel::new()?;
        let meshes = model.meshes(&mut model.build(&kernel)?, true)?;
        let mut glb = crate::game_asset::glb(&model, &meshes)?;
        let sources = vec![("model.luau".to_owned(), std::borrow::Cow::Borrowed(source))];
        let (bytes, _) = generate(&mut glb, &sources, &[hash(source.as_bytes())], &model)?;
        Ok((
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
            glb,
        ))
    }

    #[test]
    fn named_references_measure_exported_geometry_and_embed_provenance() -> Result<(), String> {
        let source = "cad.asset({id='test/body',author='Studio',license='MIT'}) cad.model_options({game={scale=2}}) local frame=cad.group('frame',{assembly={position={x=5,z=7},rotation={z=90}}}) cad.show('body',cad.box({width=2,depth=3,height=4}),{parent=frame}) cad.show('weapon',cad.box({width=1,depth=1,height=100})) cad.reference('body',{parts={'body'}})";
        let (metadata, bytes) = contract(source)?;
        let reference = &metadata["references"]["body"];
        for (axis, expected) in [0.006, 0.008, 0.004].into_iter().enumerate() {
            assert!((reference["size"][axis].as_f64().unwrap() - expected).abs() < 1e-8);
        }
        assert!(metadata["bounds"]["size"][1].as_f64().unwrap() > 0.19);
        let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let document: Value =
            serde_json::from_slice(&bytes[20..20 + length]).map_err(|e| e.to_string())?;
        assert_eq!(
            document["extras"]["spice_cad"]["references"],
            metadata["references"]
        );
        assert_eq!(
            document["extensions"]["KHR_xmp_json_ld"]["packets"][0]["dc:creator"]["@list"][0],
            "Studio"
        );
        assert_eq!(metadata["files"]["glb"]["sha256"], hash(&bytes));
        assert!(contract(&source.replace("parts={'body'}", "parts={'missing'}")).is_err());
        Ok(())
    }

    #[test]
    fn game_contract_reports_actual_scaled_bounds_and_stable_semantic_identity()
    -> Result<(), String> {
        let geometry = "local root=cad.group('Body',{assembly={position={z=7}}});local grip=cad.group('Grip',{parent=root,attachment='weapon_grip',assembly={position={x=4,z=3},rotation={z=90}}});cad.show('blade',cad.box({width=2,depth=4,height=10}),{parent=grip})";
        let (original, original_glb) = contract(geometry)?;
        let configured =
            format!("cad.model_options({{game={{preset='crowns_adrift',scale=10}}}});{geometry}");
        let (scaled, scaled_glb) = contract(&configured)?;
        assert_eq!(original["asset_id"], scaled["asset_id"]);
        assert_ne!(
            original["files"]["source_bundle_sha256"],
            scaled["files"]["source_bundle_sha256"]
        );
        assert_eq!(scaled["files"]["glb"]["sha256"], hash(&scaled_glb));
        assert_ne!(hash(&original_glb), hash(&scaled_glb));
        assert_eq!(
            scaled["coordinates"]["root"]["scale"],
            json!([10.0, 10.0, 10.0])
        );
        assert!(scaled["bounds"]["minimum"][1].as_f64().unwrap().abs() < 1e-7);
        for axis in 0..3 {
            let expected = original["bounds"]["size"][axis].as_f64().unwrap() * 10.0;
            assert!((scaled["bounds"]["size"][axis].as_f64().unwrap() - expected).abs() < 1e-7);
        }
        assert_eq!(scaled["counts"]["parts"], 1);
        assert_eq!(scaled["counts"]["triangles"], 12);
        assert_eq!(scaled["counts"]["gpu_vertices"], 24);
        assert_eq!(scaled["features"]["transform_frames"], 2);
        assert_eq!(scaled["features"]["attachment_frames"], 1);
        assert_eq!(scaled["nodes"][2]["attachment_role"], "weapon_grip");
        assert_eq!(original["nodes"][0]["id"], scaled["nodes"][0]["id"]);
        assert_eq!(
            original["nodes"][0]["local_transform"],
            scaled["nodes"][0]["local_transform"]
        );
        assert_eq!(original["materials"][0]["id"], scaled["materials"][0]["id"]);
        assert_eq!(
            scaled["compatibility"]["target_loader_validation"],
            "not_run"
        );
        let (repeated, repeated_glb) = contract(&configured)?;
        assert_eq!(scaled_glb, repeated_glb);
        assert_eq!(scaled, repeated);
        let (edited, _) = contract(&configured.replace("height=10", "height=12"))?;
        assert_eq!(scaled["asset_id"], edited["asset_id"]);
        assert_eq!(scaled["nodes"][0]["id"], edited["nodes"][0]["id"]);
        assert_ne!(
            scaled["files"]["source_bundle_sha256"],
            edited["files"]["source_bundle_sha256"]
        );
        assert_ne!(
            scaled["files"]["glb"]["sha256"],
            edited["files"]["glb"]["sha256"]
        );
        Ok(())
    }

    #[test]
    fn game_contract_records_farmstead_material_requirements_without_altering_export()
    -> Result<(), String> {
        let source = include_str!("../../../examples/farmstead.luau");
        let (metadata, _) = contract(source)?;
        assert_eq!(metadata["counts"]["triangles"], 44);
        assert_eq!(metadata["counts"]["gpu_vertices"], 90);
        assert_eq!(
            metadata["extensions_required"],
            json!(["KHR_materials_ior", "KHR_materials_specular"])
        );
        assert_eq!(metadata["compatibility"]["warnings"][0], MATERIAL_WARNING);
        for (axis, expected) in [0.115, 0.1155, 0.100].into_iter().enumerate() {
            assert!((metadata["bounds"]["size"][axis].as_f64().unwrap() - expected).abs() < 1e-7);
        }
        assert_eq!(metadata["display"]["approval"], "trial");
        assert_eq!(metadata["asset_id"], "crowns-adrift/farmstead");
        assert_eq!(metadata["display"]["license"], "MIT");
        assert!((metadata["display"]["additional_scale"].as_f64().unwrap() - 10.0).abs() < 1e-5);
        let source =
            format!("cad.model_options({{game={{preset='crowns_adrift',scale=10}}}});{source}");
        let (configured, _) = contract(&source)?;
        assert_eq!(configured["asset_id"], metadata["asset_id"]);
        assert_eq!(configured["display"]["additional_scale"], 1.0);
        Ok(())
    }

    #[test]
    fn asset_contract_rejects_invalid_authored_metadata() {
        for declaration in [
            "{id=''}",
            "{approval='shipped'}",
            "{display_width=0}",
            "{display_width=0/0}",
            "{other=1}",
            "{source='line\\nbreak'}",
        ] {
            assert!(
                crate::model::compile(&format!(
                    "cad.asset({declaration});cad.show('box',cad.box({{width=1,depth=1,height=1}}))"
                ))
                .is_err()
            );
        }
        assert!(
            crate::model::compile(
                "cad.asset({});cad.asset({});cad.show('box',cad.box({width=1,depth=1,height=1}))"
            )
            .is_err()
        );
    }
}
