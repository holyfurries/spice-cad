use glam::DVec3;
use mlua::Table;

#[derive(Clone, PartialEq)]
pub struct Section {
    pub polygon: crate::profile::Polygon,
    pub position: [f64; 3],
}

#[derive(PartialEq)]
pub enum Recipe {
    Sweep {
        polygon: crate::profile::Polygon,
        path: Vec<[f64; 3]>,
    },
    Loft(Vec<Section>),
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

pub fn path(table: Table, polygon: &crate::profile::Polygon) -> mlua::Result<Vec<[f64; 3]>> {
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
    let mut minimum = DVec3::splat(f64::INFINITY);
    let mut maximum = DVec3::splat(f64::NEG_INFINITY);
    for point in &polygon.points {
        minimum = minimum.min(DVec3::new(point[0], point[1], 0.0));
        maximum = maximum.max(DVec3::new(point[0], point[1], 0.0));
    }
    let radius = (maximum - minimum).length() * 0.5;
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
            if previous.length().min(direction.length()) <= radius * tangent_half * 2.0 {
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
            if distance <= radius * 2.0 {
                return Err(mlua::Error::runtime(
                    "Nonadjacent sweep segments must be separated by more than the profile diameter",
                ));
            }
        }
    }
    Ok(points)
}

#[cfg(test)]
mod tests {
    use crate::{kernel::Kernel, model};

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
