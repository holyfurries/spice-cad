use glam::DVec3;
use mlua::Table;

#[derive(Clone, PartialEq)]
pub struct Section {
    pub profile: crate::profile::Profile,
    pub position: [f64; 3],
}

#[derive(PartialEq)]
pub enum Recipe {
    Sweep {
        profile: SweepProfile,
        path: SweepPath,
    },
    Loft {
        sections: Vec<Section>,
        degree: u32,
    },
}

#[derive(PartialEq)]
pub enum SweepPath {
    Mitered(Vec<[f64; 3]>),
    Smooth(Vec<PathSegment>),
}

#[derive(PartialEq)]
pub enum PathSegment {
    Line {
        start: DVec3,
        end: DVec3,
    },
    Arc {
        center: DVec3,
        x_axis: DVec3,
        y_axis: DVec3,
        radius: f64,
        angle: f64,
    },
}

#[derive(Clone, PartialEq)]
pub enum SweepProfile {
    Polygon(crate::profile::Polygon),
    Circle { radius: f64 },
}

impl SweepProfile {
    pub fn from_profile(profile: &crate::profile::Profile) -> mlua::Result<Self> {
        match profile {
            crate::profile::Profile::Polygon(polygon) => Ok(Self::Polygon(polygon.clone())),
            crate::profile::Profile::Curves(segments) if segments.len() == 2 => {
                if let Some(center) = segments[0].center {
                    if segments.iter().all(|segment| {
                        segment
                            .center
                            .is_some_and(|other| other.distance(center) <= 1e-7)
                            && (segment.angle - std::f64::consts::PI).abs() <= 1e-9
                            && (segment.radius() - segments[0].radius()).abs() <= 1e-7
                    }) {
                        return Ok(Self::Circle {
                            radius: segments[0].radius(),
                        });
                    }
                }
                Err(mlua::Error::runtime(
                    "Sweep requires a polygon or a full circle; mixed arcs and nested loops are unsupported",
                ))
            }
            _ => Err(mlua::Error::runtime(
                "Sweep requires a polygon or a full circle; mixed arcs and nested loops are unsupported",
            )),
        }
    }

    fn footprint_radius(&self) -> f64 {
        match self {
            Self::Circle { radius } => *radius,
            Self::Polygon(polygon) => {
                let mut minimum = glam::DVec2::splat(f64::INFINITY);
                let mut maximum = glam::DVec2::splat(f64::NEG_INFINITY);
                for &point in &polygon.points {
                    minimum = minimum.min(glam::DVec2::from_array(point));
                    maximum = maximum.max(glam::DVec2::from_array(point));
                }
                (maximum - minimum).length() * 0.5
            }
        }
    }
}

pub struct Shell {
    pub thickness: f64,
    pub opening: Option<crate::dimensions::Selector>,
}

