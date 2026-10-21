use glam::DVec2;
use mlua::Table;
use std::f64::consts::{PI, TAU};

const TOLERANCE: f64 = 1e-7;
pub const LOOP_COUNT_MAX: usize = 16;

#[derive(Clone, PartialEq)]
pub enum Profile {
    Polygon(super::Polygon),
    Curves(Vec<Segment>),
    Region(Vec<Profile>),
}

#[derive(Clone, PartialEq)]
pub struct Segment {
    pub start: DVec2,
    pub end: DVec2,
    pub center: Option<DVec2>,
    pub angle: f64,
}

impl Segment {
    pub fn radius(&self) -> f64 {
        self.start.distance(self.center.expect("Arc center"))
    }

    pub fn start_angle(&self) -> f64 {
        let radial = self.start - self.center.expect("Arc center");
        radial.y.atan2(radial.x)
    }

    fn point(&self, parameter: f64) -> DVec2 {
        match self.center {
            Some(center) => {
                let angle = self.start_angle() + self.angle * parameter;
                center + self.radius() * DVec2::new(angle.cos(), angle.sin())
            }
            None => self.start.lerp(self.end, parameter),
        }
    }

    fn parameter(&self, point: DVec2) -> Option<f64> {
        if let Some(center) = self.center {
            if (point.distance(center) - self.radius()).abs() > TOLERANCE {
                return None;
            }
            let radial = point - center;
            let angle = radial.y.atan2(radial.x);
            let distance = if self.angle > 0.0 {
                (angle - self.start_angle()).rem_euclid(TAU)
            } else {
                (self.start_angle() - angle).rem_euclid(TAU)
            };
            if point.distance(self.start) < TOLERANCE {
                return Some(0.0);
            }
            let parameter = distance / self.angle.abs();
            return (parameter <= 1.0 + TOLERANCE).then_some(parameter);
        }
        let direction = self.end - self.start;
        let parameter = (point - self.start).dot(direction) / direction.length_squared();
        if !(-TOLERANCE..=1.0 + TOLERANCE).contains(&parameter) {
            return None;
        }
        (self.point(parameter).distance(point) < TOLERANCE).then_some(parameter)
    }

    fn area_twice(&self) -> f64 {
        match self.center {
            Some(center) => {
                center.perp_dot(self.end - self.start) + self.radius().powi(2) * self.angle
            }
            None => self.start.perp_dot(self.end),
        }
    }
}

impl Profile {
    pub fn face_coordinates(&self, axis: usize, positive: bool) -> Self {
        match self {
            Self::Region(loops) => Self::Region(
                loops
                    .iter()
                    .map(|boundary| boundary.face_coordinates_loop(axis, positive))
                    .collect(),
            ),
            _ => self.face_coordinates_loop(axis, positive),
        }
    }

    fn face_coordinates_loop(&self, axis: usize, positive: bool) -> Self {
        let map = |point: DVec2| match (axis, positive) {
            (0, true) => DVec2::new(-point.y, point.x),
            (0, false) => DVec2::new(point.y, point.x),
            (1, true) | (2, false) => DVec2::new(point.x, -point.y),
            (1, false) | (2, true) => point,
            _ => unreachable!(),
        };
        let reflected = matches!((axis, positive), (0, false) | (1, true) | (2, false));
        let mut segments = self.segments();
        for segment in &mut segments {
            segment.start = map(segment.start);
            segment.end = map(segment.end);
            segment.center = segment.center.map(map);
            if reflected {
                segment.angle = -segment.angle;
            }
        }
        if reflected {
            segments.reverse();
            for segment in &mut segments {
                std::mem::swap(&mut segment.start, &mut segment.end);
                segment.angle = -segment.angle;
            }
        }
        Self::Curves(segments)
    }

    pub fn polygon(&self) -> Result<&super::Polygon, String> {
        match self {
            Self::Polygon(polygon) => Ok(polygon),
            _ => Err("This operation requires a single straight-sided polygon profile".into()),
        }
    }

    pub fn circle(table: Table) -> mlua::Result<Self> {
        crate::model::options_fields(&table, &["radius", "x", "y"])?;
        let radius: f64 = table.get("radius")?;
        let center = DVec2::new(
            table.get::<Option<f64>>("x")?.unwrap_or(0.0),
            table.get::<Option<f64>>("y")?.unwrap_or(0.0),
        );
        if !radius.is_finite() || !(0.01..=500.0).contains(&radius) {
            return Err(error("Circle radius must be between 0.01 and 500 mm"));
        }
        let right = center + DVec2::X * radius;
        let left = center - DVec2::X * radius;
        Self::curves(vec![
            Segment {
                start: right,
                end: left,
                center: Some(center),
                angle: PI,
            },
            Segment {
                start: left,
                end: right,
                center: Some(center),
                angle: PI,
            },
        ])
        .map_err(error)
    }

