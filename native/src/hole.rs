use crate::{
    dimensions::Selector,
    kernel::{Body, Kernel},
};
use glam::DVec3;

pub struct Recipe {
    pub face: Selector,
    pub offset: [f64; 2],
    pub radius: f64,
    pub depth: Option<f64>,
}

impl Recipe {
    pub fn apply<'kernel>(
        &self,
        kernel: &'kernel Kernel,
        body: &mut Body<'kernel>,
    ) -> Result<(), String> {
        self.cut(kernel, body)
            .map_err(|error| format!("Hole on face '{}': {error}", self.face.name))
    }

    fn cut<'kernel>(
        &self,
        kernel: &'kernel Kernel,
        body: &mut Body<'kernel>,
    ) -> Result<(), String> {
        assert!(self.radius.is_finite());
        assert!(self.radius > 0.0);
        let mesh = body.mesh()?;
        let (face_id, bounds) = self.face.resolve(&crate::dimensions::planes(&mesh))?;
        let axis = self.face.axis;
        let axes = match axis {
            0 => [1, 2],
            1 => [0, 2],
            2 => [0, 1],
            _ => unreachable!(),
        };
        let mut center = (bounds[0].as_dvec3() + bounds[1].as_dvec3()) * 0.5;
        center[axes[0]] += self.offset[0];
        center[axes[1]] += self.offset[1];
        let on_face = mesh
            .vertices
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|triangle| triangle[0].face_id == face_id)
            .any(|triangle| {
                let [a, b, c] = std::array::from_fn(|index| {
                    DVec3::from_array(triangle[index].position.map(f64::from))
                });
                let normal = (b - a).cross(c - a);
                let length_squared = normal.length_squared();
                if length_squared <= 1e-20 {
                    return false;
                }
                let first = (b - center).cross(c - center).dot(normal) / length_squared;
                let second = (c - center).cross(a - center).dot(normal) / length_squared;
                let third = 1.0 - first - second;
                first >= -1e-8 && second >= -1e-8 && third >= -1e-8
            });
        if !on_face {
            return Err(
                "Hole center lies outside the selected face or inside an existing opening".into(),
            );
        }
        let body_bounds = body.bounds(false)?;
        let sign = if self.face.positive { 1.0 } else { -1.0 };
        let length = self.depth.unwrap_or_else(|| {
            if self.face.positive {
                center[axis] - body_bounds[0][axis]
            } else {
                body_bounds[1][axis] - center[axis]
            }
        });
        if !length.is_finite() || length <= 0.0 {
            return Err("No material behind the selected face".into());
        }
        let margin_mm = 0.01;
        let height = length
            + if self.depth.is_some() {
                margin_mm
            } else {
                2.0 * margin_mm
            };
        let mut cutter = kernel.cylinder([0.0; 3], self.radius, height)?;
        match (axis, self.face.positive) {
            (0, true) => cutter.rotate([0.0, 1.0, 0.0], -90.0)?,
            (0, false) => cutter.rotate([0.0, 1.0, 0.0], 90.0)?,
            (1, true) => cutter.rotate([1.0, 0.0, 0.0], 90.0)?,
            (1, false) => cutter.rotate([1.0, 0.0, 0.0], -90.0)?,
            (2, true) => cutter.rotate([1.0, 0.0, 0.0], 180.0)?,
            (2, false) => {}
            _ => unreachable!(),
        }
        center[axis] += sign * margin_mm;
        cutter.translate(center.to_array())?;
        body.subtract(cutter)
    }
}

#[cfg(test)]
mod tests {
    use crate::{kernel::Kernel, model::Cache};
    use std::sync::Arc;

    #[test]
    fn face_holes_cut_inward_on_all_six_sides_with_exact_blind_depth() -> Result<(), String> {
        for normal in ["x+", "x-", "y+", "y-", "z+", "z-"] {
            for depth in [None, Some(4.0)] {
                let depth_option = depth.map_or(String::new(), |depth| format!(",depth={depth}"));
                let source = format!(
                    "local p=cad.box({{width=20,depth=20,height=20,x=-10,y=-10,z=-10}});local f=cad.face(p,{{name='mount',normal='{normal}'}});p=cad.hole(p,{{face=f,radius=2,u=3,v=-2{depth_option}}});cad.show('part',p)"
                );
                let snapshot = crate::build_script_snapshot(&source, 1)?;
                let expected = 8000.0 - std::f64::consts::PI * 4.0 * depth.unwrap_or(20.0);
                assert!(
                    (snapshot.volume_mm3 - expected).abs() < 0.001,
                    "{normal} {depth:?}: {} vs {expected}",
                    snapshot.volume_mm3
                );
                snapshot.scene.world_mesh().validate()?;
                assert!(snapshot.scene.annotations.faces.is_empty());
            }
        }
        Ok(())
    }

