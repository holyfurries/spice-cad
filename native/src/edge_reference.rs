use glam::DVec3;
use mlua::Table;

pub const COUNT_MAX: usize = 64;
const TOLERANCE_MM: f64 = 0.001;

pub struct Reference {
    pub name: String,
    pub solid: usize,
    pub points: [DVec3; 2],
}

impl Reference {
    pub fn from_table(solid: usize, options: Table) -> mlua::Result<Self> {
        crate::model::options_fields(&options, &["name", "from", "to"])?;
        let name = options.get::<String>("name")?;
        crate::dimensions::valid_name(&name)?;
        let mut points = [DVec3::ZERO; 2];
        for (index, key) in ["from", "to"].into_iter().enumerate() {
            let point = options.get::<Table>(key)?;
            crate::model::options_fields(&point, &["x", "y", "z"])?;
            points[index] = DVec3::new(point.get("x")?, point.get("y")?, point.get("z")?);
            if !points[index].is_finite() || points[index].abs().max_element() > 2000.0 {
                return Err(mlua::Error::RuntimeError(
                    "Edge endpoints must be finite and within ±2000 mm".into(),
                ));
            }
        }
        if points[0].distance(points[1]) < 0.01 {
            return Err(mlua::Error::RuntimeError(
                "Edge endpoints must be at least 0.01 mm apart".into(),
            ));
        }
        Ok(Self {
            name,
            solid,
            points,
        })
    }

    pub fn matches_endpoints(&self, endpoints: [DVec3; 2]) -> bool {
        let [first, last] = self.points;
        let forward = first.distance(endpoints[0]) <= TOLERANCE_MM
            && last.distance(endpoints[1]) <= TOLERANCE_MM;
        let backward = last.distance(endpoints[0]) <= TOLERANCE_MM
            && first.distance(endpoints[1]) <= TOLERANCE_MM;
        forward || backward
    }

    pub fn matches(&self, samples: &[DVec3; 17]) -> bool {
        if !self.matches_endpoints([samples[0], samples[16]]) {
            return false;
        }
        let [first, last] = self.points;
        let direction = last - first;
        let length_mm = direction.length();
        assert!(length_mm >= 0.01);
        samples.iter().all(|point| {
            let along_mm = (*point - first).dot(direction) / length_mm;
            (*point - first).cross(direction).length() / length_mm <= TOLERANCE_MM
                && (-TOLERANCE_MM..=length_mm + TOLERANCE_MM).contains(&along_mm)
        })
    }
}