    pub fn path(table: Table) -> mlua::Result<Self> {
        crate::model::options_fields(&table, &["start", "segments"])?;
        let start = point(table.get("start")?)?;
        let entries: Table = table.get("segments")?;
        let count = dense_count(&entries, 2, super::POINT_COUNT_MAX)?;
        let mut segments = Vec::with_capacity(count);
        let mut previous = start;
        for index in 1..=count {
            let entry: Table = entries.raw_get(index)?;
            crate::model::options_fields(&entry, &["to", "center", "clockwise"])?;
            let end = point(entry.get("to")?)?;
            let center = entry
                .get::<Option<Table>>("center")?
                .map(point)
                .transpose()?;
            let clockwise = entry.get::<Option<bool>>("clockwise")?.unwrap_or(false);
            let angle = match center {
                Some(center) => {
                    let radius = previous.distance(center);
                    if radius < 0.01
                        || radius > 1000.0
                        || (end.distance(center) - radius).abs() > TOLERANCE
                    {
                        return Err(error(
                            "Arc endpoints must have the same radius (0.01–1000 mm) from center",
                        ));
                    }
                    let first = previous - center;
                    let last = end - center;
                    let difference = last.y.atan2(last.x) - first.y.atan2(first.x);
                    if clockwise {
                        -(-difference).rem_euclid(TAU)
                    } else {
                        difference.rem_euclid(TAU)
                    }
                }
                None => {
                    if clockwise {
                        return Err(error("clockwise requires an arc center"));
                    }
                    0.0
                }
            };
            segments.push(Segment {
                start: previous,
                end,
                center,
                angle,
            });
            previous = end;
        }
        if previous.distance(start) > TOLERANCE {
            return Err(error(
                "A path must close: its final endpoint must equal start",
            ));
        }
        Self::curves(segments).map_err(error)
    }

    fn curves(mut segments: Vec<Segment>) -> Result<Self, String> {
        assert!((2..=super::POINT_COUNT_MAX).contains(&segments.len()));
        let mut minimum = DVec2::splat(f64::INFINITY);
        let mut maximum = DVec2::splat(f64::NEG_INFINITY);
        for (index, segment) in segments.iter().enumerate() {
            if !segment.start.is_finite()
                || !segment.end.is_finite()
                || segment.start.abs().max_element() > 2000.0
            {
                return Err("Profile coordinates must be finite and within ±2000 mm".into());
            }
            if segment.start.distance(segment.end) < 0.01 {
                return Err("Path segment endpoints must be at least 0.01 mm apart; use cad.circle for a full circle".into());
            }
            let mut parameters = vec![0.0, 1.0];
            if let Some(center) = segment.center {
                if !center.is_finite() || center.abs().max_element() > 2000.0 {
                    return Err("Arc centers must be finite and within ±2000 mm".into());
                }
                for axis in [DVec2::X, DVec2::Y, -DVec2::X, -DVec2::Y] {
                    if let Some(parameter) = segment.parameter(center + axis * segment.radius()) {
                        parameters.push(parameter);
                    }
                }
            }
            for parameter in parameters {
                minimum = minimum.min(segment.point(parameter));
                maximum = maximum.max(segment.point(parameter));
            }
            for (other_index, other) in segments.iter().enumerate().skip(index + 1) {
                let adjacent =
                    other_index == index + 1 || (index == 0 && other_index == segments.len() - 1);
                for intersection in intersections(segment, other) {
                    let shared_endpoint = [segment.start, segment.end].iter().any(|first| {
                        [other.start, other.end].iter().any(|second| {
                            first.distance(*second) < TOLERANCE
                                && first.distance(intersection) < TOLERANCE
                        })
                    });
                    if !adjacent || !shared_endpoint {
                        return Err("Path segments must not intersect, overlap, or touch except at consecutive endpoints".into());
                    }
                }
            }
        }
        if minimum.abs().max_element() > 2000.0
            || maximum.abs().max_element() > 2000.0
            || (maximum - minimum).max_element() > 1000.0
        {
            return Err(
                "Profile extents must be within ±2000 mm and no larger than 1000 mm".into(),
            );
        }
        let area: f64 = segments.iter().map(Segment::area_twice).sum();
        if area.abs() < 0.0002 {
            return Err("Profile area must be at least 0.0001 mm²".into());
        }
        if area < 0.0 {
            segments.reverse();
            for segment in &mut segments {
                std::mem::swap(&mut segment.start, &mut segment.end);
                segment.angle = -segment.angle;
            }
        }
        Ok(Self::Curves(segments))
    }

