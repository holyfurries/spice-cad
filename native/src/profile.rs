use glam::DVec2;
use mlua::Table;

pub const POINT_COUNT_MAX: usize = 128;

pub struct Polygon {
    pub points: Vec<[f64; 2]>,
}

impl Polygon {
    pub fn validate_revolve(&self) -> Result<(), String> {
        assert!((3..=POINT_COUNT_MAX).contains(&self.points.len()));
        if self
            .points
            .iter()
            .any(|point| !(0.0..=1000.0).contains(&point[0]))
        {
            return Err("Revolve profile X is radius and must be between 0 and 1000 mm; profiles must not cross the rotation axis".into());
        }
        Ok(())
    }
    pub fn from_table(table: Table) -> mlua::Result<Self> {
        let count = table.raw_len();
        if !(3..=POINT_COUNT_MAX).contains(&count) {
            return Err(mlua::Error::RuntimeError(
                "A polygon needs 3–128 points; do not repeat the first point".into(),
            ));
        }
        let mut entries = 0;
        for pair in table.clone().pairs::<mlua::Value, mlua::Value>() {
            let (key, _) = pair?;
            if !matches!(key, mlua::Value::Integer(index) if index >= 1 && index as usize <= count)
            {
                return Err(mlua::Error::RuntimeError(
                    "Polygon points must be a dense array".into(),
                ));
            }
            entries += 1;
        }
        if entries != count {
            return Err(mlua::Error::RuntimeError(
                "Polygon points must be a dense array".into(),
            ));
        }
        let mut points = Vec::with_capacity(count);
        for index in 1..=count {
            let point: Table = table.raw_get(index)?;
            crate::model::options_fields(&point, &["x", "y"])?;
            points.push([point.get("x")?, point.get("y")?]);
        }
        Self::new(points).map_err(mlua::Error::RuntimeError)
    }

    pub fn new(mut points: Vec<[f64; 2]>) -> Result<Self, String> {
        if !(3..=POINT_COUNT_MAX).contains(&points.len()) {
            return Err("A polygon needs 3–128 points".into());
        }
        if points
            .iter()
            .flatten()
            .any(|value| !value.is_finite() || value.abs() > 2000.0)
        {
            return Err("Polygon coordinates must be finite and within ±2000 mm".into());
        }
        let count = points.len();
        let mut minimum = DVec2::splat(f64::INFINITY);
        let mut maximum = DVec2::splat(f64::NEG_INFINITY);
        let mut area_twice = 0.0;
        let origin = DVec2::from_array(points[0]);
        for index in 0..count {
            let first = DVec2::from_array(points[index]);
            let second = DVec2::from_array(points[(index + 1) % count]);
            let previous = DVec2::from_array(points[(index + count - 1) % count]);
            if first.distance(second) < 0.01 {
                return Err(
                    "Polygon edges must be at least 0.01 mm; do not repeat the first point".into(),
                );
            }
            let incoming = first - previous;
            let outgoing = second - first;
            if incoming.perp_dot(outgoing).abs() <= 1e-9 {
                if incoming.dot(outgoing) < 0.0 {
                    return Err("Polygon edges must not double back or overlap".into());
                }
            }
            minimum = minimum.min(first);
            maximum = maximum.max(first);
            area_twice += (first - origin).perp_dot(second - origin);
            for other in index + 1..count {
                if other == index + 1 || (index == 0 && other == count - 1) {
                    continue;
                }
                let third = DVec2::from_array(points[other]);
                let fourth = DVec2::from_array(points[(other + 1) % count]);
                if polygon_segments_intersect(first, second, third, fourth) {
                    return Err(
                        "Polygon edges must not intersect or touch except at consecutive endpoints"
                            .into(),
                    );
                }
            }
        }
        if (maximum - minimum).max_element() > 1000.0 {
            return Err("Polygon width and depth must not exceed 1000 mm".into());
        }
        if area_twice.abs() < 0.0002 {
            return Err("Polygon area must be at least 0.0001 mm²".into());
        }
        if area_twice < 0.0 {
            points.reverse();
        }
        assert!(points.len() >= 3);
        assert!(points.len() <= POINT_COUNT_MAX);
        Ok(Self { points })
    }
}