pub fn validate_loft_polygon(
    polygon: &crate::profile::Polygon,
    previous: Option<&crate::profile::Polygon>,
) -> mlua::Result<()> {
    assert!((3..=crate::profile::POINT_COUNT_MAX).contains(&polygon.points.len()));
    assert!(previous.is_none_or(|previous| previous.points.len() == polygon.points.len()));
    for index in 0..polygon.points.len() {
        let first = glam::DVec2::from_array(polygon.points[index]);
        let second = glam::DVec2::from_array(polygon.points[(index + 1) % polygon.points.len()]);
        let third = glam::DVec2::from_array(polygon.points[(index + 2) % polygon.points.len()]);
        if (second - first).perp_dot(third - second) <= 1e-9 {
            return Err(mlua::Error::runtime(
                "Loft profiles must be strictly convex polygons",
            ));
        }
        if let Some(previous) = previous {
            let edge =
                glam::DVec2::from_array(previous.points[(index + 1) % previous.points.len()])
                    - glam::DVec2::from_array(previous.points[index]);
            if edge.dot(second - first) <= 1e-9 {
                return Err(mlua::Error::runtime(
                    "Loft vertices must correspond without twisting; keep matching edges facing the same direction",
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_loft_profile(
    profile: &crate::profile::Profile,
    previous: Option<&crate::profile::Profile>,
) -> mlua::Result<()> {
    use crate::profile::Profile;
    if matches!(profile, Profile::Region(_)) {
        return Err(mlua::Error::runtime(
            "Loft requires single convex loops; holes and islands are unsupported",
        ));
    }
    if let Profile::Polygon(polygon) = profile {
        let previous_polygon = previous.and_then(|value| value.polygon().ok());
        if previous_polygon.is_some_and(|value| value.points.len() != polygon.points.len()) {
            return Err(mlua::Error::runtime(
                "Loft profiles must have equal vertex counts and matching vertex order",
            ));
        }
        validate_loft_polygon(polygon, previous_polygon)?;
    }
    let segments = profile.segments();
    let tangent = |segment: &crate::profile::Segment, end: bool| {
        let point = if end { segment.end } else { segment.start };
        match segment.center {
            Some(center) => (point - center).perp().normalize(),
            None => (segment.end - segment.start).normalize(),
        }
    };
    let mut turning = 0.0;
    for (index, segment) in segments.iter().enumerate() {
        if segment.angle < 0.0 {
            return Err(mlua::Error::runtime(
                "Loft profiles must be convex with counterclockwise arcs",
            ));
        }
        let incoming = tangent(segment, true);
        let outgoing = tangent(&segments[(index + 1) % segments.len()], false);
        let angle = incoming.perp_dot(outgoing).atan2(incoming.dot(outgoing));
        if angle < -1e-7 {
            return Err(mlua::Error::runtime(
                "Loft profiles must be convex without reflex corners",
            ));
        }
        turning += segment.angle + angle.max(0.0);
    }
    if (turning - std::f64::consts::TAU).abs() > 1e-6 {
        return Err(mlua::Error::runtime(
            "Loft profiles must be single convex loops",
        ));
    }
    if let Some(previous) = previous {
        if matches!(previous, Profile::Region(_)) {
            return Err(mlua::Error::runtime("Loft requires single convex loops"));
        }
        let before = previous.segments();
        if before.len() != segments.len() {
            return Err(mlua::Error::runtime(
                "Loft profiles need matching line/arc counts and order",
            ));
        }
        for (first, second) in before.iter().zip(&segments) {
            if first.center.is_some() != second.center.is_some()
                || (first.angle - second.angle).abs() > 1e-7
                || tangent(first, false).dot(tangent(second, false)) <= 1e-9
                || tangent(first, true).dot(tangent(second, true)) <= 1e-9
            {
                return Err(mlua::Error::runtime(
                    "Loft sections need corresponding edges, arc angles, and seam directions without twisting",
                ));
            }
        }
    }
    Ok(())
}

pub fn array_count(table: &Table, minimum: usize, maximum: usize) -> mlua::Result<usize> {
    let count = table.raw_len();
    if !(minimum..=maximum).contains(&count) {
        return Err(mlua::Error::runtime(format!(
            "Expected {minimum}–{maximum} dense array entries"
        )));
    }
    let mut entries = 0;
    for pair in table.clone().pairs::<mlua::Value, mlua::Value>() {
        let (key, _) = pair?;
        if !matches!(key, mlua::Value::Integer(index) if index >= 1 && index as usize <= count) {
            return Err(mlua::Error::runtime(
                "Expected a dense array without named fields",
            ));
        }
        entries += 1;
    }
    if entries != count {
        return Err(mlua::Error::runtime("Expected a dense array"));
    }
    Ok(count)
}

pub fn position(table: &Table, require_xy: bool) -> mlua::Result<[f64; 3]> {
    let values = [
        if require_xy {
            table.get("x")?
        } else {
            table.get::<Option<f64>>("x")?.unwrap_or(0.0)
        },
        if require_xy {
            table.get("y")?
        } else {
            table.get::<Option<f64>>("y")?.unwrap_or(0.0)
        },
        table.get("z")?,
    ];
    if values
        .iter()
        .any(|value: &f64| !value.is_finite() || value.abs() > 2000.0)
    {
        return Err(mlua::Error::runtime(
            "Shape coordinates must be finite and within ±2000 mm",
        ));
    }
    Ok(values)
}

pub fn path(
    table: Table,
    profile: &SweepProfile,
    bend_radius: Option<f64>,
) -> mlua::Result<SweepPath> {
    let count = array_count(&table, 2, 64)?;
    let mut points = Vec::with_capacity(count);
    for index in 1..=count {
        let point: Table = table.raw_get(index)?;
        crate::model::options_fields(&point, &["x", "y", "z"])?;
        points.push(position(&point, true)?);
    }
    let vectors: Vec<_> = points.iter().copied().map(DVec3::from_array).collect();
    let mut normal = None;
    let first = vectors[1] - vectors[0];
    let mut length = 0.0;
    let radius = profile.footprint_radius();
    if let Some(bend) = bend_radius {
        if !bend.is_finite() || !(0.01..=1000.0).contains(&bend) {
            return Err(mlua::Error::runtime(
                "Sweep bend radius must be finite and between 0.01 and 1000 mm",
            ));
        }
        if bend <= radius + 1e-7 {
            return Err(mlua::Error::runtime(
                "Sweep bend radius must exceed the profile footprint radius",
            ));
        }
    }
    for index in 1..count {
        let direction = vectors[index] - vectors[index - 1];
        if direction.length() < 0.01 {
            return Err(mlua::Error::runtime(
                "Sweep path segments must be at least 0.01 mm",
            ));
        }
        length += direction.length();
        let cross = first.cross(vectors[index] - vectors[0]);
        if cross.length() > 1e-8 {
            if normal.is_none() {
                normal = Some(cross.normalize());
            }
        }
        if index > 1 {
            let previous = vectors[index - 1] - vectors[index - 2];
            let cosine = previous
                .normalize()
                .dot(direction.normalize())
                .clamp(-1.0, 1.0);
            if cosine < -0.999 {
                return Err(mlua::Error::runtime("Sweep paths must not double back"));
            }
            let tangent_half = ((1.0 - cosine) / (1.0 + cosine)).sqrt();
            if bend_radius.is_none()
                && previous.length().min(direction.length()) <= radius * tangent_half * 2.0
            {
                return Err(mlua::Error::runtime(
                    "Sweep segments are too short for the profile and miter angle",
                ));
            }
        }
    }
    if length > 4000.0 {
        return Err(mlua::Error::runtime("Sweep path length exceeds 4000 mm"));
    }
    if let Some(normal) = normal {
        for point in &vectors {
            if normal.dot(*point - vectors[0]).abs() > 1e-7 {
                return Err(mlua::Error::runtime("Sweep paths must be planar"));
            }
        }
    }
    for index in 0..count - 1 {
        for other in index + 2..count - 1 {
            let first = vectors[index + 1] - vectors[index];
            let second = vectors[other + 1] - vectors[other];
            let offset = vectors[index] - vectors[other];
            let determinant =
                first.length_squared() * second.length_squared() - first.dot(second).powi(2);
            let endpoint_distance = |point: DVec3, start: DVec3, direction: DVec3| {
                point.distance(
                    start
                        + direction
                            * ((point - start).dot(direction) / direction.length_squared())
                                .clamp(0.0, 1.0),
                )
            };
            let mut distance = endpoint_distance(vectors[index], vectors[other], second)
                .min(endpoint_distance(
                    vectors[index + 1],
                    vectors[other],
                    second,
                ))
                .min(endpoint_distance(vectors[other], vectors[index], first))
                .min(endpoint_distance(vectors[other + 1], vectors[index], first));
            if determinant > 1e-12 {
                let a = (first.dot(second) * second.dot(offset)
                    - second.length_squared() * first.dot(offset))
                    / determinant;
                let b = (first.length_squared() * second.dot(offset)
                    - first.dot(second) * first.dot(offset))
                    / determinant;
                if (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b) {
                    distance = distance.min((offset + first * a - second * b).length());
                }
            }
            if distance <= (radius + bend_radius.unwrap_or(0.0)) * 2.0 {
                return Err(mlua::Error::runtime(
                    "Nonadjacent sweep segments need separation beyond the profile diameter and bend allowance",
                ));
            }
        }
    }
    let Some(bend_radius) = bend_radius else {
        return Ok(SweepPath::Mitered(points));
    };
    let mut trims = vec![0.0; count];
    for index in 1..count - 1 {
        let incoming = (vectors[index] - vectors[index - 1]).normalize();
        let outgoing = (vectors[index + 1] - vectors[index]).normalize();
        let angle = incoming
            .cross(outgoing)
            .length()
            .atan2(incoming.dot(outgoing));
        if angle > 1e-10 {
            trims[index] = bend_radius * (angle * 0.5).tan();
            if trims[index] < 0.01 {
                return Err(mlua::Error::runtime(
                    "Sweep bend is too small; remove nearly collinear waypoints",
                ));
            }
        }
    }
    for index in 0..count - 1 {
        if vectors[index].distance(vectors[index + 1]) - trims[index] - trims[index + 1] < 0.01 {
            return Err(mlua::Error::runtime(
                "Sweep bend radius leaves insufficient straight length between corners",
            ));
        }
    }
    let mut segments = Vec::with_capacity(count * 2 - 3);
    let mut start = vectors[0];
    for index in 1..count - 1 {
        let incoming = (vectors[index] - vectors[index - 1]).normalize();
        let outgoing = (vectors[index + 1] - vectors[index]).normalize();
        let end = vectors[index] - incoming * trims[index];
        segments.push(PathSegment::Line { start, end });
        start = vectors[index] + outgoing * trims[index];
        if trims[index] > 0.0 {
            let normal = incoming.cross(outgoing).normalize();
            let x_axis = incoming.cross(normal);
            let center = end - x_axis * bend_radius;
            segments.push(PathSegment::Arc {
                center,
                x_axis,
                y_axis: normal.cross(x_axis),
                radius: bend_radius,
                angle: incoming.dot(outgoing).clamp(-1.0, 1.0).acos(),
            });
        }
    }
    segments.push(PathSegment::Line {
        start,
        end: vectors[count - 1],
    });
    Ok(SweepPath::Smooth(segments))
}

#[cfg(test)]
mod tests {
    use crate::{kernel::Kernel, model};

    const ROUNDED_PROFILE: &str = "local function rounded(width:number,height:number,radius:number) return cad.path({start={x=radius,y=0},segments={{to={x=width-radius,y=0}},{to={x=width,y=radius},center={x=width-radius,y=radius}},{to={x=width,y=height-radius}},{to={x=width-radius,y=height},center={x=width-radius,y=height-radius}},{to={x=radius,y=height}},{to={x=0,y=height-radius},center={x=radius,y=height-radius}},{to={x=0,y=radius}},{to={x=radius,y=0},center={x=radius,y=radius}}}}) end\n";

    #[test]
    fn curved_lofts_keep_exact_sections_volume_closed_mesh_and_step() -> Result<(), String> {
        let kernel = Kernel::new()?;
        for (source, expected) in [
            ("cad.show('cone',cad.loft({{profile=cad.circle({radius=5,x=7,y=9}),z=0},{profile=cad.circle({radius=3,x=7,y=9}),z=12,x=2}}))".to_owned(), std::f64::consts::PI * 196.0),
            (format!("{ROUNDED_PROFILE}local p=rounded(20,12,2); cad.show('rounded',cad.loft({{{{profile=p,z=0}},{{profile=p,z=15,x=3}}}}))"), (240.0 - (4.0 - std::f64::consts::PI) * 4.0) * 15.0),
            ("local p=cad.circle({radius=4});cad.show('smooth',cad.loft({{profile=p,z=0},{profile=p,z=8},{profile=p,z=16},{profile=p,z=24}},{degree=3}))".to_owned(), std::f64::consts::PI * 16.0 * 24.0),
        ] {
            let compiled = model::compile(&source)?;
            let mut bodies = compiled.build(&kernel)?;
            let body = &mut bodies[0];
            let volume = body.volume()?;
            assert!((volume - expected).abs() < expected * 1e-5, "{volume} != {expected}");
            let mesh = body.mesh()?.mesh;
            mesh.validate()?;
            assert!((mesh.volume_mm3() - expected).abs() < expected * 0.02);
            assert!(body.step_data("curved-loft")?.contains("CLOSED_SHELL"));
        }
        Ok(())
    }

    #[test]
    fn rounded_sweeps_have_exact_tangent_bends_in_multiple_planes() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let elbow_length = 60.0 - 20.0 + std::f64::consts::FRAC_PI_2 * 10.0;
        for (path, length) in [
            (
                "{{x=0,y=0,z=0},{x=0,y=0,z=30},{x=30,y=0,z=30}}",
                elbow_length,
            ),
            (
                "{{x=0,y=0,z=0},{x=0,y=0,z=30},{x=-30,y=0,z=30}}",
                elbow_length,
            ),
            (
                "{{x=0,y=0,z=0},{x=30,y=0,z=0},{x=30,y=30,z=0}}",
                elbow_length,
            ),
            (
                "{{x=0,y=0,z=0},{x=18,y=24,z=0},{x=18,y=24,z=30}}",
                elbow_length,
            ),
            (
                "{{x=0,y=0,z=0},{x=0,y=0,z=60},{x=60,y=0,z=60},{x=60,y=0,z=120}}",
                180.0 - 40.0 + std::f64::consts::PI * 10.0,
            ),
            ("{{x=0,y=0,z=0},{x=18,y=24,z=0}}", 30.0),
        ] {
            for (profile, area) in [
                ("cad.circle({radius=2})", std::f64::consts::PI * 4.0),
                (
                    "cad.polygon({{x=-1,y=-2},{x=1,y=-2},{x=1,y=2},{x=-1,y=2}})",
                    8.0,
                ),
            ] {
                let source =
                    format!("cad.show('bend',cad.sweep({profile},{{path={path},bend_radius=10}}))");
                let compiled = model::compile(&source)?;
                let mut bodies = compiled
                    .build(&kernel)
                    .map_err(|error| format!("{path} {profile}: {error}"))?;
                let body = &mut bodies[0];
                let expected = area * length;
                let volume = body.volume()?;
                assert!(
                    (volume - expected).abs() < expected * 1e-5,
                    "{path}: {volume} != {expected}"
                );
                let mesh = body.mesh()?.mesh;
                mesh.validate()?;
                assert!((mesh.volume_mm3() - expected).abs() < expected * 0.025);
                assert!(body.step_data("smooth-bend")?.contains("CLOSED_SHELL"));
            }
        }
        Ok(())
    }

    #[test]
    fn curved_operations_reject_invalid_matches_degrees_and_bend_clearances() -> Result<(), String>
    {
        for source in [
            "local p=cad.circle({radius=2});cad.show('x',cad.loft({{profile=p,z=0},{profile=p,z=10}},{degree=3}))",
            "local p=cad.circle({radius=2});cad.show('x',cad.loft({{profile=p,z=0},{profile=p,z=10}},{degree=0}))",
            "local p=cad.circle({radius=2});cad.show('x',cad.loft({{profile=p,z=0},{profile=p,z=10},{profile=p,z=20},{profile=p,z=30}},{degree=2.5}))",
            "local p=cad.circle({radius=2});cad.show('x',cad.loft({{profile=p,z=0},{profile=cad.polygon({{x=0,y=0},{x=4,y=0},{x=4,y=4},{x=0,y=4}}),z=10}}))",
            "local p=cad.profile({loops={cad.circle({radius=4}),cad.circle({radius=2})}});cad.show('x',cad.loft({{profile=p,z=0},{profile=p,z=10}}))",
        ] {
            assert!(model::compile(source).is_err(), "{source}");
        }
        for radius in ["0", "2", "30", "0/0", "math.huge", "-1"] {
            let source = format!(
                "cad.show('x',cad.sweep(cad.circle({{radius=2}}),{{path={{{{x=0,y=0,z=0}},{{x=0,y=0,z=30}},{{x=30,y=0,z=30}}}},bend_radius={radius}}}))"
            );
            assert!(model::compile(&source).is_err(), "radius={radius}");
        }
        Ok(())
    }

    #[test]
    fn curved_operation_cache_tracks_bend_radius_sections_and_loft_degree() -> Result<(), String> {
        let kernel = Kernel::new()?;
        for (source, target, replacement) in [
            (
                "cad.show('x',cad.sweep(cad.circle({radius=2}),{path={{x=0,y=0,z=0},{x=0,y=0,z=40},{x=40,y=0,z=40}},bend_radius=10}))",
                "bend_radius=10",
                "bend_radius=12",
            ),
            (
                "cad.show('x',cad.loft({{profile=cad.circle({radius=6}),z=0},{profile=cad.circle({radius=7}),z=10},{profile=cad.circle({radius=5}),z=20},{profile=cad.circle({radius=4}),z=30}},{degree=1}))",
                "degree=1",
                "degree=3",
            ),
        ] {
            let mut cache = model::Cache::new(&kernel);
            let first = model::compile(source)?;
            let prepared = cache.prepare(&first)?;
            let geometry = std::sync::Arc::clone(&prepared.geometry[0]);
            drop(prepared);
            let same = model::compile(source)?;
            let prepared = cache.prepare(&same)?;
            assert_eq!(prepared.hits, 1);
            assert!(std::sync::Arc::ptr_eq(&geometry, &prepared.geometry[0]));
            drop(prepared);
            let changed = model::compile(&source.replace(target, replacement))?;
            let prepared = cache.prepare(&changed)?;
            assert_eq!(prepared.hits, 0);
            assert!(!std::sync::Arc::ptr_eq(&geometry, &prepared.geometry[0]));
            assert!((prepared.geometry[0].volume_mm3 - geometry.volume_mm3).abs() > 0.1);
            drop(prepared);
            let changed = model::compile(
                &source
                    .replace("radius=2", "radius=3")
                    .replace("radius=6", "radius=8"),
            )?;
            let prepared = cache.prepare(&changed)?;
            assert_eq!(prepared.hits, 0);
            assert!((prepared.geometry[0].volume_mm3 - geometry.volume_mm3).abs() > 0.1);
        }
        Ok(())
    }

    #[test]
    fn curved_examples_build_closed_solids_and_hollow_bends() -> Result<(), String> {
        for source in [
            include_str!("../../../examples/curved-loft.luau"),
            include_str!("../../../examples/smooth-sweep.luau"),
        ] {
            let snapshot = crate::build_script_snapshot(source, 1)?;
            snapshot.scene.world_mesh().validate()?;
            assert!(snapshot.volume_mm3 > 1.0);
            assert!(snapshot.scene.bounds[0].z.abs() < 0.001);
        }
        let source = include_str!("../../../examples/smooth-sweep.luau");
        let compiled = model::compile(source)?;
        let kernel = Kernel::new()?;
        let mut bodies = compiled.build(&kernel)?;
        let expected =
            std::f64::consts::PI * (36.0 - 20.25) * (90.0 - 40.0 + std::f64::consts::PI * 10.0);
        assert!((bodies[0].volume()? - expected).abs() < expected * 1e-5);
        assert!(
            bodies[0]
                .step_data("hollow-elbow")?
                .contains("CLOSED_SHELL")
        );
        Ok(())
    }

    #[test]
    fn exact_circle_sweeps_preserve_volume_centering_closed_mesh_and_step() -> Result<(), String> {
        let kernel = Kernel::new()?;
        for (path, length, bounds) in [
            (
                "{{x=5,y=7,z=0},{x=5,y=7,z=20}}",
                20.0,
                Some([[2.0, 4.0, 0.0], [8.0, 10.0, 20.0]]),
            ),
            ("{{x=0,y=0,z=0},{x=0,y=0,z=20},{x=20,y=0,z=20}}", 40.0, None),
        ] {
            for center in ["", ",x=123,y=-27"] {
                let source = format!(
                    "local p=cad.circle({{radius=3{center}}});cad.show('tube',cad.sweep(p,{{path={path}}}))"
                );
                let model = model::compile(&source)?;
                let mut bodies = model.build(&kernel)?;
                let body = &mut bodies[0];
                let expected = std::f64::consts::PI * 9.0 * length;
                let volume = body.volume()?;
                assert!(
                    (volume - expected).abs() < expected * 1e-6,
                    "center={center}, path={path}: volume {volume} != {expected}"
                );
                let mesh = body.mesh()?.mesh;
                mesh.validate()?;
                assert!(
                    (mesh.volume_mm3() - expected).abs() < expected * 0.015,
                    "Tessellated volume {} differs from exact {expected}",
                    mesh.volume_mm3()
                );
                if let Some([minimum, maximum]) = bounds {
                    let actual = body.bounds(true)?;
                    assert!(
                        (actual[0] - glam::DVec3::from_array(minimum))
                            .abs()
                            .max_element()
                            < 0.001
                    );
                    assert!(
                        (actual[1] - glam::DVec3::from_array(maximum))
                            .abs()
                            .max_element()
                            < 0.001
                    );
                }
                let step = body.step_data("circle-sweep")?;
                assert!(step.contains("CLOSED_SHELL"));
                assert!(step.contains("ISO-10303-21"));
            }
        }
        Ok(())
    }

    #[test]
    fn circle_sweep_guards_use_the_centered_radius_and_reject_other_curves() -> Result<(), String> {
        for profile in [
            "cad.profile({loops={cad.circle({radius=3}),cad.circle({radius=1})}})",
            "cad.path({start={x=0,y=-3},segments={{to={x=0,y=3},center={x=0,y=0}},{to={x=0,y=-3}}}})",
        ] {
            let source = format!(
                "cad.show('bad',cad.sweep({profile},{{path={{{{x=0,y=0,z=0}},{{x=0,y=0,z=20}}}}}}))"
            );
            let error = model::compile(&source)
                .err()
                .ok_or("Expected unsupported profile error")?;
            assert!(error.contains("polygon or a full circle"), "{error}");
        }
        for radius in [3, 4] {
            let source = format!(
                "local p=cad.circle({{radius={radius},x=100,y=-100}});cad.show('elbow',cad.sweep(p,{{path={{{{x=0,y=0,z=0}},{{x=0,y=0,z=7}},{{x=7,y=0,z=7}}}}}}))"
            );
            assert_eq!(model::compile(&source).is_ok(), radius == 3);
        }
        Ok(())
    }

    #[test]
    fn circle_sweep_cache_includes_radius_and_reuses_equal_profiles() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let mut cache = model::Cache::new(&kernel);
        let source = "local p=cad.circle({radius=3});cad.show('pipe',cad.sweep(p,{path={{x=0,y=0,z=0},{x=0,y=0,z=20},{x=20,y=0,z=20}}}))";
        let model = model::compile(source)?;
        let prepared = cache.prepare(&model)?;
        let original = std::sync::Arc::clone(&prepared.geometry[0]);
        let volume = prepared.geometry[0].volume_mm3;
        drop(prepared);
        let equal = model::compile(source)?;
        let prepared = cache.prepare(&equal)?;
        assert_eq!(prepared.hits, 1);
        assert!(std::sync::Arc::ptr_eq(&original, &prepared.geometry[0]));
        drop(prepared);
        let centered = model::compile(&source.replace("radius=3", "radius=3,x=123,y=-27"))?;
        let prepared = cache.prepare(&centered)?;
        assert_eq!(prepared.hits, 1);
        assert!(std::sync::Arc::ptr_eq(&original, &prepared.geometry[0]));
        drop(prepared);
        let changed = model::compile(&source.replace("radius=3", "radius=4"))?;
        let prepared = cache.prepare(&changed)?;
        assert_eq!(prepared.hits, 0);
        assert!(!std::sync::Arc::ptr_eq(&original, &prepared.geometry[0]));
        assert!((prepared.geometry[0].volume_mm3 / volume - 16.0 / 9.0).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn hollow_circle_sweep_example_keeps_an_open_bore_and_closed_material() -> Result<(), String> {
        let source = include_str!("../../../examples/mitered-pipe.luau");
        let snapshot = crate::build_script_snapshot(source, 1)?;
        let expected = std::f64::consts::PI * (25.0 - 12.25) * 60.0;
        assert!((snapshot.volume_mm3 - expected).abs() < expected * 1e-6);
        snapshot.scene.world_mesh().validate()?;
        assert!(snapshot.scene.bounds[0].z.abs() < 0.001);
        Ok(())
    }

    #[test]
    fn loft_profile_lookup_can_create_profiles_without_borrowing_recipe() -> Result<(), String> {
        let source = "local a=cad.polygon({{x=0,y=0},{x=10,y=0},{x=10,y=10},{x=0,y=10}});local section:any=setmetatable({z=12},{__index=function(_,key) if key=='profile' then return cad.polygon({{x=0,y=0},{x=8,y=0},{x=8,y=8},{x=0,y=8}}) end;return nil end});cad.show('loft',cad.loft({{profile=a,z=0},section}))";
        let compiled = model::compile(source)?;
        assert_eq!(compiled.parts.len(), 1);
        assert_eq!(compiled.parts[0].name, "loft");
        Ok(())
    }

    #[test]
    fn shape_operations_make_closed_solids_with_exact_volume_and_step() -> Result<(), String> {
        let kernel = Kernel::new()?;
        for (source, expected) in [
            (
                "local p=cad.polygon({{x=-2,y=-1},{x=2,y=-1},{x=2,y=1},{x=-2,y=1}}) cad.show('rail',cad.sweep(p,{path={{x=0,y=0,z=0},{x=0,y=0,z=20},{x=20,y=0,z=20}}}))",
                320.0,
            ),
            (
                "local a=cad.polygon({{x=-5,y=-5},{x=5,y=-5},{x=5,y=5},{x=-5,y=5}}) local b=cad.polygon({{x=-3,y=-3},{x=3,y=-3},{x=3,y=3},{x=-3,y=3}}) cad.show('transition',cad.loft({{profile=a,z=0},{profile=b,z=12}}))",
                784.0,
            ),
            (
                "local b=cad.box({width=20,depth=16,height=12}) local f=cad.face(b,{name='opening',normal='z+'}) cad.show('housing',cad.shell(b,{thickness=2,open=f}))",
                1920.0,
            ),
            (
                "cad.show('hollow',cad.shell(cad.box({width=20,depth=16,height=12}),{thickness=2}))",
                2304.0,
            ),
        ] {
            let model = model::compile(source)?;
            let mut bodies = model.build(&kernel)?;
            let body = &mut bodies[0];
            let volume = body.volume()?;
            assert!(
                (volume - expected).abs() < 0.01,
                "volume {volume} expected {expected}"
            );
            let mesh = body.mesh()?.mesh;
            mesh.validate()?;
            assert!((mesh.volume_mm3() - expected).abs() < 0.02);
            let step = body.step_data("shape operation")?;
            assert!(step.contains("CLOSED_SHELL"));
            assert!(step.contains("ISO-10303-21"));
        }
        Ok(())
    }

    #[test]
    fn shape_operations_reject_invalid_paths_sections_and_shells() -> Result<(), String> {
        let polygon = "local p=cad.polygon({{x=0,y=0},{x=4,y=0},{x=4,y=4},{x=0,y=4}}) ";
        for operation in [
            "cad.show('x',cad.sweep(p,{path={{x=0,y=0,z=0},{x=0,y=0,z=0}}}))",
            "cad.show('x',cad.sweep(p,{path={{x=0,y=0,z=0},{x=0,y=0,z=20},{x=20,y=0,z=20},{x=20,y=20,z=30}}}))",
            "cad.show('x',cad.loft({{profile=p,z=10},{profile=p,z=0}}))",
            "cad.show('x',cad.loft({{profile=p,z=0},{profile=p,z=10},bad=true}))",
        ] {
            assert!(model::compile(&format!("{polygon}{operation}")).is_err());
        }
        let kernel = Kernel::new()?;
        let model = model::compile(
            "cad.show('x',cad.shell(cad.box({width=10,depth=10,height=10}),{thickness=6}))",
        )?;
        assert!(model.build(&kernel).is_err());
        Ok(())
    }

    #[test]
    fn shape_operation_cache_reuses_equal_recipes_and_invalidates_shape_changes()
    -> Result<(), String> {
        let kernel = Kernel::new()?;
        let mut cache = model::Cache::new(&kernel);
        for source in [
            "local p=cad.polygon({{x=-2,y=-2},{x=2,y=-2},{x=2,y=2},{x=-2,y=2}}) cad.show('x',cad.sweep(p,{path={{x=0,y=0,z=0},{x=0,y=0,z=20}}}))",
            "local p=cad.polygon({{x=-2,y=-2},{x=2,y=-2},{x=2,y=2},{x=-2,y=2}}) cad.show('x',cad.loft({{profile=p,z=0},{profile=p,z=20}}))",
            "cad.show('x',cad.shell(cad.box({width=20,depth=20,height=20}),{thickness=2}))",
        ] {
            let first = model::compile(source)?;
            let prepared = cache.prepare(&first)?;
            let original = std::sync::Arc::clone(&prepared.geometry[0]);
            drop(prepared);
            let equal = model::compile(source)?;
            let prepared = cache.prepare(&equal)?;
            assert_eq!(prepared.hits, 1);
            assert!(std::sync::Arc::ptr_eq(&original, &prepared.geometry[0]));
            drop(prepared);
            let changed = model::compile(
                &source
                    .replace("z=20", "z=25")
                    .replace("thickness=2", "thickness=3"),
            )?;
            let prepared = cache.prepare(&changed)?;
            assert_eq!(prepared.hits, 0);
            assert!(!std::sync::Arc::ptr_eq(&original, &prepared.geometry[0]));
        }
        Ok(())
    }
}
