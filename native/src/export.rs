use crate::{kernel::Kernel, model};
use std::path::Path;

pub fn model_files(source: &str, folder: &Path) -> Result<(), String> {
    let model = model::compile(source)?;
    let kernel = Kernel::new()?;
    let mut body = model.build(&kernel)?;
    let [minimum, maximum] = body.bounds(true)?;
    if (maximum - minimum).max_element() > 256.0001 {
        return Err("Model exceeds the P1S 256 × 256 × 256 mm envelope; change the model or its orientation".into());
    }
    if minimum.z.abs() > 0.001 {
        return Err(
            "Model must rest on Z=0; translate it onto the print bed before exporting".into(),
        );
    }
    let mesh = body.mesh()?;
    let volume_before = body.volume()?;
    let step = body.step_data(&model.name)?;
    let volume_after = body.volume()?;
    if (volume_after - volume_before).abs() > volume_before.abs().mul_add(1e-5, 0.001) {
        return Err("NURBS conversion changed the model volume; export cancelled".into());
    }
    let count = u32::try_from(mesh.vertices.len() / 3).map_err(|error| error.to_string())?;
    let mut stl = Vec::with_capacity(84 + count as usize * 50);
    let mut header = [0_u8; 80];
    header[..24].copy_from_slice(b"Spice CAD binary STL, mm");
    stl.extend_from_slice(&header);
    stl.extend_from_slice(&count.to_le_bytes());
    for triangle in mesh.vertices.as_chunks::<3>().0 {
        for value in triangle[0]
            .normal
            .iter()
            .chain(triangle.iter().flat_map(|vertex| vertex.position.iter()))
        {
            stl.extend_from_slice(&value.to_le_bytes());
        }
        stl.extend_from_slice(&0_u16.to_le_bytes());
    }
    assert_eq!(stl.len(), 84 + count as usize * 50);
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
        std::fs::write(folder.join("model.step"), step)?;
        std::fs::write(folder.join("model.stl"), stl)?;
        std::fs::write(folder.join("model.luau"), source)?;
        std::fs::write(
            folder.join("PRINTING.md"),
            "# Print model\n\nImport model.stl into Bambu Studio in millimetres. Select your P1S and actual filament, inspect orientation and supports, then slice. Bed fit and mesh closure were checked; strength, fit, and support requirements have not been physically verified. model.step contains native SMLib geometry converted to NURBS, not a triangle mesh. Some intersection curves may use the kernel's NURBS fitting tolerance.\n",
        )?;
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
