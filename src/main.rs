use spice_rack::{CABINET_DIMENSIONS_MM, PARAMETERS, printing};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let layout = PARAMETERS.layout()?;
    let parts = printing::build_parts(PARAMETERS)?;
    let samples = printing::build_fit_samples(PARAMETERS.thickness_mm)?;
    let shape = printing::assemble_parts(&parts);
    std::fs::create_dir_all("output")?;
    shape.write_step("output/spice-rack.step")?;
    for part in parts.iter().chain(samples.iter()) {
        let (print_shape, dimensions_mm) = part.on_bed()?;
        print_shape.write_step(format!("output/spice-rack-{}.step", part.name))?;
        print_shape
            .write_stl_with_tolerance(format!("output/spice-rack-{}.stl", part.name), 0.1)?;
        println!(
            "Part: {}, {:.1} x {:.1} x {:.1} mm on the print bed",
            part.name, dimensions_mm.x, dimensions_mm.y, dimensions_mm.z
        );
    }
    match std::fs::remove_file("output/spice-rack.stl") {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    println!(
        "Rack: {:.1} x {:.1} x {:.1} mm (width x depth x height), {} steps, {} largest jars per step",
        layout.dimensions_mm.x,
        layout.dimensions_mm.y,
        layout.dimensions_mm.z,
        PARAMETERS.step_count,
        layout.jars_per_step_count,
    );
    let loaded_height_mm =
        layout.dimensions_mm.z - PARAMETERS.lip_height_mm + PARAMETERS.jar_height_mm;
    println!(
        "Loaded height: {loaded_height_mm:.1} mm; ceiling gap: {:.1} mm",
        CABINET_DIMENSIONS_MM.z - loaded_height_mm
    );
    println!(
        "Jar height limit: {:.1} mm, including space to lift over {:.1} mm lips",
        layout.jar_height_mm_max, PARAMETERS.lip_height_mm
    );
    println!(
        "Sauces section: {:.1} mm remaining before gaps or a divider",
        CABINET_DIMENSIONS_MM.x - layout.dimensions_mm.x
    );
    println!(
        "Joints: {:.2} mm clearance; print fit samples before the four rack parts",
        printing::JOINT_CLEARANCE_MM
    );
    println!(
        "Export: output/spice-rack.step is the assembled preview; use the named part STLs to print"
    );
    Ok(())
}
