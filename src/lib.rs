use glam::{DVec3, dvec3};
#[cfg(feature = "reference")]
use opencascade::primitives::Shape;

#[cfg(feature = "reference")]
pub mod printing;

pub const CABINET_DIMENSIONS_MM: DVec3 = dvec3(493.0, 293.0, 322.0);
pub const PARAMETERS: RackParameters = RackParameters {
    rack_width_mm: 310.0,
    step_count: 5,
    step_rise_mm: 42.0,
    jar_diameter_mm: 50.0,
    jar_height_mm: 135.0,
    clearance_mm: 1.5,
    cabinet_clearance_mm: 5.0,
    thickness_mm: 3.0,
    lip_height_mm: 8.0,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RackParameters {
    pub rack_width_mm: f64,
    pub step_count: u32,
    pub step_rise_mm: f64,
    pub jar_diameter_mm: f64,
    pub jar_height_mm: f64,
    pub clearance_mm: f64,
    pub cabinet_clearance_mm: f64,
    pub thickness_mm: f64,
    pub lip_height_mm: f64,
}

pub struct RackLayout {
    pub dimensions_mm: DVec3,
    pub tread_depth_mm: f64,
    pub jars_per_step_count: u32,
    pub jar_height_mm_max: f64,
}

impl RackParameters {
    pub fn layout(self) -> Result<RackLayout, String> {
        if !(1..=8).contains(&self.step_count) {
            return Err("step_count must be between 1 and 8".into());
        }
        for (name, value, minimum, maximum) in [
            ("rack_width_mm", self.rack_width_mm, 100.0, 480.0),
            ("step_rise_mm", self.step_rise_mm, 10.0, 80.0),
            ("jar_diameter_mm", self.jar_diameter_mm, 20.0, 100.0),
            ("jar_height_mm", self.jar_height_mm, 20.0, 300.0),
            ("clearance_mm", self.clearance_mm, 0.5, 10.0),
            ("cabinet_clearance_mm", self.cabinet_clearance_mm, 1.0, 20.0),
            ("thickness_mm", self.thickness_mm, 1.0, 10.0),
            ("lip_height_mm", self.lip_height_mm, 1.0, 20.0),
        ] {
            if !(minimum..=maximum).contains(&value) {
                return Err(format!(
                    "{name} must be finite and between {minimum} and {maximum}"
                ));
            }
        }
        if self.jar_height_mm <= self.lip_height_mm {
            return Err("jar_height_mm must exceed lip_height_mm".into());
        }
        let tread_depth_mm = self.jar_diameter_mm + 2.0 * self.clearance_mm + self.thickness_mm;
        let jar_space_width_mm = self.rack_width_mm - 2.0 * (self.thickness_mm + self.clearance_mm);
        if jar_space_width_mm < self.jar_diameter_mm {
            return Err("rack_width_mm must accommodate a spice jar with side clearance".into());
        }
        let jars_per_step_count = ((jar_space_width_mm + self.clearance_mm)
            / (self.jar_diameter_mm + self.clearance_mm))
            .floor() as u32;
        let jar_floor_mm_max =
            f64::from(self.step_count - 1) * self.step_rise_mm + self.thickness_mm;
        let dimensions_mm = dvec3(
            self.rack_width_mm,
            f64::from(self.step_count) * tread_depth_mm + self.thickness_mm,
            jar_floor_mm_max + self.lip_height_mm,
        );
        if (dimensions_mm + DVec3::splat(self.cabinet_clearance_mm))
            .cmpgt(CABINET_DIMENSIONS_MM)
            .any()
        {
            return Err("rack exceeds cabinet dimensions including cabinet_clearance_mm".into());
        }
        let jar_height_mm_max = CABINET_DIMENSIONS_MM.z
            - self.cabinet_clearance_mm
            - self.lip_height_mm
            - jar_floor_mm_max;
        if self.jar_height_mm > jar_height_mm_max {
            return Err(format!(
                "jar_height_mm exceeds {jar_height_mm_max} mm with clearance to lift over lips"
            ));
        }
        assert!(dimensions_mm.is_finite());
        assert!(dimensions_mm.min_element() > 0.0);
        assert!((1..=24).contains(&jars_per_step_count));
        Ok(RackLayout {
            dimensions_mm,
            tread_depth_mm,
            jars_per_step_count,
            jar_height_mm_max,
        })
    }
}

#[cfg(feature = "reference")]
pub fn build_rack(parameters: RackParameters) -> Result<Shape, String> {
    let layout = parameters.layout()?;
    let width_mm = layout.dimensions_mm.x;
    let depth_mm = layout.dimensions_mm.y;
    let thickness_mm = parameters.thickness_mm;
    assert!(width_mm > 2.0 * thickness_mm);
    assert!(layout.tread_depth_mm > thickness_mm);

    let mut rack = Shape::box_with_dimensions(width_mm, depth_mm, thickness_mm);
    for step_index in 0..parameters.step_count {
        let y_mm = f64::from(step_index) * layout.tread_depth_mm;
        let z_mm = f64::from(step_index) * parameters.step_rise_mm;
        let step_height_mm = z_mm + thickness_mm + parameters.lip_height_mm;
        for x_mm in [0.0, width_mm - thickness_mm] {
            let side = Shape::box_from_corners(
                dvec3(x_mm, y_mm, 0.0),
                dvec3(
                    x_mm + thickness_mm,
                    y_mm + layout.tread_depth_mm,
                    step_height_mm,
                ),
            );
            rack = rack.union(&side).into();
        }
        let tread = Shape::box_from_corners(
            dvec3(0.0, y_mm, z_mm),
            dvec3(width_mm, y_mm + layout.tread_depth_mm, z_mm + thickness_mm),
        );
        let riser_bottom_mm = z_mm - parameters.step_rise_mm.min(z_mm);
        let riser = Shape::box_from_corners(
            dvec3(0.0, y_mm, riser_bottom_mm),
            dvec3(width_mm, y_mm + thickness_mm, step_height_mm),
        );
        rack = rack.union(&tread).into();
        rack = rack.union(&riser).into();
    }
    let back = Shape::box_from_corners(
        dvec3(0.0, depth_mm - thickness_mm, 0.0),
        layout.dimensions_mm,
    );
    Ok(rack.union(&back).clean())
}

#[cfg(all(test, feature = "reference"))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn rejects_invalid_dimensions_and_cabinet_collisions() {
        for parameters in [
            RackParameters {
                step_count: 0,
                ..PARAMETERS
            },
            RackParameters {
                step_count: 9,
                ..PARAMETERS
            },
            RackParameters {
                step_count: 6,
                ..PARAMETERS
            },
            RackParameters {
                rack_width_mm: 100.0,
                jar_diameter_mm: 100.0,
                ..PARAMETERS
            },
            RackParameters {
                jar_diameter_mm: f64::NAN,
                ..PARAMETERS
            },
            RackParameters {
                jar_height_mm: f64::INFINITY,
                ..PARAMETERS
            },
            RackParameters {
                jar_height_mm: 139.0,
                ..PARAMETERS
            },
            RackParameters {
                step_rise_mm: 44.0,
                ..PARAMETERS
            },
            RackParameters {
                thickness_mm: -1.0,
                ..PARAMETERS
            },
        ] {
            assert!(parameters.layout().is_err());
            assert!(build_rack(parameters).is_err());
        }
    }

    #[test]
    fn stepped_mesh_is_closed_and_matches_the_envelope() -> Result<(), Box<dyn std::error::Error>> {
        for parameters in [
            PARAMETERS,
            RackParameters {
                step_count: 3,
                step_rise_mm: 35.0,
                ..PARAMETERS
            },
        ] {
            let layout = parameters.layout()?;
            let parts = printing::build_parts(parameters)?;
            let (minimum_mm, maximum_mm) = assert_closed_mesh(&printing::assemble_parts(&parts))?;
            assert!(minimum_mm.abs().max_element() < 1e-6);
            assert!((maximum_mm - layout.dimensions_mm).abs().max_element() < 1e-6);
        }
        Ok(())
    }

    #[test]
    fn rack_parts_are_closed_and_fit_the_p1s() -> Result<(), Box<dyn std::error::Error>> {
        let dimensions_mm_expected = [
            dvec3(95.0, 174.0, 161.0),
            dvec3(179.0, 115.0, 161.0),
            dvec3(95.0, 174.0, 155.0),
            dvec3(179.0, 115.0, 155.0),
        ];
        for (part, expected_mm) in printing::build_parts(PARAMETERS)?
            .iter()
            .zip(dimensions_mm_expected)
        {
            let (on_bed, dimensions_mm) = part.on_bed()?;
            let (minimum_mm, maximum_mm) = assert_closed_mesh(&on_bed)?;
            assert!((dimensions_mm - expected_mm).abs().max_element() < 1e-6);
            assert!(minimum_mm.abs().max_element() < 1e-6);
            assert!(maximum_mm.cmple(dvec3(246.0, 246.0, 256.0)).all());
        }
        Ok(())
    }

    #[test]
    fn fit_samples_are_closed_and_fit_the_p1s() -> Result<(), Box<dyn std::error::Error>> {
        let dimensions_mm_expected = [
            dvec3(3.0, 24.0, 18.0),
            dvec3(3.0, 24.0, 12.0),
            dvec3(24.0, 18.0, 3.0),
            dvec3(24.0, 12.0, 3.0),
        ];
        for (part, expected_mm) in printing::build_fit_samples(PARAMETERS.thickness_mm)?
            .iter()
            .zip(dimensions_mm_expected)
        {
            let (on_bed, dimensions_mm) = part.on_bed()?;
            let (minimum_mm, maximum_mm) = assert_closed_mesh(&on_bed)?;
            assert!((dimensions_mm - expected_mm).abs().max_element() < 1e-6);
            assert!(minimum_mm.abs().max_element() < 1e-6);
            assert!(maximum_mm.cmple(dvec3(246.0, 246.0, 256.0)).all());
        }
        Ok(())
    }

    #[test]
    fn print_orientations_have_no_overhangs_steeper_than_45_degrees()
    -> Result<(), Box<dyn std::error::Error>> {
        let parts = printing::build_parts(PARAMETERS)?;
        let samples = printing::build_fit_samples(PARAMETERS.thickness_mm)?;
        for part in parts.iter().chain(samples.iter()) {
            let (on_bed, _) = part.on_bed()?;
            let mesh = on_bed.mesh()?;
            assert!(!mesh.indices.is_empty());
            for triangle in mesh.indices.as_chunks::<3>().0 {
                let [first, second, third] = triangle.map(|index| mesh.vertices[index]);
                if [first, second, third]
                    .iter()
                    .all(|point| point.z.abs() < 1e-6)
                {
                    continue;
                }
                let normal = (second - first).cross(third - first);
                assert!(normal.length_squared() > 0.0);
                assert!(
                    normal.normalize().z >= -std::f64::consts::FRAC_1_SQRT_2 - 1e-6,
                    "{} has an unsupported downward face",
                    part.name
                );
            }
        }
        Ok(())
    }

    fn assert_closed_mesh(shape: &Shape) -> Result<(DVec3, DVec3), Box<dyn std::error::Error>> {
        let mesh = shape.mesh()?;
        assert!(!mesh.indices.is_empty());
        assert_eq!(mesh.indices.len() % 3, 0);
        let minimum_mm = mesh
            .vertices
            .iter()
            .copied()
            .fold(DVec3::splat(f64::INFINITY), DVec3::min);
        let maximum_mm = mesh
            .vertices
            .iter()
            .copied()
            .fold(DVec3::splat(f64::NEG_INFINITY), DVec3::max);
        let mut edges = BTreeMap::<([i64; 3], [i64; 3]), (u32, i32)>::new();
        let mut volume_mm3 = 0.0;
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [first, second, third] = triangle.map(|index| mesh.vertices[index]);
            volume_mm3 += first.dot(second.cross(third)) / 6.0;
            for (source, target) in [(0, 1), (1, 2), (2, 0)] {
                let first = mesh.vertices[triangle[source]]
                    .to_array()
                    .map(|value| (value * 1e6).round() as i64);
                let second = mesh.vertices[triangle[target]]
                    .to_array()
                    .map(|value| (value * 1e6).round() as i64);
                assert_ne!(first, second);
                let (key, orientation) = if first < second {
                    ((first, second), 1)
                } else {
                    ((second, first), -1)
                };
                let entry = edges.entry(key).or_default();
                entry.0 += 1;
                entry.1 += orientation;
            }
        }
        assert!(edges.values().all(|entry| *entry == (2, 0)));
        assert!(volume_mm3 > 0.0);
        Ok((minimum_mm, maximum_mm))
    }

    #[test]
    fn every_jar_position_has_room_to_lift_over_its_lip() -> Result<(), Box<dyn std::error::Error>>
    {
        let parameters = RackParameters {
            jar_height_mm: 138.0,
            ..PARAMETERS
        };
        let layout = parameters.layout()?;
        assert_eq!(layout.jars_per_step_count, 5);
        assert_eq!(layout.dimensions_mm, dvec3(310.0, 283.0, 179.0));
        assert_eq!(layout.jar_height_mm_max, 138.0);
        assert!(PARAMETERS.layout().is_ok());
        let parts = printing::build_parts(parameters)?;
        let rack = printing::assemble_parts(&parts);
        for step_index in 0..parameters.step_count {
            for jar_index in 0..layout.jars_per_step_count {
                let center_mm = dvec3(
                    parameters.thickness_mm
                        + parameters.clearance_mm
                        + parameters.jar_diameter_mm / 2.0
                        + f64::from(jar_index)
                            * (parameters.jar_diameter_mm + parameters.clearance_mm),
                    f64::from(step_index) * layout.tread_depth_mm
                        + parameters.thickness_mm
                        + parameters.clearance_mm
                        + parameters.jar_diameter_mm / 2.0,
                    f64::from(step_index) * parameters.step_rise_mm
                        + parameters.thickness_mm
                        + 0.01,
                );
                let lifted_top_mm =
                    center_mm.z + parameters.jar_height_mm + parameters.lip_height_mm - 0.01;
                assert!(lifted_top_mm + parameters.cabinet_clearance_mm <= CABINET_DIMENSIONS_MM.z);
                let lift_volume = Shape::cylinder(
                    center_mm,
                    parameters.jar_diameter_mm / 2.0,
                    DVec3::Z,
                    parameters.jar_height_mm + parameters.lip_height_mm - 0.01,
                );
                assert_eq!(rack.intersect(&lift_volume).faces().count(), 0);
            }
        }
        Ok(())
    }
}
