use super::*;

fn add_lines(
    points: &[[f64; 3]],
    closed: bool,
    curves: &mut UniquePtr<ffi::SmCurveArray>,
    lines: &mut Vec<UniquePtr<ffi::SmLine>>,
) -> Result<(), String> {
    assert!(points.len() >= 2);
    assert!(points.len() <= 128);
    for index in 0..points.len() - usize::from(!closed) {
        let vector = |point: [f64; 3]| Vector {
            x: point[0],
            y: point[1],
            z: point[2],
        };
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            ffi::SmApiCreateLineSegment(
                &vector(points[index]),
                &vector(points[(index + 1) % points.len()]),
                &mut result,
            )
        };
        let line = unsafe { UniquePtr::from_raw(result) };
        let status = status.map_err(|error| error.to_string())?;
        if status != 1000 || line.is_null() {
            return Err(format!("SMLib shape edge failed: {status}"));
        }
        unsafe { curves.pin_mut().Add(line.as_ptr().cast_mut().cast()) }
            .map_err(|error| error.to_string())?;
        lines.push(line);
    }
    Ok(())
}

impl Kernel {
    pub fn path_sweep(
        &self,
        polygon: &crate::profile::Polygon,
        path: &[[f64; 3]],
    ) -> Result<Body<'_>, String> {
        let mut lines = Vec::with_capacity(polygon.points.len() + path.len());
        let mut profile = ffi::spice_curve_array().map_err(|error| error.to_string())?;
        let mut route = ffi::spice_curve_array().map_err(|error| error.to_string())?;
        let points: Vec<_> = polygon.points.iter().map(|&[x, y]| [x, y, 0.0]).collect();
        add_lines(&points, true, &mut profile, &mut lines)?;
        add_lines(path, false, &mut route, &mut lines)?;
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            ffi::SmApiCreateSweepAlongPlanarPath(
                profile.pin_mut(),
                route.pin_mut(),
                1,
                3,
                &mut result,
            )
        };
        self.shape_result(result, status, "sweep")
    }

    pub fn loft(&self, sections: &[crate::shape_operation::Section]) -> Result<Body<'_>, String> {
        assert!((2..=16).contains(&sections.len()));
        let mut curves = ffi::spice_curve_array().map_err(|error| error.to_string())?;
        let mut lines = Vec::with_capacity(sections.len() * sections[0].polygon.points.len());
        for section in sections {
            let [x, y, z] = section.position;
            let points: Vec<_> = section
                .polygon
                .points
                .iter()
                .map(|point| [point[0] + x, point[1] + y, z])
                .collect();
            add_lines(&points, true, &mut curves, &mut lines)?;
        }
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            ffi::spice_loft(
                &curves,
                sections.len(),
                sections[0].polygon.points.len(),
                &mut result,
            )
        };
        self.shape_result(result, status, "loft")
    }

    fn shape_result(
        &self,
        result: *mut ffi::SmBrep,
        status: Result<i64, cxx::Exception>,
        name: &str,
    ) -> Result<Body<'_>, String> {
        let shape = unsafe { UniquePtr::from_raw(result) };
        let status = status.map_err(|error| error.to_string())?;
        if status != 1000 || shape.is_null() {
            return Err(format!("SMLib {name} failed: {status}"));
        }
        if shape.IsManifoldSolid() != 1 {
            return Err(format!(
                "SMLib {name} did not produce a closed manifold solid"
            ));
        }
        let body = Body {
            shape,
            volume_mm3: Cell::new(None),
            #[cfg(test)]
            volume_integrations: Cell::new(0),
            _kernel: self,
        };
        if body.volume()? <= 1e-8 {
            return Err(format!("SMLib {name} produced no positive material volume"));
        }
        Ok(body)
    }
}

impl Body<'_> {
    pub fn shell(
        &mut self,
        thickness: f64,
        opening: Option<&crate::dimensions::Selector>,
    ) -> Result<(), String> {
        assert!(thickness.is_finite());
        assert!(thickness > 0.0);
        let face = if let Some(selector) = opening {
            self.planar_feature_face(selector)?.0
        } else {
            std::ptr::null_mut()
        };
        let bounds = self.bounds(true)?;
        if (bounds[1] - bounds[0]).min_element() <= thickness * 2.0 {
            return Err(
                "Shell thickness must be less than half the smallest body dimension".into(),
            );
        }
        let mut result = std::ptr::null_mut();
        let status = unsafe { ffi::spice_shell(self.shape.as_ptr(), thickness, face, &mut result) };
        let body = self._kernel.shape_result(result, status, "shell")?;
        if body.volume()? >= self.volume()? {
            return Err("Shell did not remove interior material".into());
        }
        *self = body;
        Ok(())
    }
}
