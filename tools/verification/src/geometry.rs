use opencascade_sys::{g_prop, if_select, message, step_control};
use std::{error::Error, path::Path};

#[cxx::bridge]
mod bridge {
    unsafe extern "C++" {
        include!("BRepAlgo.hxx");
        include!("BRepGProp.hxx");
        type TopoDS_Shape = opencascade_sys::topo_ds::TopoDS_Shape;
        type GProp_GProps = opencascade_sys::g_prop::GProp_GProps;
        type BRepGProp;
        type BRepAlgo;
        #[Self = "BRepAlgo"]
        #[cxx_name = "IsValid"]
        fn is_valid(shape: &TopoDS_Shape) -> Result<bool>;
        #[Self = "BRepGProp"]
        #[cxx_name = "VolumePropertiesGK"]
        #[allow(clippy::too_many_arguments)]
        fn volume_properties(
            shape: &TopoDS_Shape,
            properties: Pin<&mut GProp_GProps>,
            tolerance: f64,
            only_closed: bool,
            use_spans: bool,
            center_of_mass: bool,
            inertia: bool,
            skip_shared: bool,
        ) -> Result<f64>;
    }
}

pub fn volume_mm3(path: &Path) -> Result<f64, Box<dyn Error>> {
    let mut reader = step_control::STEPControl_Reader_new();
    let status = step_control::read_step(reader.pin_mut(), path.to_string_lossy().into_owned());
    if status != if_select::IFSelect_ReturnStatus::IFSelect_RetDone {
        return Err(format!("Cannot read STEP {}: {status:?}", path.display()).into());
    }
    let roots = reader
        .pin_mut()
        .TransferRoots(&message::Message_ProgressRange_new());
    if roots <= 0 {
        return Err(format!("No transferable STEP roots: {}", path.display()).into());
    }
    let shape = step_control::one_shape_step(&reader);
    validate(&shape).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut properties = g_prop::GProps_new();
    let error_relative = bridge::BRepGProp::volume_properties(
        &shape,
        properties.pin_mut(),
        1e-9,
        true,
        true,
        false,
        false,
        false,
    )?;
    if !error_relative.is_finite() || error_relative < 0.0 {
        return Err(format!(
            "STEP volume integration failed: {} ({error_relative})",
            path.display()
        )
        .into());
    }
    if error_relative > 1e-8 {
        return Err(format!(
            "STEP volume integration did not converge: {} ({error_relative})",
            path.display()
        )
        .into());
    }
    let volume = properties.Mass();
    if !volume.is_finite() || volume <= 0.0 {
        return Err(format!("Invalid STEP volume {volume}: {}", path.display()).into());
    }
    Ok(volume)
}

pub fn validate(shape: &opencascade_sys::topo_ds::TopoDS_Shape) -> Result<(), Box<dyn Error>> {
    if !bridge::BRepAlgo::is_valid(shape)? {
        return Err("Invalid STEP B-rep".into());
    }
    Ok(())
}