    pub fn region(loops: Vec<Self>) -> Result<Self, String> {
        if !(1..=LOOP_COUNT_MAX).contains(&loops.len()) {
            return Err("A profile needs 1–16 loops".into());
        }
        if loops
            .iter()
            .any(|profile| matches!(profile, Self::Region(_)))
        {
            return Err("Profile loops must be circles, paths, or polygons; list nested holes and islands together".into());
        }
        let boundaries: Vec<_> = loops.iter().map(Self::segments).collect();
        for first in 0..boundaries.len() {
            for second in first + 1..boundaries.len() {
                for a in &boundaries[first] {
                    for b in &boundaries[second] {
                        if !intersections(a, b).is_empty() {
                            return Err("Profile loops must not intersect or touch".into());
                        }
                    }
                }
            }
        }
        Ok(Self::Region(loops))
    }

    pub fn segments(&self) -> Vec<Segment> {
        match self {
            Self::Polygon(polygon) => polygon
                .points
                .iter()
                .enumerate()
                .map(|(index, &start)| Segment {
                    start: DVec2::from_array(start),
                    end: DVec2::from_array(polygon.points[(index + 1) % polygon.points.len()]),
                    center: None,
                    angle: 0.0,
                })
                .collect(),
            Self::Curves(segments) => {
                let mut result = Vec::with_capacity(segments.len() * 2);
                for segment in segments {
                    // SMLib closes arcs within 0.01° of a full turn; splitting preserves their endpoints.
                    if segment.angle.abs() > PI {
                        let middle = segment.point(0.5);
                        let mut first = segment.clone();
                        first.end = middle;
                        first.angle *= 0.5;
                        let mut second = segment.clone();
                        second.start = middle;
                        second.angle *= 0.5;
                        result.extend([first, second]);
                    } else {
                        result.push(segment.clone());
                    }
                }
                result
            }
            Self::Region(_) => unreachable!("A region is not a single loop"),
        }
    }

    pub fn contains(&self, point: DVec2) -> bool {
        let mut crossings = 0;
        for segment in self.segments() {
            let mut parameters = vec![0.0, 1.0];
            if let Some(center) = segment.center {
                for direction in [DVec2::Y, -DVec2::Y] {
                    if let Some(parameter) =
                        segment.parameter(center + direction * segment.radius())
                    {
                        if parameter > TOLERANCE && parameter < 1.0 - TOLERANCE {
                            parameters.push(parameter);
                        }
                    }
                }
            }
            parameters.sort_by(f64::total_cmp);
            for interval in parameters.windows(2) {
                let first = segment.point(interval[0]);
                let last = segment.point(interval[1]);
                if (first.y > point.y) == (last.y > point.y) {
                    continue;
                }
                let x = if let Some(center) = segment.center {
                    let distance = (segment.radius().powi(2) - (point.y - center.y).powi(2))
                        .max(0.0)
                        .sqrt();
                    let middle = segment.point((interval[0] + interval[1]) * 0.5);
                    center.x
                        + if middle.x >= center.x {
                            distance
                        } else {
                            -distance
                        }
                } else {
                    first.x + (point.y - first.y) * (last.x - first.x) / (last.y - first.y)
                };
                if x > point.x {
                    crossings += 1;
                }
            }
        }
        crossings % 2 == 1
    }

    pub fn validate_revolve(&self) -> Result<(), String> {
        match self {
            Self::Polygon(_) => self.polygon()?.validate_revolve(),
            Self::Curves(_) | Self::Region(_) => Err("Curved and nested profiles currently support extrusion; revolve requires a polygon".into()),
        }
    }
}

