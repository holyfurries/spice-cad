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
}

impl Target {
    pub fn description(self) -> &'static str {
        match self {
            Self::Print => "STEP, STL, print checks, and source",
            Self::Game => "GLB game asset and source",
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

fn write_model(
    source: &str,
    project: Option<&crate::project::Project>,
    folder: &Path,
    target: Target,
    model: &model::Model,
    bodies: &mut [Body<'_>],
    canonical_meshes: &[&crate::mesh::Mesh],
) -> Result<(), String> {
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
    for (part, body) in model.parts.iter().zip(bodies.iter_mut()) {
        part.placement(layout).apply(body, false)?;
    }
    let meshes = if target == Target::Game {
        model.meshes(bodies, true)?
    } else {
        canonical_meshes
            .iter()
            .zip(&model.parts)
            .map(|(mesh, part)| mesh.placed(part.placement(layout).matrix(), part.color))
            .collect()
    };
    let mut files = Vec::new();
    if target == Target::Game {
        files.push(("model.glb".into(), game_asset::glb(&model.parts, &meshes)?));
        files.push(("GAME_ASSET.md".into(), b"# Game asset\n\nImport model.glb as static meshes. Each named part is a separate object with its own color material. Coordinates are metres, right-handed, Y up: CAD (x, y, z) becomes (x, z, -y) / 1000. All parts preserve the source origin as their pivot; no centering or bed placement is applied. Tessellation uses 0.25 mm chord and 30 degree angular tolerances, with smooth normals within CAD faces and hard boundaries between faces. UVs, textures, collision meshes, LODs, rigging, and animation are not generated. model.luau is the exact source snapshot.\n".to_vec()));
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
        if let Some(project) = project {
            for (name, source) in project.sources("model.luau") {
                std::fs::write(folder.join(name), source)?;
            }
        } else {
            std::fs::write(folder.join("model.luau"), source)?;
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
