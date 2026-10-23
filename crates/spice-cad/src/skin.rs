use glam::{DVec2, DVec3};
use mlua::{Table, Value};
use std::collections::BTreeMap;

pub const CONTROL_COUNT_MAX: usize = 32768;

#[derive(PartialEq)]
pub struct Skin {
    pub points: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl Skin {
    pub fn from_table(options: Table, remaining: usize) -> mlua::Result<(Self, usize)> {
        crate::model::options_fields(&options, &["sections", "closed", "diagonal"])?;
        let sections: Table = options.get("sections")?;
        let closed = options.get::<Option<bool>>("closed")?.unwrap_or(false);
        let count =
            crate::shape_operation::array_count(&sections, if closed { 3 } else { 2 }, 128)?;
        let first: Table = sections.raw_get(1)?;
        let columns = crate::shape_operation::array_count(&first, 3, 128)?;
        let diagonal_value: Value = options.get("diagonal")?;
        let mut diagonals = Vec::with_capacity(columns);
        if let Value::Table(rules) = diagonal_value {
            if crate::shape_operation::array_count(&rules, 3, 128)? != columns {
                return Err(mlua::Error::runtime(
                    "Skin diagonal rules must match the section control point count",
                ));
            }
            for column in 1..=columns {
                diagonals.push(Diagonal::from_value(rules.raw_get(column)?)?);
            }
        } else {
            diagonals.resize(columns, Diagonal::from_value(diagonal_value)?);
        }
        let controls = count * columns;
        if controls > remaining {
            return Err(mlua::Error::runtime(
                "Model exceeds 32768 skin control points",
            ));
        }
        let mut points = Vec::<[f64; 3]>::with_capacity(controls);
        let mut unique = BTreeMap::<[i64; 3], u32>::new();
        let mut indices = Vec::with_capacity(controls);
        for index in 1..=count {
            let section: Table = sections.raw_get(index)?;
            if crate::shape_operation::array_count(&section, 3, 128)? != columns {
                return Err(mlua::Error::runtime(
                    "Skin sections must have equal control point counts",
                ));
            }
            for column in 1..=columns {
                let point: Table = section.raw_get(column)?;
                crate::model::options_fields(&point, &["x", "y", "z"])?;
                let value = [point.get::<f64>("x")?, point.get("y")?, point.get("z")?];
                if value.iter().any(|v| !v.is_finite() || v.abs() > 2000.0) {
                    return Err(mlua::Error::runtime(
                        "Skin coordinates must be finite and within ±2000 mm",
                    ));
                }
                let next = points.len() as u32;
                let vertex = *unique
                    .entry(value.map(|v| (v * 1e9).round() as i64))
                    .or_insert_with(|| {
                        points.push(value);
                        next
                    });
                indices.push(vertex);
            }
        }
        let mut triangles = Vec::with_capacity(controls * 2);
        for section in 0..count - usize::from(!closed) {
            let next = (section + 1) % count;
            for column in 0..columns {
                let after = (column + 1) % columns;
                let [a, b, c, d] = [
                    indices[section * columns + column],
                    indices[section * columns + after],
                    indices[next * columns + after],
                    indices[next * columns + column],
                ];
                let point = |i: u32| DVec3::from_array(points[i as usize]);
                let forward = match diagonals[column] {
                    Diagonal::Forward => true,
                    Diagonal::Backward => false,
                    Diagonal::Shortest => {
                        point(a).distance_squared(point(c)) <= point(b).distance_squared(point(d))
                    }
                };
                let faces = if forward {
                    [[a, b, c], [a, c, d]]
                } else {
                    [[a, b, d], [b, c, d]]
                };
                for face in faces {
                    if face[0] == face[1] || face[1] == face[2] || face[2] == face[0] {
                        continue;
                    }
                    let [a, b, c] = face.map(point);
                    if (b - a).cross(c - a).length_squared() <= 1e-20 {
                        return Err(mlua::Error::runtime(
                            "Skin cells must not contain collinear triangles",
                        ));
                    }
                    triangles.push(face);
                }
            }
        }
        if !closed {
            let mut first = indices[..columns].to_vec();
            first.reverse();
            cap(&points, first, &mut triangles).map_err(mlua::Error::runtime)?;
            cap(
                &points,
                indices[(count - 1) * columns..].to_vec(),
                &mut triangles,
            )
            .map_err(mlua::Error::runtime)?;
        }
        if triangles.len() < 4 {
            return Err(mlua::Error::runtime("Skin must enclose a nonzero volume"));
        }
        let mut edges = BTreeMap::<(u32, u32), (u32, i32)>::new();
        let mut volume = 0.0;
        let origin = DVec3::from_array(points[0]);
        for face in &triangles {
            let [a, b, c] = face.map(|i| DVec3::from_array(points[i as usize]) - origin);
            volume += a.dot(b.cross(c)) / 6.0;
            for corner in 0..3 {
                let a = face[corner];
                let b = face[(corner + 1) % 3];
                let edge = edges.entry((a.min(b), a.max(b))).or_default();
                edge.0 += 1;
                edge.1 += if a < b { 1 } else { -1 };
            }
        }
        if edges
            .values()
            .any(|&(count, orientation)| count != 2 || orientation != 0)
        {
            return Err(mlua::Error::runtime(
                "Skin sections do not form a closed, consistently oriented manifold",
            ));
        }
        if volume.abs() <= 1e-8 {
            return Err(mlua::Error::runtime("Skin must enclose a nonzero volume"));
        }
        if volume < 0.0 {
            for face in &mut triangles {
                face.swap(1, 2);
            }
        }
        assert!(points.len() <= controls);
        assert!(triangles.len() <= 32768);
        Ok((Self { points, triangles }, controls))
    }
}

#[derive(Clone, Copy)]
enum Diagonal {
    Shortest,
    Forward,
    Backward,
}

impl Diagonal {
    fn from_value(value: Value) -> mlua::Result<Self> {
        match value {
            Value::Nil => Ok(Self::Shortest),
            Value::String(value) => match value.to_str()?.as_ref() {
                "shortest" => Ok(Self::Shortest),
                "forward" => Ok(Self::Forward),
                "backward" => Ok(Self::Backward),
                _ => Err(mlua::Error::runtime(
                    "Skin diagonal must be shortest, forward, or backward",
                )),
            },
            _ => Err(mlua::Error::runtime(
                "Skin diagonal must be a rule or one rule per section edge",
            )),
        }
    }
}

fn cap(
    points: &[[f64; 3]],
    mut outline: Vec<u32>,
    triangles: &mut Vec<[u32; 3]>,
) -> Result<(), String> {
    assert!((3..=128).contains(&outline.len()));
    assert!(!points.is_empty());
    outline.dedup();
    if outline.first() == outline.last() {
        outline.pop();
    }
    if outline.len() < 3 {
        return Ok(());
    }
    let mut normal = DVec3::ZERO;
    let origin = DVec3::from_array(points[outline[0] as usize]);
    for index in 0..outline.len() {
        normal += (DVec3::from_array(points[outline[index] as usize]) - origin).cross(
            DVec3::from_array(points[outline[(index + 1) % outline.len()] as usize]) - origin,
        );
    }
    if normal.length_squared() <= 1e-18 {
        return Err("Skin end section has zero projected area".into());
    }
    let axis = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() {
        0
    } else if normal.y.abs() >= normal.z.abs() {
        1
    } else {
        2
    };
    let project = |index: u32| {
        let p = points[index as usize];
        match axis {
            0 => DVec2::new(p[1], p[2]),
            1 => DVec2::new(p[2], p[0]),
            _ => DVec2::new(p[0], p[1]),
        }
    };
    let sign = normal[axis].signum();
    for _ in 0..128 {
        if outline.len() == 3 {
            triangles.push([outline[0], outline[1], outline[2]]);
            return Ok(());
        }
        let mut ear: Option<(usize, [u32; 3], f64, [f64; 3])> = None;
        for index in 0..outline.len() {
            let indices = [
                (index + outline.len() - 1) % outline.len(),
                index,
                (index + 1) % outline.len(),
            ];
            let [a, b, c] = indices.map(|i| project(outline[i]));
            if (b - a).perp_dot(c - a) * sign <= 1e-14 {
                continue;
            }
            let contains = outline.iter().enumerate().any(|(i, &v)| {
                if indices.contains(&i) {
                    return false;
                }
                let p = project(v);
                (b - a).perp_dot(p - a) * sign >= -1e-14
                    && (c - b).perp_dot(p - b) * sign >= -1e-14
                    && (a - c).perp_dot(p - c) * sign >= -1e-14
            });
            if !contains {
                let face = indices.map(|i| outline[i]);
                let length = DVec3::from_array(points[face[0] as usize])
                    .distance_squared(DVec3::from_array(points[face[2] as usize]));
                let anchor = points[face[1] as usize];
                let replace = match &ear {
                    Some((_, _, best, position)) => {
                        length < *best || length == *best && anchor < *position
                    }
                    None => true,
                };
                if replace {
                    ear = Some((index, face, length, anchor));
                }
            }
        }
        let Some((index, face, _, _)) = ear else {
            return Err("Skin end section overlaps itself or cannot be capped".into());
        };
        triangles.push(face);
        outline.remove(index);
    }
    Err("Skin cap exceeds its control point budget".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_strip_diagonals_preserve_topology_when_controls_move() -> mlua::Result<()> {
        let lua = mlua::Lua::new();
        let source = "return {sections={{{x=0,y=0,z=0},{x=2,y=0,z=0},{x=0,y=2,z=0}},{{x=0,y=0,z=2},{x=3,y=0,z=3},{x=0,y=3,z=2}}},diagonal=RULE}";
        let skin = |rule: &str, moved: bool| -> mlua::Result<Skin> {
            let source = source.replace("RULE", rule);
            let source = if moved {
                source.replace("x=3,y=0,z=3", "x=4,y=0,z=4")
            } else {
                source
            };
            Ok(Skin::from_table(lua.load(&source).eval()?, CONTROL_COUNT_MAX)?.0)
        };
        let forward = skin("'forward'", false)?;
        let backward = skin("'backward'", false)?;
        let mixed = skin("{'forward','backward','forward'}", false)?;
        assert_ne!(forward.triangles, backward.triangles);
        assert_ne!(mixed.triangles, forward.triangles);
        assert_ne!(mixed.triangles, backward.triangles);
        for rule in [
            "'forward'",
            "'backward'",
            "{'forward','backward','forward'}",
        ] {
            let before = skin(rule, false)?;
            let after = skin(rule, true)?;
            assert_eq!(before.triangles, after.triangles);
            assert_ne!(before.points, after.points);
        }
        for rule in [
            "'invalid'",
            "true",
            "{'forward','backward'}",
            "{'forward','invalid','backward'}",
        ] {
            assert!(skin(rule, false).is_err(), "{rule}");
        }
        Ok(())
    }

    #[test]
    fn control_arrays_and_resource_bounds_are_checked_before_meshing() -> mlua::Result<()> {
        let lua = mlua::Lua::new();
        let source = "local sections={{{x=0,y=0,z=0},{x=1,y=0,z=0},{x=0,y=1,z=0}},{{x=0,y=0,z=1},{x=1,y=0,z=1},{x=0,y=1,z=1}}} return {sections=sections}";
        let options = lua.load(source).eval::<Table>()?;
        let (skin, count) = Skin::from_table(options.clone(), CONTROL_COUNT_MAX)?;
        assert_eq!(count, 6);
        assert_eq!(skin.triangles.len(), 8);
        assert!(Skin::from_table(options, 5).is_err());
        for source in [
            source.replace(
                "return {sections=sections}",
                "table.remove(sections) return {sections=sections}",
            ),
            source.replace(
                "return {sections=sections}",
                "table.remove(sections[2]) return {sections=sections}",
            ),
            source.replace(
                "return {sections=sections}",
                "sections[4]=sections[1] return {sections=sections}",
            ),
            source.replace("x=1,y=0,z=1", "x=1,y=0,z=1,unknown=true"),
            source.replace(
                "return {sections=sections}",
                "for i=3,129 do sections[i]=sections[1] end return {sections=sections}",
            ),
            source.replace(
                "return {sections=sections}",
                "for i=4,129 do sections[1][i]=sections[1][1] end return {sections=sections}",
            ),
        ] {
            assert!(
                Skin::from_table(lua.load(&source).eval::<Table>()?, CONTROL_COUNT_MAX).is_err(),
                "{source}"
            );
        }
        Ok(())
    }
}