fn intersections(first: &Segment, second: &Segment) -> Vec<DVec2> {
    let mut candidates = Vec::with_capacity(8);
    match (first.center, second.center) {
        (None, None) => {
            let a = first.end - first.start;
            let b = second.end - second.start;
            let cross = a.perp_dot(b);
            if cross.abs() > TOLERANCE {
                candidates.push(first.start + a * (second.start - first.start).perp_dot(b) / cross);
            } else {
                candidates.extend([
                    first.start,
                    first.end,
                    second.start,
                    second.end,
                    first.point(0.5),
                    second.point(0.5),
                ]);
            }
        }
        (Some(center), None) | (None, Some(center)) => {
            let (arc, line) = if first.center.is_some() {
                (first, second)
            } else {
                (second, first)
            };
            let direction = line.end - line.start;
            let relative = line.start - center;
            let a = direction.length_squared();
            let b = 2.0 * relative.dot(direction);
            let c = relative.length_squared() - arc.radius().powi(2);
            let discriminant = b * b - 4.0 * a * c;
            if discriminant >= -TOLERANCE {
                for sign in [-1.0, 1.0] {
                    candidates.push(
                        line.start
                            + direction * ((-b + sign * discriminant.max(0.0).sqrt()) / (2.0 * a)),
                    );
                }
            }
        }
        (Some(a), Some(b)) => {
            let distance = a.distance(b);
            if distance < TOLERANCE && (first.radius() - second.radius()).abs() < TOLERANCE {
                candidates.extend([
                    first.start,
                    first.end,
                    second.start,
                    second.end,
                    first.point(0.5),
                    second.point(0.5),
                ]);
            } else if distance > TOLERANCE {
                let along = (first.radius().powi(2) - second.radius().powi(2) + distance.powi(2))
                    / (2.0 * distance);
                let height_squared = first.radius().powi(2) - along.powi(2);
                if height_squared >= -TOLERANCE {
                    let direction = (b - a) / distance;
                    let middle = a + along * direction;
                    let offset =
                        height_squared.max(0.0).sqrt() * DVec2::new(-direction.y, direction.x);
                    candidates.extend([middle + offset, middle - offset]);
                }
            }
        }
    }
    candidates
        .retain(|point| first.parameter(*point).is_some() && second.parameter(*point).is_some());
    candidates
}

fn point(table: Table) -> mlua::Result<DVec2> {
    crate::model::options_fields(&table, &["x", "y"])?;
    let point = DVec2::new(table.get("x")?, table.get("y")?);
    if !point.is_finite() || point.abs().max_element() > 2000.0 {
        return Err(error(
            "Profile coordinates must be finite and within ±2000 mm",
        ));
    }
    Ok(point)
}

pub fn dense_count(table: &Table, minimum: usize, maximum: usize) -> mlua::Result<usize> {
    let count = table.raw_len();
    if !(minimum..=maximum).contains(&count) {
        return Err(error(format!("Expected {minimum}–{maximum} entries")));
    }
    let mut entries = 0;
    for pair in table.clone().pairs::<mlua::Value, mlua::Value>() {
        let (key, _) = pair?;
        if !matches!(key, mlua::Value::Integer(index) if index >= 1 && index as usize <= count) {
            return Err(error("Expected a dense array"));
        }
        entries += 1;
    }
    if entries != count {
        return Err(error("Expected a dense array"));
    }
    Ok(count)
}