fn polygon_segments_intersect(first: DVec2, second: DVec2, third: DVec2, fourth: DVec2) -> bool {
    let tolerance = 1e-9;
    let lower = first.min(second).max(third.min(fourth));
    let upper = first.max(second).min(third.max(fourth));
    if (lower - upper).max_element() > tolerance {
        return false;
    }
    let side = |a: DVec2, b: DVec2, c: DVec2| {
        let cross = (b - a).perp_dot(c - a);
        if cross.abs() <= tolerance {
            0.0
        } else {
            cross.signum()
        }
    };
    side(first, second, third) * side(first, second, fourth) <= 0.0
        && side(third, fourth, first) * side(third, fourth, second) <= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_and_partial_revolves_have_correct_volume_bounds_and_closed_meshes() -> Result<(), String>
    {
        for inner in [0.0, 5.0] {
            for angle in [90.0, 270.0, 360.0] {
                let source = format!(
                    "local p=cad.polygon({{{{x={inner},y=0}},{{x=10,y=0}},{{x=10,y=20}},{{x={inner},y=20}}}});cad.show('ring',cad.revolve(p,{{angle={angle}}}))"
                );
                let snapshot = crate::build_script_snapshot(&source, 1)?;
                let expected =
                    std::f64::consts::PI * (100.0 - inner * inner) * 20.0 * angle / 360.0;
                assert!(
                    (snapshot.volume_mm3 - expected).abs() < expected * 1e-5,
                    "inner={inner}, angle={angle}: {} vs {expected}",
                    snapshot.volume_mm3
                );
                snapshot.mesh.validate()?;
                assert!(snapshot.bounds[0].z.abs() < 0.001);
                assert!((snapshot.bounds[1].z - 20.0).abs() < 0.001);
                if angle == 90.0 {
                    assert!(snapshot.bounds[0].truncate().abs().max_element() < 0.001);
                    assert!((snapshot.bounds[1].x - 10.0).abs() < 0.001);
                    assert!((snapshot.bounds[1].y - 10.0).abs() < 0.001);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn revolved_axis_contacts_tapers_and_cups_are_closed() -> Result<(), String> {
        for (points, volume_over_pi) in [
            ("{{x=0,y=0},{x=10,y=0},{x=0,y=20}}", 100.0 * 20.0 / 3.0),
            (
                "{{x=0,y=0},{x=10,y=0},{x=5,y=20},{x=0,y=20}}",
                20.0 * 175.0 / 3.0,
            ),
            (
                "{{x=0,y=0},{x=10,y=0},{x=10,y=20},{x=8,y=20},{x=8,y=2},{x=0,y=2}}",
                848.0,
            ),
        ] {
            let snapshot = crate::build_script_snapshot(
                &format!("local p=cad.polygon({points});cad.show('turned',cad.revolve(p,{{}}))"),
                1,
            )?;
            let expected = volume_over_pi * std::f64::consts::PI;
            assert!((snapshot.volume_mm3 - expected).abs() < expected * 1e-5);
            snapshot.mesh.validate()?;
        }
        Ok(())
    }

    #[test]
    fn revolve_reuses_profiles_with_either_winding_and_existing_solid_operations()
    -> Result<(), String> {
        let snapshot =
            crate::build_script_snapshot(include_str!("../../models/revolved-bushing.luau"), 1)?;
        let expected = std::f64::consts::PI * ((256.0 - 25.0) * 4.0 + (100.0 - 25.0) * 16.0);
        assert!((snapshot.volume_mm3 - expected).abs() < expected * 1e-5);
        snapshot.mesh.validate()?;
        for points in [
            "{{x=5,y=0},{x=10,y=0},{x=10,y=20},{x=5,y=20}}",
            "{{x=5,y=20},{x=10,y=20},{x=10,y=0},{x=5,y=0}}",
        ] {
            let source = format!(
                "local p=cad.polygon({points});local a=cad.revolve(p,{{angle=90}});local b=cad.translate(cad.revolve(p,{{angle=90}}),{{x=40}});local part=cad.union(a,b);part=cad.rotate(part,{{axis='z',angle=30}});cad.show('reuse',part)"
            );
            let snapshot = crate::build_script_snapshot(&source, 1)?;
            let expected = 2.0 * std::f64::consts::PI * 75.0 * 20.0 / 4.0;
            assert!((snapshot.volume_mm3 - expected).abs() < expected * 1e-5);
            snapshot.mesh.validate()?;
        }
        let source = "local p=cad.polygon({{x=5,y=0},{x=10,y=0},{x=10,y=20},{x=5,y=20}});local part=cad.revolve(p,{angle=90});part=cad.subtract(part,cad.box({width=5,depth=20,height=20,x=5}));cad.show('cut',part)";
        let snapshot = crate::build_script_snapshot(source, 1)?;
        assert!(snapshot.volume_mm3 > 0.0);
        assert!(snapshot.volume_mm3 < std::f64::consts::PI * 75.0 * 20.0 / 4.0);
        snapshot.mesh.validate()?;
        Ok(())
    }

    #[test]
    fn invalid_revolves_and_oversized_exports_are_rejected() -> Result<(), String> {
        let base = "local p=cad.polygon({{x=5,y=0},{x=10,y=0},{x=10,y=20},{x=5,y=20}});";
        for options in [
            "{angle=0}",
            "{angle=-90}",
            "{angle=361}",
            "{angle=0/0}",
            "{angle=math.huge}",
            "{angle=0.01}",
            "{axis='y'}",
        ] {
            assert!(
                crate::model::compile(&format!("{base}cad.show('bad',cad.revolve(p,{options}))"))
                    .is_err()
            );
        }
        for points in [
            "{{x=-5,y=0},{x=10,y=0},{x=10,y=20},{x=-5,y=20}}",
            "{{x=1001,y=0},{x=1010,y=0},{x=1010,y=20},{x=1001,y=20}}",
        ] {
            assert!(
                crate::model::compile(&format!(
                    "local p=cad.polygon({points});cad.show('bad',cad.revolve(p,{{}}))"
                ))
                .err()
                .is_some_and(|error| error.contains("radius"))
            );
        }
        assert!(
            crate::model::compile(
                "cad.show('bad',cad.revolve(cad.box({width=10,depth=10,height=10}),{}))"
            )
            .is_err()
        );
        let source = "local p=cad.polygon({{x=0,y=0},{x=200,y=0},{x=200,y=20},{x=0,y=20}});cad.show('large',cad.revolve(p,{}))";
        let folder =
            std::env::temp_dir().join(format!("spice-cad-revolve-large-{}", std::process::id()));
        assert!(
            crate::export::model_files(source, &folder)
                .err()
                .is_some_and(|error| error.contains("P1S"))
        );
        assert!(!folder.exists());
        Ok(())
    }

    #[test]
    fn concave_extrusions_preserve_volume_and_accept_booleans_transforms_and_reuse()
    -> Result<(), String> {
        let source = include_str!("../../models/extruded-bracket.luau");
        let snapshot = crate::build_script_snapshot(source, 1)?;
        let area_mm2 = 64.0 * 10.0 + (42.0 - 10.0) * 10.0 + 24.0 * 24.0 * 0.5;
        let expected_mm3 = (area_mm2 - 2.0 * std::f64::consts::PI * 2.0 * 2.0) * 6.0;
        assert!((snapshot.volume_mm3 - expected_mm3).abs() < expected_mm3 * 1e-6);
        snapshot.mesh.validate()?;
        {
            let kernel = crate::kernel::Kernel::new()?;
            let points = vec![
                [0.0, 0.0],
                [5.0, 0.0],
                [10.0, 0.0],
                [10.0, 10.0],
                [0.0, 10.0],
            ];
            for points in [points.clone(), points.into_iter().rev().collect()] {
                let mut body = kernel.sweep_profile(
                    &Polygon::new(points)?,
                    crate::kernel::ProfileSweep::Extrude { height_mm: 3.0 },
                )?;
                assert!((body.volume()? - 300.0).abs() < 1e-4);
                body.mesh()?.validate()?;
            }
        }
        let source = "--!strict\nlocal p=cad.polygon({{x=0,y=0},{x=10,y=0},{x=0,y=10}})\nlocal a=cad.extrude(p,{height=4})\nlocal b=cad.translate(cad.extrude(p,{height=8}),{x=20})\nlocal part=cad.rotate(cad.union(a,b),{axis='x',angle=90})\ncad.show('reused',part)";
        let snapshot = crate::build_script_snapshot(source, 1)?;
        assert!((snapshot.volume_mm3 - 600.0).abs() < 1e-4);
        snapshot.mesh.validate()?;
        Ok(())
    }

    #[test]
    fn luau_profiles_validate_shape_types_and_resource_limits() -> Result<(), String> {
        for source in [
            "local p=cad.polygon({{x=0,y=0},{x=10,y=10},{x=0,y=10},{x=10,y=0}});cad.show('bad',cad.extrude(p,{height=2}))",
            "local p=cad.polygon({{x=0,y=0},{x=10,y=0},{x=0,y=10}});cad.show('bad',cad.extrude(p,{height=0}))",
            "local p=cad.polygon({{x=0,y=0},{x=10,y=0},{x=0,y=10}});cad.show('bad',cad.extrude(p,{height=0/0}))",
            "local p=cad.polygon({{x=0,y=0},{x=10,y=0},{x=0,y=10}});cad.show('bad',p)",
            "cad.show('bad',cad.extrude(cad.box({width=10,depth=10,height=10}),{height=2}))",
            "local points:any={[1]={x=0,y=0},[2]={x=10,y=0},[4]={x=0,y=10}};local p=cad.polygon(points);cad.show('bad',cad.extrude(p,{height=2}))",
            "local points:any={{x=0,y=0},{x=10,y=0},{x=0,y=10}};points.extra=1;local p=cad.polygon(points);cad.show('bad',cad.extrude(p,{height=2}))",
            "local p:any=cad.polygon;for _=1,257 do p({{x=0,y=0},{x=10,y=0},{x=0,y=10}}) end;cad.show('bad',cad.box({width=1,depth=1,height=1}))",
        ] {
            assert!(
                crate::model::compile(source).is_err(),
                "Accepted invalid profile source: {source}"
            );
        }
        let source = "--!strict\nlocal points={{x=0,y=0},{x=10,y=0},{x=0,y=10}}\nlocal p=cad.polygon(points)\npoints[2].x=20\ncad.show('copied',cad.extrude(p,{height=2}))";
        assert!((crate::build_script_snapshot(source, 1)?.volume_mm3 - 100.0).abs() < 1e-4);
        Ok(())
    }

    #[test]
    fn accepts_concave_profiles_and_normalizes_either_winding() -> Result<(), String> {
        let points = vec![
            [0.0, 0.0],
            [40.0, 0.0],
            [40.0, 8.0],
            [8.0, 8.0],
            [8.0, 30.0],
            [0.0, 30.0],
        ];
        let forward = Polygon::new(points.clone())?;
        let backward = Polygon::new(points.into_iter().rev().collect())?;
        assert_eq!(forward.points, backward.points);
        assert_eq!(forward.points.len(), 6);
        Ok(())
    }

    #[test]
    fn rejects_invalid_boundaries_before_kernel_work() {
        for points in [
            vec![],
            vec![[0.0, 0.0]; 129],
            vec![[0.0, 0.0], [20.0, 20.0], [0.0, 20.0], [20.0, 0.0]],
            vec![[0.0, 0.0], [20.0, 0.0], [10.0, 0.0], [10.0, 20.0]],
            vec![[0.0, 0.0], [20.0, 0.0], [0.0, 20.0], [0.0, 0.0]],
            vec![[0.0, 0.0], [0.001, 0.0], [0.0, 20.0]],
            vec![[0.0, 0.0], [20.0, 0.0], [0.0, 0.000001]],
            vec![[0.0, 0.0], [f64::NAN, 0.0], [0.0, 20.0]],
            vec![[0.0, 0.0], [2001.0, 0.0], [0.0, 20.0]],
            vec![[-600.0, 0.0], [600.0, 0.0], [0.0, 20.0]],
            vec![
                [0.0, 0.0],
                [20.0, 0.0],
                [20.0, 20.0],
                [10.0, 0.0],
                [0.0, 20.0],
            ],
        ] {
            assert!(Polygon::new(points).is_err());
        }
    }
}
