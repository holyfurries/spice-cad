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
    pub fn skin(&self, skin: &crate::shape_operation::skin::Skin) -> Result<Body<'_>, String> {
        assert!(skin.points.len() >= 4);
        assert!((4..=32768).contains(&skin.triangles.len()));
        let mut coordinates = Vec::with_capacity(skin.triangles.len() * 9);
        for face in &skin.triangles {
            for &index in face {
                coordinates.extend_from_slice(&skin.points[index as usize]);
            }
        }
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            ffi::spice_faceted_body(coordinates.as_ptr(), coordinates.len(), &mut result)
        };
        self.shape_result(result, status, "skin")
    }

    pub fn faceted_revolve(
        &self,
        polygon: &crate::profile::Polygon,
        segments: u32,
    ) -> Result<Body<'_>, String> {
        assert!((3..=128).contains(&segments));
        assert!((3..=128).contains(&polygon.points.len()));
        let mut directions = [[0.0; 2]; 128];
        for index in 0..segments {
            let angle = std::f64::consts::TAU * f64::from(index) / f64::from(segments);
            directions[index as usize] = [angle.cos(), angle.sin()];
        }
        let mut coordinates = Vec::with_capacity(polygon.points.len() * segments as usize * 18);
        for index in 0..polygon.points.len() {
            let start = polygon.points[index];
            let end = polygon.points[(index + 1) % polygon.points.len()];
            for sector in 0..segments as usize {
                let next = (sector + 1) % segments as usize;
                let point = |[radius, height]: [f64; 2], angle: usize| {
                    let radius = if radius < 1e-9 { 0.0 } else { radius };
                    [
                        radius * directions[angle][0],
                        radius * directions[angle][1],
                        height,
                    ]
                };
                let a = point(start, sector);
                let b = point(end, sector);
                let c = point(end, next);
                let d = point(start, next);
                if end[0] >= 1e-9 {
                    for point in [a, c, b] {
                        coordinates.extend_from_slice(&point);
                    }
                }
                if start[0] >= 1e-9 {
                    for point in [a, d, c] {
                        coordinates.extend_from_slice(&point);
                    }
                }
            }
        }
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            ffi::spice_faceted_body(coordinates.as_ptr(), coordinates.len(), &mut result)
        };
        self.shape_result(result, status, "faceted revolve")
    }

    pub fn path_sweep(
        &self,
        section: &crate::shape_operation::SweepProfile,
        path: &crate::shape_operation::SweepPath,
    ) -> Result<Body<'_>, String> {
        let crate::shape_operation::SweepPath::Mitered(path) = path else {
            let crate::shape_operation::SweepPath::Smooth(segments) = path else {
                unreachable!()
            };
            return self.smooth_sweep(section, segments);
        };
        let line_count = match section {
            crate::shape_operation::SweepProfile::Polygon(polygon) => polygon.points.len(),
            crate::shape_operation::SweepProfile::Circle { .. } => 0,
        };
        let mut lines = Vec::with_capacity(line_count + path.len());
        let mut arcs = Vec::with_capacity(usize::from(line_count == 0) * 2);
        let mut profile = ffi::spice_curve_array().map_err(|error| error.to_string())?;
        let mut route = ffi::spice_curve_array().map_err(|error| error.to_string())?;
        match section {
            crate::shape_operation::SweepProfile::Polygon(polygon) => {
                let points: Vec<_> = polygon.points.iter().map(|&[x, y]| [x, y, 0.0]).collect();
                add_lines(&points, true, &mut profile, &mut lines)?;
            }
            crate::shape_operation::SweepProfile::Circle { radius } => {
                assert!(radius.is_finite());
                assert!(*radius >= 0.01);
                for start in [0.0, 180.0] {
                    let arc =
                        ffi::spice_profile_arc(&Vector::default(), *radius, start, start + 180.0)
                            .map_err(|error| error.to_string())?;
                    if arc.is_null() {
                        return Err("SMLib sweep circle creation failed".into());
                    }
                    unsafe { profile.pin_mut().Add(arc.as_ptr().cast_mut()) }
                        .map_err(|error| error.to_string())?;
                    arcs.push(arc);
                }
            }
        }
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

    fn smooth_sweep(
        &self,
        section: &crate::shape_operation::SweepProfile,
        segments: &[crate::shape_operation::PathSegment],
    ) -> Result<Body<'_>, String> {
        use crate::shape_operation::{PathSegment, SweepProfile};
        assert!(!segments.is_empty());
        assert!(segments.len() <= 125);
        let first_tangent = match segments[0] {
            PathSegment::Line { start, end } => (end - start).normalize(),
            _ => unreachable!("Smooth paths begin with a straight span"),
        };
        let normal = segments
            .iter()
            .find_map(|segment| match segment {
                PathSegment::Arc { x_axis, y_axis, .. } => Some(x_axis.cross(*y_axis).normalize()),
                _ => None,
            })
            .unwrap_or_else(|| first_tangent.any_orthonormal_vector());
        let vector = |point: glam::DVec3| Vector {
            x: point.x,
            y: point.y,
            z: point.z,
        };
        let points = match section {
            SweepProfile::Polygon(polygon) => {
                let mut minimum = glam::DVec2::splat(f64::INFINITY);
                let mut maximum = glam::DVec2::splat(f64::NEG_INFINITY);
                for &point in &polygon.points {
                    minimum = minimum.min(glam::DVec2::from_array(point));
                    maximum = maximum.max(glam::DVec2::from_array(point));
                }
                let center = (minimum + maximum) * 0.5;
                polygon
                    .points
                    .iter()
                    .map(|&point| glam::DVec2::from_array(point) - center)
                    .collect::<Vec<_>>()
            }
            SweepProfile::Circle { .. } => Vec::new(),
        };
        let mut lines = Vec::with_capacity(points.len());
        let mut arcs = Vec::with_capacity(usize::from(points.is_empty()) * 2);
        let mut transformed = [[0.0; 3]; crate::profile::POINT_COUNT_MAX];
        let mut body: Option<Body<'_>> = None;
        for segment in segments {
            lines.clear();
            arcs.clear();
            let (origin, tangent) = match segment {
                PathSegment::Line { start, end } => (*start, (*end - *start).normalize()),
                PathSegment::Arc {
                    center,
                    x_axis,
                    y_axis,
                    radius,
                    ..
                } => (*center + *x_axis * *radius, *y_axis),
            };
            let y_axis = tangent.cross(normal).normalize();
            let mut curves = ffi::spice_curve_array().map_err(|error| error.to_string())?;
            match section {
                SweepProfile::Polygon(_) => {
                    for (target, point) in transformed.iter_mut().zip(&points) {
                        *target = (origin + normal * point.x + y_axis * point.y).to_array();
                    }
                    add_lines(&transformed[..points.len()], true, &mut curves, &mut lines)?;
                }
                SweepProfile::Circle { radius } => {
                    for sense in [1.0, -1.0] {
                        let arc = ffi::spice_path_arc(
                            &vector(origin),
                            &vector(normal * sense),
                            &vector(y_axis * sense),
                            *radius,
                            180.0,
                        )
                        .map_err(|error| error.to_string())?;
                        if arc.is_null() {
                            return Err("SMLib smooth sweep section failed".into());
                        }
                        unsafe { curves.pin_mut().Add(arc.as_ptr().cast_mut()) }
                            .map_err(|error| error.to_string())?;
                        arcs.push(arc);
                    }
                }
            }
            let mut result = std::ptr::null_mut();
            let status = unsafe {
                match segment {
                    PathSegment::Line { start, end } => ffi::SmApiCreateLinearSweep(
                        &curves,
                        &vector(tangent),
                        start.distance(*end),
                        1,
                        &mut result,
                    ),
                    PathSegment::Arc {
                        center,
                        x_axis,
                        y_axis,
                        angle,
                        ..
                    } => ffi::SmApiCreateRotationalSweep(
                        &curves,
                        &vector(*center),
                        &vector(x_axis.cross(*y_axis)),
                        angle.to_degrees(),
                        1,
                        &mut result,
                    ),
                }
            };
            let next = self.shape_result(result, status, "smooth sweep span")?;
            if let Some(body) = body.as_mut() {
                body.union(next)?;
            } else {
                body = Some(next);
            }
        }
        let body = body.expect("Nonempty smooth path");
        if body.shape.IsManifoldSolid() != 1 || body.volume()? <= 1e-8 {
            return Err("Smooth sweep did not produce a closed manifold solid".into());
        }
        Ok(body)
    }

    pub fn loft(
        &self,
        sections: &[crate::shape_operation::Section],
        degree: u32,
    ) -> Result<Body<'_>, String> {
        assert!((2..=16).contains(&sections.len()));
        assert!((1..=3).contains(&degree));
        let mut curves = ffi::spice_curve_array().map_err(|error| error.to_string())?;
        let edge_count = sections[0].profile.segments().len();
        let mut lines = Vec::with_capacity(sections.len() * edge_count);
        let mut arcs = Vec::with_capacity(sections.len() * edge_count);
        for section in sections {
            let [x, y, z] = section.position;
            for segment in section.profile.segments() {
                if let Some(center) = segment.center {
                    let arc = ffi::spice_profile_arc(
                        &Vector {
                            x: center.x + x,
                            y: center.y + y,
                            z,
                        },
                        segment.radius(),
                        segment.start_angle().to_degrees(),
                        (segment.start_angle() + segment.angle).to_degrees(),
                    )
                    .map_err(|error| error.to_string())?;
                    if arc.is_null() {
                        return Err("SMLib loft arc creation failed".into());
                    }
                    unsafe { curves.pin_mut().Add(arc.as_ptr().cast_mut()) }
                        .map_err(|error| error.to_string())?;
                    arcs.push(arc);
                } else {
                    add_lines(
                        &[
                            [segment.start.x + x, segment.start.y + y, z],
                            [segment.end.x + x, segment.end.y + y, z],
                        ],
                        false,
                        &mut curves,
                        &mut lines,
                    )?;
                }
            }
        }
        let mut result = std::ptr::null_mut();
        let status =
            unsafe { ffi::spice_loft(&curves, sections.len(), edge_count, degree, &mut result) };
        self.shape_result(result, status, "loft")
    }

    pub(super) fn shape_result(
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