fn error(message: impl Into<String>) -> mlua::Error {
    mlua::Error::RuntimeError(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_circles_arcs_and_nested_loops_preserve_volume_and_closed_meshes() -> Result<(), String>
    {
        let capsule = "local p=cad.path({start={x=0,y=-5},segments={{to={x=20,y=-5}},{to={x=20,y=5},center={x=20,y=0}},{to={x=0,y=5}},{to={x=0,y=-5},center={x=0,y=0}}}})";
        let cases = [
            ("local p=cad.circle({radius=10})".to_string(), 100.0 * PI),
            (capsule.to_string(), 200.0 + 25.0 * PI),
            ("local p=cad.path({start={x=0,y=10},segments={{to={x=10,y=0},center={x=0,y=0}},{to={x=0,y=10}}}})".to_string(), 50.0 * (1.5 * PI + 1.0)),
            ("local a=math.rad(0.009);local p=cad.path({start={x=100,y=0},segments={{to={x=100*math.cos(a),y=-100*math.sin(a)},center={x=0,y=0}},{to={x=100,y=0}}}})".to_string(), 5000.0 * (TAU - 0.009_f64.to_radians() + 0.009_f64.to_radians().sin())),
            ("local p=cad.path({start={x=0,y=0},segments={{to={x=20,y=0}},{to={x=20,y=20}},{to={x=0,y=20}},{to={x=0,y=0},center={x=0,y=10},clockwise=true}}})".to_string(), 400.0 - 50.0 * PI),
            ("local p=cad.path({start={x=0,y=-10},segments={{to={x=0,y=10},center={x=0,y=0},clockwise=true},{to={x=0,y=-10}}}})".to_string(), 50.0 * PI),
            ("local p=cad.profile({loops={cad.circle({radius=2}),cad.circle({radius=10}),cad.circle({radius=6})}})".to_string(), 68.0 * PI),
            ("local p=cad.profile({loops={cad.polygon({{x=-10,y=-10},{x=10,y=-10},{x=10,y=10},{x=-10,y=10}}),cad.circle({radius=3}),cad.circle({radius=2,x=6})}})".to_string(), 400.0 - 13.0 * PI),
        ];
        for (prefix, area) in cases {
            let snapshot = crate::build_script_snapshot(
                &format!("{prefix};cad.show('curved',cad.extrude(p,{{height=4}}))"),
                1,
            )?;
            let expected = area * 4.0;
            assert!(
                (snapshot.volume_mm3 - expected).abs() < expected * 1e-6,
                "{} != {expected}",
                snapshot.volume_mm3
            );
            snapshot.scene.world_mesh().validate()?;
            assert!((snapshot.scene.bounds[1].z - 4.0).abs() < 0.001);
        }
        Ok(())
    }

    #[test]
    fn rejects_invalid_curves_and_intersecting_or_touching_loops_before_kernel() {
        for source in [
            "local p=cad.circle({radius=0})",
            "local p=cad.circle({radius=0/0})",
            "local p=cad.circle({radius=501})",
            "local p=cad.circle({radius=4,x=2000})",
            "local p=cad.circle({radius=4,bogus=true})",
            "local p=cad.path({start={x=0,y=0},segments={{to={x=10,y=0}},{to={x=10,y=10}}}})",
            "local p=cad.path({start={x=0,y=0},segments={{to={x=10,y=0},center={x=4,y=0}},{to={x=0,y=0}}}})",
            "local p=cad.path({start={x=0,y=0},segments={{to={x=10,y=0},clockwise=true},{to={x=0,y=0}}}})",
            "local p=cad.path({start={x=0,y=0},segments={{to={x=10,y=10}},{to={x=0,y=10}},{to={x=10,y=0}},{to={x=0,y=0}}}})",
            "local p=cad.profile({loops={cad.circle({radius=10}),cad.circle({radius=2,x=8})}})",
            "local p=cad.profile({loops={cad.circle({radius=10}),cad.circle({radius=6,x=8})}})",
            "local p=cad.profile({loops={cad.circle({radius=10}),cad.circle({radius=10})}})",
            "local p=cad.profile({loops={cad.circle({radius=10}),cad.polygon({{x=9,y=-1},{x=11,y=-1},{x=11,y=1},{x=9,y=1}})}})",
            "local p=cad.profile({loops={cad.profile({loops={cad.circle({radius=2})}})}})",
            "cad.show('bad',cad.revolve(cad.circle({radius=2,x=4}),{}))",
        ] {
            assert!(crate::model::compile(source).is_err(), "Accepted {source}");
        }
    }

    #[test]
    fn curved_face_features_work_on_each_axis_and_sign() -> Result<(), String> {
        for normal in ["x+", "x-", "y+", "y-", "z+", "z-"] {
            let source = format!(
                "local body=cad.box({{width=20,depth=20,height=20}});local face=cad.face(body,{{name='surface',normal='{normal}'}});local p=cad.circle({{radius=3}});cad.show('pocket',cad.pocket(body,{{face=face,profile=p,depth=2}}))"
            );
            let snapshot = crate::build_script_snapshot(&source, 1)?;
            let expected = 8000.0 - PI * 9.0 * 2.0;
            assert!(
                (snapshot.volume_mm3 - expected).abs() < expected * 1e-6,
                "{normal}: {} != {expected}",
                snapshot.volume_mm3
            );
            snapshot.scene.world_mesh().validate()?;
        }
        Ok(())
    }

    #[test]
    fn nested_curve_export_contains_closed_print_mesh_and_step_data() -> Result<(), String> {
        let source = include_str!("../../models/rounded-enclosure.luau");
        let model = crate::model::compile(source)?;
        let kernel = crate::kernel::Kernel::new()?;
        for mut body in model.build(&kernel)? {
            body.mesh()?.mesh.validate()?;
            assert!(body.volume()? > 0.0);
            let step = body.step_data("rounded-enclosure")?;
            assert!(step.len() > 1024);
            assert!(step.contains("ISO-10303-21"));
        }
        drop(kernel);
        let folder = std::env::temp_dir().join(format!(
            "spice-curved-profile-export-{}",
            std::process::id()
        ));
        crate::export::model_files(source, &folder, crate::export::Target::Print)?;
        let stl = std::fs::read(folder.join("model.stl")).map_err(|error| error.to_string())?;
        assert!(stl.len() >= 84);
        let triangles = u32::from_le_bytes(stl[80..84].try_into().expect("STL triangle count"));
        assert!(triangles > 0);
        assert_eq!(stl.len(), 84 + triangles as usize * 50);
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }
}
