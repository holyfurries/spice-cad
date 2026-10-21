use crate::{
    dimensions::Selector,
    kernel::{Body, Kernel},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pocket,
    Boss,
}

#[derive(PartialEq)]
pub enum Shape {
    Rectangle([f64; 2]),
    Profile(crate::profile::Profile),
}

pub struct Recipe {
    pub face: Selector,
    pub offset: [f64; 2],
    pub shape: Shape,
    pub depth: f64,
    pub kind: Kind,
}

impl Recipe {
    pub fn apply<'kernel>(
        &self,
        kernel: &'kernel Kernel,
        body: &mut Body<'kernel>,
    ) -> Result<(), String> {
        self.build(kernel, body)
            .map_err(|error| format!("{:?} on face '{}': {error}", self.kind, self.face.name))
    }

    fn build<'kernel>(
        &self,
        kernel: &'kernel Kernel,
        body: &mut Body<'kernel>,
    ) -> Result<(), String> {
        assert!(self.depth.is_finite());
        assert!(self.depth > 0.0);
        let center = body.planar_feature_origin(&self.face, self.offset)?;
        let axis = self.face.axis;
        let outward = if self.face.positive { 1.0 } else { -1.0 };
        let direction = match self.kind {
            Kind::Pocket => -outward,
            Kind::Boss => outward,
        };
        if let Shape::Profile(profile) = &self.shape {
            let profile = profile.face_coordinates(axis, direction > 0.0);
            let mut feature = kernel.sweep_curved_profile(
                &profile,
                crate::kernel::ProfileSweep::Extrude {
                    height_mm: self.depth + 0.01,
                },
            )?;
            match (axis, direction > 0.0) {
                (0, positive) => {
                    feature.rotate([0.0, 1.0, 0.0], if positive { 90.0 } else { -90.0 })?
                }
                (1, positive) => {
                    feature.rotate([1.0, 0.0, 0.0], if positive { -90.0 } else { 90.0 })?
                }
                (2, false) => feature.rotate([1.0, 0.0, 0.0], 180.0)?,
                (2, true) => {}
                _ => unreachable!(),
            }
            let mut origin = center;
            origin[axis] -= direction * 0.01;
            feature.translate(origin.to_array())?;
            return match self.kind {
                Kind::Pocket => body.subtract(feature),
                Kind::Boss => body.union(feature),
            };
        }
        let Shape::Rectangle(size) = &self.shape else {
            unreachable!()
        };
        let axes = match axis {
            0 => [1, 2],
            1 => [0, 2],
            2 => [0, 1],
            _ => unreachable!(),
        };
        let mut minimum = center;
        let mut maximum = center;
        for (index, local_axis) in axes.into_iter().enumerate() {
            minimum[local_axis] -= size[index] * 0.5;
            maximum[local_axis] += size[index] * 0.5;
        }
        let end = center[axis] + direction * self.depth;
        let overlap = center[axis] - direction * 0.01;
        minimum[axis] = end.min(overlap);
        maximum[axis] = end.max(overlap);
        let feature = kernel.box_shape(minimum.to_array(), maximum.to_array())?;
        match self.kind {
            Kind::Pocket => body.subtract(feature),
            Kind::Boss => body.union(feature),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{kernel::Kernel, model::Cache};
    use std::sync::Arc;

    #[test]
    fn manufacturing_example_combines_profile_features_and_screw_seats() -> Result<(), String> {
        let snapshot =
            crate::build_script_snapshot(include_str!("../../models/manufacturing-panel.luau"), 1)?;
        let expected = 80.0 * 60.0 * 8.0 - 292.0 * 3.0 + 84.5 * 4.0
            - std::f64::consts::PI * (56.0 + 128.0 / 3.0);
        assert!((snapshot.volume_mm3 - expected).abs() < 0.001);
        snapshot.scene.world_mesh().validate()?;
        assert_eq!(snapshot.parts.len(), 1);
        Ok(())
    }

    #[test]
    fn concave_profile_features_keep_world_uv_axes_on_all_six_normals() -> Result<(), String> {
        for normal in ["x+", "x-", "y+", "y-", "z+", "z-"] {
            for operation in ["pocket", "boss"] {
                let source = format!(
                    "local p=cad.box({{width=20,depth=20,height=20,x=-10,y=-10,z=-10}});local profile=cad.polygon({{{{x=0,y=0}},{{x=4,y=0}},{{x=4,y=1}},{{x=1,y=1}},{{x=1,y=3}},{{x=0,y=3}}}});local f=cad.face(p,{{name='support',normal='{normal}'}});cad.show('part',cad.{operation}(p,{{face=f,profile=profile,depth=4,u=2,v=-1}}))"
                );
                let snapshot = crate::build_script_snapshot(&source, 1)?;
                let expected = if operation == "boss" { 8024.0 } else { 7976.0 };
                assert!(
                    (snapshot.volume_mm3 - expected).abs() < 0.001,
                    "{normal} {operation}: {}",
                    snapshot.volume_mm3
                );
                let axis = match normal.as_bytes()[0] {
                    b'x' => 0,
                    b'y' => 1,
                    _ => 2,
                };
                let [u, v] = match axis {
                    0 => [1, 2],
                    1 => [0, 2],
                    _ => [0, 1],
                };
                let distance = if operation == "boss" { 14.0 } else { 6.0 };
                let tip = if normal.ends_with('+') {
                    distance
                } else {
                    -distance
                };
                let mesh = snapshot.scene.world_mesh();
                mesh.validate()?;
                let points: Vec<_> = mesh
                    .vertices
                    .iter()
                    .filter(|vertex| (vertex.position[axis] - tip).abs() < 0.001)
                    .map(|vertex| vertex.position)
                    .collect();
                assert!(!points.is_empty());
                for [expected_u, expected_v] in [
                    [2.0, -1.0],
                    [6.0, -1.0],
                    [6.0, 0.0],
                    [3.0, 0.0],
                    [3.0, 2.0],
                    [2.0, 2.0],
                ] {
                    assert!(
                        points
                            .iter()
                            .any(|point| (point[u] - expected_u).abs() < 0.001
                                && (point[v] - expected_v).abs() < 0.001),
                        "{normal} {operation}: missing {expected_u},{expected_v}"
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn profile_features_validate_options_and_cache_profile_coordinates() -> Result<(), String> {
        let base = "local p=cad.box({width=20,depth=20,height=8});local profile=cad.polygon({{x=-2,y=-1},{x=3,y=-1},{x=0,y=2}});local f=cad.face(p,{name='top',normal='z+'});";
        for operation in ["pocket", "boss"] {
            for options in [
                "{face=f,profile=profile,width=2,height=2,depth=3}",
                "{face=f,profile=p,depth=3}",
                "{face=f,profile=profile,depth=0}",
                "{face=f,profile=profile,depth=3,u=math.huge}",
            ] {
                assert!(
                    crate::model::compile(&format!(
                        "{base}cad.show('bad',cad.{operation}(p,{options}))"
                    ))
                    .is_err()
                );
            }
        }
        let source =
            format!("{base}cad.show('part',cad.pocket(p,{{face=f,profile=profile,depth=3}}))");
        let kernel = Kernel::new()?;
        let mut cache = Cache::new(&kernel);
        let model = cache.compile(&source)?;
        let prepared = cache.prepare(&model)?;
        let original = Arc::clone(&prepared.geometry[0]);
        let model = cache.compile(&source.replace("'top'", "'renamed'"))?;
        let prepared = cache.prepare(&model)?;
        assert!(Arc::ptr_eq(&original, &prepared.geometry[0]));
        for edited in [
            source.replace("x=3,y=-1", "x=4,y=-1"),
            source.replace("pocket", "boss"),
        ] {
            let model = cache.compile(&edited)?;
            let prepared = cache.prepare(&model)?;
            assert_eq!(prepared.hits, 0);
            assert!(!Arc::ptr_eq(&original, &prepared.geometry[0]));
        }
        Ok(())
    }

    #[test]
    fn face_features_preserve_depth_and_local_axes_on_all_six_normals() -> Result<(), String> {
        for normal in ["x+", "x-", "y+", "y-", "z+", "z-"] {
            for (operation, expected) in [("pocket", 8000.0 - 120.0), ("boss", 8000.0 + 120.0)] {
                let source = format!(
                    "local p=cad.box({{width=20,depth=20,height=20,x=-10,y=-10,z=-10}});local f=cad.face(p,{{name='support',normal='{normal}'}});p=cad.{operation}(p,{{face=f,width=6,height=5,depth=4,u=2,v=-1}});cad.show('part',p)"
                );
                let snapshot = crate::build_script_snapshot(&source, 1)?;
                assert!(
                    (snapshot.volume_mm3 - expected).abs() < 0.001,
                    "{normal} {operation}: {}",
                    snapshot.volume_mm3
                );
                snapshot.scene.world_mesh().validate()?;
                assert!(snapshot.scene.annotations.faces.is_empty());
                let axis = match normal.as_bytes()[0] {
                    b'x' => 0,
                    b'y' => 1,
                    _ => 2,
                };
                let axes = match axis {
                    0 => [1, 2],
                    1 => [0, 2],
                    _ => [0, 1],
                };
                let distance = if operation == "boss" { 14.0 } else { 6.0 };
                let tip = if normal.ends_with('+') {
                    distance
                } else {
                    -distance
                };
                let tip_bounds = snapshot
                    .scene
                    .world_mesh()
                    .vertices
                    .iter()
                    .filter(|vertex| (vertex.position[axis] - tip).abs() < 0.001)
                    .fold(
                        [
                            glam::Vec3::splat(f32::INFINITY),
                            glam::Vec3::splat(f32::NEG_INFINITY),
                        ],
                        |bounds, vertex| {
                            let position = glam::Vec3::from_array(vertex.position);
                            [bounds[0].min(position), bounds[1].max(position)]
                        },
                    );
                for (index, center, half_size) in [(axes[0], 2.0, 3.0), (axes[1], -1.0, 2.5)] {
                    assert!((tip_bounds[0][index] - (center - half_size)).abs() < 0.001);
                    assert!((tip_bounds[1][index] - (center + half_size)).abs() < 0.001);
                }
                let bounds = snapshot.scene.world_mesh().vertices.iter().fold(
                    [
                        glam::Vec3::splat(f32::INFINITY),
                        glam::Vec3::splat(f32::NEG_INFINITY),
                    ],
                    |bounds, vertex| {
                        let position = glam::Vec3::from_array(vertex.position);
                        [bounds[0].min(position), bounds[1].max(position)]
                    },
                );
                if operation == "boss" {
                    if normal.ends_with('+') {
                        assert!((bounds[1][axis] - 14.0).abs() < 0.001);
                    } else {
                        assert!((bounds[0][axis] + 14.0).abs() < 0.001);
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn face_features_reject_invalid_options_stale_handles_and_off_face_centers()
    -> Result<(), String> {
        let base = "local p=cad.box({width=30,depth=20,height=8});local f=cad.face(p,{name='support',normal='z+'});";
        for operation in ["pocket", "boss"] {
            for options in [
                "{face=f,width=0,height=5,depth=3}",
                "{face=f,width=5,height=-1,depth=3}",
                "{face=f,width=5,height=5,depth=0}",
                "{face=f,width=5,height=5,depth=0/0}",
                "{face=f,width=5,height=5,depth=3,u=0/0}",
                "{face=f,width=5,height=5,depth=3,unknown=1}",
            ] {
                assert!(
                    crate::model::compile(&format!(
                        "{base}cad.show('bad',cad.{operation}(p,{options}))"
                    ))
                    .is_err()
                );
            }
            for (solid, offset, message) in [
                ("cad.translate(p,{x=1})", 0, "different source solid"),
                ("p", 100, "outside"),
            ] {
                let source = format!(
                    "{base}cad.show('bad',cad.{operation}({solid},{{face=f,width=5,height=5,depth=3,u={offset}}}))"
                );
                let error = crate::build_script_snapshot(&source, 1)
                    .err()
                    .ok_or("Accepted invalid feature")?;
                assert!(error.contains(message), "{error}");
            }
        }
        Ok(())
    }

    #[test]
    fn face_features_compose_on_internal_floors_and_support_open_through_cuts() -> Result<(), String>
    {
        let snapshot =
            crate::build_script_snapshot(include_str!("../../models/enclosure.luau"), 1)?;
        let expected = 80.0 * 50.0 * 20.0 - 74.0 * 44.0 * 17.0 + 10.0 * 10.0 * 5.0
            - std::f64::consts::PI * 1.5 * 1.5 * 5.0;
        assert!((snapshot.volume_mm3 - expected).abs() < 0.001);
        snapshot.scene.world_mesh().validate()?;
        let source = "local p=cad.box({width=20,depth=20,height=8});local f=cad.face(p,{name='top',normal='z+'});p=cad.pocket(p,{face=f,width=10,height=10,depth=20,u=8});cad.show('part',p)";
        let snapshot = crate::build_script_snapshot(source, 1)?;
        assert!((snapshot.volume_mm3 - (3200.0 - 7.0 * 10.0 * 8.0)).abs() < 0.001);
        snapshot.scene.world_mesh().validate()?;
        for operation in ["pocket", "boss"] {
            let source = format!(
                "local p=cad.box({{width=20,depth=20,height=8}});local f=cad.face(p,{{name='top',normal='z+'}});p=cad.hole(p,{{face=f,radius=2}});local f=cad.face(p,{{name='remaining top',normal='z+'}});cad.show('bad',cad.{operation}(p,{{face=f,width=5,height=5,depth=3}}))"
            );
            let error = crate::build_script_snapshot(&source, 1)
                .err()
                .ok_or("Accepted feature inside opening")?;
            assert!(error.contains("existing opening"), "{error}");
            assert!(error.contains("remaining top"), "{error}");
        }
        Ok(())
    }

    #[test]
    fn face_feature_cache_matches_all_geometry_inputs_and_reuses_placements() -> Result<(), String>
    {
        let kernel = Kernel::new()?;
        let mut cache = Cache::new(&kernel);
        let source = "local p=cad.box({width=40,depth=30,height=8});local f=cad.face(p,{name='mount',normal='z+'});p=cad.pocket(p,{face=f,width=10,height=8,depth=3,u=2,v=1});cad.show('part',p,{assembly={position={x=0}}})";
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
            ("width=10", "width=12"),
            ("height=8", "height=6"),
            ("depth=3", "depth=4"),
            ("u=2", "u=3"),
            ("v=1", "v=-1"),
            ("pocket", "boss"),
            ("normal='z+'", "normal='z-'"),
        ] {
            let model = cache.compile(&source.replace(from, to))?;
            let prepared = cache.prepare(&model)?;
            assert_eq!(prepared.hits, 0, "{from}");
            assert!(!Arc::ptr_eq(&original, &prepared.geometry[0]));
        }
        Ok(())
    }
}