pub fn selector(edge: &crate::inspection::Edge) -> Option<String> {
    let crate::inspection::EdgeGeometry::Line([first, last]) = edge.geometry else {
        return None;
    };
    Some(format!(
        "cad.edge(part, {{ name = \"edge_name\", from = {{ x = {:.6}, y = {:.6}, z = {:.6} }}, to = {{ x = {:.6}, y = {:.6}, z = {:.6} }} }})",
        first.x, first.y, first.z, last.x, last.y, last.z
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_fillets_follow_parameterized_edges_and_preserve_hole_rims() -> Result<(), String> {
        let source = include_str!("../../models/edge-fillets.luau");
        for width in [60.0, 80.0] {
            let source = source.replace("= 60", &format!("= {width}"));
            let snapshot = crate::build_script_snapshot(&source, 1)?;
            let removed_mm3 = 2.0 * (1.0 - std::f64::consts::PI / 4.0) * 4.0 * 4.0 * 8.0;
            let expected_mm3 =
                width * 30.0 * 8.0 - std::f64::consts::PI * 3.0 * 3.0 * 8.0 - removed_mm3;
            assert!(
                (snapshot.volume_mm3 - expected_mm3).abs() < expected_mm3 * 1e-5,
                "{} vs {expected_mm3}",
                snapshot.volume_mm3
            );
            snapshot.mesh.validate()?;
            assert_eq!(snapshot.inspected_edges.iter().filter(|edge| matches!(edge.geometry, crate::inspection::EdgeGeometry::Circle { radial, .. } if (radial.length()-3.0).abs()<0.001)).count(), 2);
            assert!(snapshot.inspected_edges.iter().any(|edge| matches!(edge.geometry, crate::inspection::EdgeGeometry::Line(points) if points.iter().all(|point| point.y==30.0 && point.x==0.0))));
        }
        Ok(())
    }

    #[test]
    fn references_match_either_direction_and_reject_curved_or_partial_edges() {
        let reference = Reference {
            name: "test".into(),
            solid: 0,
            points: [DVec3::ZERO, DVec3::X * 10.0],
        };
        let samples = std::array::from_fn(|index| DVec3::X * index as f64 * 10.0 / 16.0);
        assert!(reference.matches(&samples));
        let mut reversed = samples;
        reversed.reverse();
        assert!(reference.matches(&reversed));
        let mut curved = samples;
        curved[8].y = 1.0;
        assert!(!reference.matches(&curved));
        assert!(!reference.matches(&samples.map(|point| point * 0.5)));
    }

    #[test]
    fn invalid_and_stale_edge_references_fail_without_selecting_another_edge() -> Result<(), String>
    {
        let base = "local part=cad.box({width=40,depth=20,height=8});local edge=cad.edge(part,{name='front',from={x=40,y=0,z=0},to={x=40,y=0,z=8}});";
        for suffix in [
            "cad.show('bad',cad.fillet(part,{radius=2,edges={}}))",
            "cad.show('bad',cad.fillet(part,{radius=2,edges={edge,edge}}))",
            "cad.show('bad',cad.fillet(cad.translate(part,{x=1}),{radius=2,edges={edge}}))",
            "local selected:any={[1]=edge,[3]=edge};cad.show('bad',cad.fillet(part,{radius=2,edges=selected}))",
            "cad.show('bad',cad.fillet(part,{radius=2,edges={part}}))",
        ] {
            assert!(crate::model::compile(&format!("{base}{suffix}")).is_err());
        }
        let suffix = "cad.show('test',cad.fillet(part,{radius=2,edges={edge}}))";
        let missing = format!("{}{suffix}", base.replace("x=40,y=0,z=8", "x=41,y=0,z=8"));
        let error = crate::build_script_snapshot(&missing, 1)
            .err()
            .ok_or("Accepted missing edge")?;
        assert!(
            error.contains("front") && error.contains("did not match"),
            "{error}"
        );
        let folder =
            std::env::temp_dir().join(format!("spice-cad-missing-edge-{}", std::process::id()));
        assert!(crate::export::model_files(&missing, &folder).is_err());
        assert!(!folder.exists());
        let aliases = format!(
            "{base}local alias=cad.edge(part,{{name='alias',from={{x=40,y=0,z=8}},to={{x=40,y=0,z=0}}}});cad.show('bad',cad.fillet(part,{{radius=2,edges={{edge,alias}}}}))"
        );
        assert!(
            crate::build_script_snapshot(&aliases, 1)
                .err()
                .is_some_and(|error| error.contains("same edge"))
        );
        let duplicate = format!("{base}{base}{suffix}");
        assert!(crate::model::compile(&duplicate).is_err());
        let capped = "local part=cad.box({width=40,depth=20,height=8});for i=1,65 do cad.edge(part,{name=tostring(i),from={x=40,y=0,z=0},to={x=40,y=0,z=8}}) end;cad.show('bad',part)";
        assert!(
            crate::model::compile(capped)
                .err()
                .is_some_and(|error| error.contains("at most 64"))
        );
        Ok(())
    }

    #[test]
    fn copied_straight_edge_selector_builds_a_selective_fillet() -> Result<(), String> {
        let base = "local part=cad.box({width=40,depth=20,height=8});";
        let snapshot = crate::build_script_snapshot(&format!("{base}cad.show('base',part)"), 1)?;
        let edge = snapshot.inspected_edges.iter().find(|edge| matches!(edge.geometry, crate::inspection::EdgeGeometry::Line(points) if points.iter().all(|point| point.x==40.0 && point.y==0.0))).ok_or("Missing vertical edge")?;
        let selector = selector(edge).ok_or("Missing selector")?;
        let output = crate::build_script_snapshot(
            &format!(
                "{base}local edge={selector};cad.show('round',cad.fillet(part,{{radius=2,edges={{edge}}}}))"
            ),
            1,
        )?;
        let expected = 40.0 * 20.0 * 8.0 - (1.0 - std::f64::consts::PI / 4.0) * 4.0 * 8.0;
        assert!((output.volume_mm3 - expected).abs() < expected * 1e-5);
        output.mesh.validate()?;
        Ok(())
    }
}