    #[test]
    fn face_holes_follow_resized_faces_and_preserve_final_face_measurements() -> Result<(), String>
    {
        for width in [40, 60] {
            let source = format!(
                "local p=cad.box({{width={width},depth=30,height=8,x=7,y=11,z=13}});local f=cad.face(p,{{name='mount',normal='z+'}});p=cad.hole(p,{{face=f,radius=2,u=5,v=-3}});local top=cad.face(p,{{name='top',normal='z+'}});local bottom=cad.face(p,{{name='bottom',normal='z-'}});cad.measure('thickness',{{from=bottom,to=top,min=8,max=8}});cad.show('part',p,{{assembly={{position={{x=100}}}}}})"
            );
            let snapshot = crate::build_script_snapshot(&source, 1)?;
            assert_eq!(snapshot.scene.annotations.faces.len(), 2);
            assert_eq!(snapshot.scene.annotations.measurements[0].distance_mm, 8.0);
            let expected_x = 100.0 + 7.0 + width as f32 / 2.0 + 5.0;
            let circles = snapshot.scene.world_edges();
            assert_eq!(circles.items.iter().filter(|edge| matches!(edge.geometry, crate::inspection::EdgeGeometry::Circle { center, .. } if (center.x-expected_x).abs()<0.001 && (center.y-23.0).abs()<0.001)).count(), 2);
            snapshot.scene.world_mesh().validate()?;
        }
        let source = include_str!("../../models/mounting-bracket.luau");
        for width in [100, 120] {
            let snapshot = crate::build_script_snapshot(
                &source.replace("cad.number(100,", &format!("cad.number({width},")),
                1,
            )?;
            snapshot.scene.world_mesh().validate()?;
            assert!(snapshot.volume_mm3 > 20000.0);
        }
        Ok(())
    }

    #[test]
    fn face_holes_reject_stale_missing_ambiguous_and_off_face_references() -> Result<(), String> {
        let base = "local p=cad.box({width=20,depth=20,height=8});local f=cad.face(p,{name='mount',normal='z+'});";
        for options in [
            "{face=f,radius=0}",
            "{face=f,radius=2,depth=0}",
            "{face=f,radius=2,depth=0/0}",
            "{face=f,radius=2,u=0/0}",
            "{face=f,radius=2,w=3}",
            "{face=p,radius=2}",
        ] {
            assert!(
                crate::model::compile(&format!("{base}cad.show('bad',cad.hole(p,{options}))"))
                    .is_err(),
                "{options}"
            );
        }
        for (source, message) in [
            (format!("{base}cad.show('bad',cad.hole(cad.translate(p,{{x=1}}),{{face=f,radius=2}}))"), "different source solid"),
            (format!("{base}cad.show('bad',cad.hole(p,{{face=f,radius=2,u=100}}))"), "outside"),
            (format!("{base}cad.show('bad',cad.hole(p,{{face=f,radius=2}}))").replace("normal='z+'", "normal='z+',at=9"), "no matching"),
            ("local p=cad.union(cad.box({width=20,depth=20,height=8}),cad.box({width=10,depth=10,height=16}));local f=cad.face(p,{name='mount',normal='z+'});cad.show('bad',cad.hole(p,{face=f,radius=2}))".into(), "ambiguous"),
            (format!("{base}p=cad.hole(p,{{face=f,radius=2}});local next_face=cad.face(p,{{name='next',normal='z+'}});cad.show('bad',cad.hole(p,{{face=next_face,radius=1}}))"), "existing opening"),
        ] {
            let error = crate::build_script_snapshot(&source, 1).err().ok_or("Accepted invalid hole")?;
            assert!(error.contains(message), "{message}: {error}");
            assert!(error.contains("mount") || error.contains("next"), "{error}");
        }
        Ok(())
    }

    #[test]
    fn face_hole_cache_reuses_placements_and_invalidates_geometry_inputs() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let mut cache = Cache::new(&kernel);
        let source = "local p=cad.box({width=40,depth=30,height=8});local f=cad.face(p,{name='mount',normal='z+'});p=cad.hole(p,{face=f,radius=2,u=3,v=4,depth=3});cad.show('part',p,{assembly={position={x=0}}})";
        let model = cache.compile(source)?;
        let prepared = cache.prepare(&model)?;
        let original = Arc::clone(&prepared.geometry[0]);
        for source in [
            source.replace("x=0", "x=100"),
            source.replace("'mount'", "'renamed'"),
        ] {
            let model = cache.compile(&source)?;
            let prepared = cache.prepare(&model)?;
            assert_eq!(prepared.hits, 1);
            assert!(Arc::ptr_eq(&original, &prepared.geometry[0]));
        }
        for (from, to) in [
            ("radius=2", "radius=3"),
            ("u=3", "u=5"),
            ("v=4", "v=-4"),
            ("depth=3", "depth=4"),
            ("normal='z+'", "normal='z-'"),
            ("width=40", "width=50"),
            ("normal='z+'", "normal='z+',at=8"),
            ("normal='z+'", "normal='z+',near={x=20,y=15,z=8}"),
            ("v=4,depth=3", "v=4"),
        ] {
            let model = cache.compile(&source.replace(from, to))?;
            let prepared = cache.prepare(&model)?;
            assert_eq!(prepared.hits, 0, "{from}");
            assert!(!Arc::ptr_eq(&original, &prepared.geometry[0]));
        }
        Ok(())
    }
}
