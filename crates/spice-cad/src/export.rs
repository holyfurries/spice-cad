use crate::{
    game_asset,
    kernel::{Body, Kernel},
    model,
};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Print,
    Game,
    GamePoses,
}

impl Target {
    pub fn description(self) -> &'static str {
        match self {
            Self::Print => "STEP, STL, print checks, and source",
            Self::Game => "GLB game asset and source",
            Self::GamePoses => "named pose GLBs and standard animation clips",
        }
    }
}

#[cfg(test)]
pub fn model_files(source: &str, folder: &Path, target: Target) -> Result<(), String> {
    model_files_project(source, None, folder, target)
}

pub fn model_files_project(
    source: &str,
    project: Option<&crate::project::Project>,
    folder: &Path,
    target: Target,
) -> Result<(), String> {
    if target == Target::GamePoses {
        return crate::pose_export::export(source, project, folder);
    }
    let model =
        model::compile_project_cached(source, project, &mut crate::analysis::Cache::default())?;
    let kernel = Kernel::new()?;
    let mut bodies = model.build(&kernel)?;
    let canonical_meshes = if target == Target::Print || model.has_annotations() {
        model.meshes(&mut bodies, false)?
    } else {
        Vec::new()
    };
    let meshes: Vec<_> = canonical_meshes.iter().collect();
    write_model(
        source,
        project,
        folder,
        target,
        &model,
        &mut bodies,
        &meshes,
    )
}

pub fn model_files_project_cached(
    source: &str,
    project: Option<&crate::project::Project>,
    folder: &Path,
    target: Target,
    cache: &mut model::Cache<'_>,
) -> Result<(), String> {
    if target == Target::GamePoses {
        return crate::pose_export::export_cached(source, project, folder, cache);
    }
    let model = cache.compile_project(source, project)?;
    let prepared = cache.prepare(&model)?;
    let mut bodies = prepared
        .bodies
        .iter()
        .map(|body| body.try_clone())
        .collect::<Result<Vec<_>, _>>()?;
    let meshes: Vec<_> = prepared
        .geometry
        .iter()
        .map(|geometry| &geometry.mesh)
        .collect();
    write_model(
        source,
        project,
        folder,
        target,
        &model,
        &mut bodies,
        &meshes,
    )
}

pub(crate) fn write_model(
    source: &str,
    project: Option<&crate::project::Project>,
    folder: &Path,
    target: Target,
    model: &model::Model,
    bodies: &mut [Body<'_>],
    canonical_meshes: &[&crate::mesh::Mesh],
) -> Result<(), String> {
    assert!(target != Target::GamePoses);
    model.annotations(canonical_meshes)?;
    let assembly_checks = if target == Target::Print {
        Some(crate::assembly_checks::check(model, bodies)?)
    } else {
        None
    };
    let layout = if target == Target::Print {
        crate::placement::Layout::Print
    } else {
        crate::placement::Layout::Assembly
    };
    if target == Target::Print {
        for (part, body) in model.parts.iter().zip(bodies.iter_mut()) {
            part.placement(layout).apply(body, false)?;
        }
    }
    let meshes = if target == Target::Game {
        model.meshes(bodies, true)?
    } else {
        canonical_meshes
            .iter()
            .zip(&model.parts)
            .map(|(mesh, part)| {
                mesh.placed(part.placement(layout).matrix(), part.material.base_color)
            })
            .collect()
    };
    let mut files = Vec::new();
    let sources: Vec<(String, std::borrow::Cow<'_, str>)> = project.map_or_else(
        || vec![("model.luau".to_owned(), std::borrow::Cow::Borrowed(source))],
        |project| {
            project
                .sources("model.luau")
                .into_iter()
                .map(|(name, source)| (name, std::borrow::Cow::Owned(source)))
                .collect()
        },
    );
    if target == Target::Game {
        files.push(("model.glb".into(), game_asset::glb(model, &meshes)?));
        files.push(("GAME_ASSET.md".into(), b"# Game asset\n\nImport model.glb as static meshes. Each named part is a separate object. Materials use the supported OpenPBR Surface 1.1.1 parameter subset. Base color, metalness and specular roughness map directly. Specular weight modulates IOR; effective IOR uses required KHR_materials_ior, and effective IOR 1 also requires KHR_materials_specular factor 0. Original supported parameters are preserved in materials[].extras.spice_cad as application metadata, not a standard interchange extension. Mapping is lossy: glTF commonly uses Schlick Fresnel rather than exact dielectric Fresnel, and compensation differs between renderers. The viewport also omits OpenPBR multiple-scattering compensation and layered diffuse-albedo normalization. No general rendered-appearance equivalence is claimed. Coordinates are metres, right-handed, Y up: CAD (x, y, z) becomes (x, z, -y) / 1000. Each mesh preserves local source coordinates and its source origin as the pivot; assembly placement is stored as node translation and quaternion rotation. Named transform groups and parent-child attachments are preserved as GLB nodes. By default no centering or bed placement is applied. cad.model_options can rotate the assembly without changing print placements. Configured game settings add a Game export root: selected model forward/up axes map to +Z/+Y, then positive uniform scale and optional lowest-point ground alignment are applied. Keep this root when importing; do not reapply its transforms. Tessellation uses 0.25 mm chord and 30 degree angular tolerances, with per-part cad.show shading: CAD-face smoothing by default, flat triangle normals, or shared-edge smoothing with a crease angle. UVs, textures, collision meshes, LODs, rigging, and animation are not generated. model.luau is the exact source snapshot.\n".to_vec()));
        let input_hashes = project.map_or_else(
            || vec![crate::game_contract::hash(source.as_bytes())],
            |project| {
                project
                    .original_sources()
                    .map(|source| crate::game_contract::hash(source.as_bytes()))
                    .collect()
            },
        );
        let (metadata, notes) =
            crate::game_contract::generate(&mut files[0].1, &sources, &input_hashes, model)?;
        files[1].1.extend_from_slice(notes.as_bytes());
        files.push(("asset.json".into(), metadata));
    } else {
        let print_bounds = bodies
            .iter()
            .map(|body| body.bounds(true))
            .collect::<Result<Vec<_>, _>>()?;
        let report = crate::print_preparation::report(
            model,
            canonical_meshes,
            &print_bounds,
            assembly_checks.expect("Print collision checks"),
        )?;
        files.push((
            "PRINT_CHECKS.json".into(),
            serde_json::to_vec_pretty(&report.json()).map_err(|error| error.to_string())?,
        ));
        files.push(("PRINT_CHECKS.md".into(), report.markdown().into_bytes()));
        let mut manifest = Vec::with_capacity(bodies.len());
        for (index, ((body, mesh), part)) in
            bodies.iter_mut().zip(&meshes).zip(&model.parts).enumerate()
        {
            let stem = if model.parts.len() == 1 {
                "model".into()
            } else {
                format!("part-{:02}", index + 1)
            };
            let [minimum, maximum] = body.bounds(true)?;
            if (maximum - minimum).max_element() > 256.0001 {
                return Err(format!(
                    "Part '{}': model exceeds the P1S 256 × 256 × 256 mm envelope; change the model or its orientation",
                    part.name
                ));
            }
            if minimum.z.abs() > 0.001 {
                return Err(format!(
                    "Part '{}': model must rest on Z=0; translate it onto the print bed before exporting",
                    part.name
                ));
            }
            let volume_before = body.volume()?;
            let step = body.step_data(&part.name)?;
            let volume_after = body.volume()?;
            if (volume_after - volume_before).abs() > volume_before.abs().mul_add(1e-5, 0.001) {
                return Err(format!(
                    "Part '{}': NURBS conversion changed the model volume; export cancelled",
                    part.name
                ));
            }
            let count =
                u32::try_from(mesh.vertices.len() / 3).map_err(|error| error.to_string())?;
            let mut stl = Vec::with_capacity(84 + count as usize * 50);
            let mut header = [0_u8; 80];
            header[..24].copy_from_slice(b"Spice CAD binary STL, mm");
            stl.extend_from_slice(&header);
            stl.extend_from_slice(&count.to_le_bytes());
            for triangle in mesh.vertices.as_chunks::<3>().0 {
                let points = triangle.map(|vertex| glam::Vec3::from_array(vertex.position));
                let normal = (points[1] - points[0])
                    .cross(points[2] - points[0])
                    .normalize_or_zero()
                    .to_array();
                for value in normal
                    .iter()
                    .chain(triangle.iter().flat_map(|vertex| vertex.position.iter()))
                {
                    stl.extend_from_slice(&value.to_le_bytes());
                }
                stl.extend_from_slice(&0_u16.to_le_bytes());
            }
            assert_eq!(stl.len(), 84 + count as usize * 50);
            manifest.push(serde_json::json!({"name": part.name, "step": format!("{stem}.step"), "stl": format!("{stem}.stl")}));
            files.push((format!("{stem}.step"), step.into_bytes()));
            files.push((format!("{stem}.stl"), stl));
        }
        files.push((
            "parts.json".into(),
            serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
        ));
        files.push(("PRINTING.md".into(), b"# Print model\n\nImport the STL files into Bambu Studio in millimetres. parts.json maps each source part name to its STEP/STL files. Each part is checked separately for bed placement, P1S envelope, and mesh closure; arrange multiple parts on plates in the slicer. Colors are not stored in STEP or STL. Select your P1S and actual filament, inspect orientation and supports, then slice. Strength, fit, and support requirements have not been physically verified. STEP files contain native SMLib geometry converted to NURBS, not triangle meshes. Some intersection curves may use the kernel's NURBS fitting tolerance.\n".to_vec()));
    }
    if let Some(parent) = folder.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir(folder).map_err(|error| {
        format!(
            "{}: {error}; choose a new export directory",
            folder.display()
        )
    })?;
    let result = (|| -> Result<(), std::io::Error> {
        for (name, data) in files {
            std::fs::write(folder.join(name), data)?;
        }
        for (name, source) in sources {
            std::fs::write(folder.join(name), source.as_bytes())?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(cleanup) = std::fs::remove_dir_all(folder) {
            return Err(format!("Export failed: {error}; cleanup failed: {cleanup}"));
        }
        return Err(format!("Export failed: {error}"));
    }
    Ok(())
}
