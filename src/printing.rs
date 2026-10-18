use glam::{DVec3, dvec3};
use opencascade::primitives::{Shape, Wire};

use crate::{RackParameters, build_rack};

pub const JOINT_CLEARANCE_MM: f64 = 0.2;
const JOINT_LENGTH_MM: f64 = 6.0;
const JOINT_NECK_HALF_MM: f64 = 4.0;
const JOINT_HEAD_HALF_MM: f64 = 8.0;
const P1S_VOLUME_MM: DVec3 = dvec3(256.0, 256.0, 256.0);
const PRINT_MARGIN_MM: DVec3 = dvec3(10.0, 10.0, 0.0);

const _: () = {
    assert!(JOINT_HEAD_HALF_MM > JOINT_NECK_HALF_MM);
    assert!(JOINT_HEAD_HALF_MM - JOINT_NECK_HALF_MM < JOINT_LENGTH_MM);
};

pub struct PrintPart {
    pub name: &'static str,
    pub shape: Shape,
    side: Side,
}

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

impl PrintPart {
    pub fn on_bed(&self) -> Result<(Shape, DVec3), String> {
        let angle_radians = match self.side {
            Side::Left => -std::f64::consts::FRAC_PI_2,
            Side::Right => std::f64::consts::FRAC_PI_2,
        };
        let rotated = self.shape.rotated(DVec3::Y, angle_radians);
        let mesh = rotated
            .mesh()
            .map_err(|error| format!("{}: {error}", self.name))?;
        if mesh.vertices.is_empty() {
            return Err(format!("{} has no mesh vertices", self.name));
        }
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
        let dimensions_mm = maximum_mm - minimum_mm;
        if !dimensions_mm.is_finite() {
            return Err(format!("{} has non-finite dimensions", self.name));
        }
        if dimensions_mm.min_element() <= 0.0 {
            return Err(format!("{} has an empty dimension", self.name));
        }
        if dimensions_mm.cmpgt(P1S_VOLUME_MM - PRINT_MARGIN_MM).any() {
            return Err(format!(
                "{} exceeds the P1S print envelope: {dimensions_mm}",
                self.name
            ));
        }
        assert!(minimum_mm.is_finite());
        assert!(dimensions_mm.max_element() <= P1S_VOLUME_MM.max_element());
        Ok((rotated.translated(-minimum_mm), dimensions_mm))
    }
}

pub fn build_parts(parameters: RackParameters) -> Result<[PrintPart; 4], String> {
    let layout = parameters.layout()?;
    if !(0.05..=0.5).contains(&JOINT_CLEARANCE_MM) {
        return Err("JOINT_CLEARANCE_MM must be finite and between 0.05 and 0.5".into());
    }
    let front_step_count = parameters.step_count.div_ceil(2);
    let front_depth_mm = f64::from(front_step_count) * layout.tread_depth_mm;
    let front_height_mm = f64::from(front_step_count - 1) * parameters.step_rise_mm
        + parameters.thickness_mm
        + parameters.lip_height_mm;
    if front_height_mm < 36.0 {
        return Err("front side panel must be at least 36 mm tall for two dovetails".into());
    }
    assert!(front_depth_mm < layout.dimensions_mm.y);
    let seam_x_mm = parameters.rack_width_mm / 2.0;
    assert!(seam_x_mm > JOINT_LENGTH_MM);
    let blank = build_rack(parameters)?;
    let mut left: Shape = blank
        .intersect(&Shape::box_from_corners(
            DVec3::ZERO,
            dvec3(seam_x_mm, layout.dimensions_mm.y, layout.dimensions_mm.z),
        ))
        .into();
    let mut right: Shape = blank
        .intersect(&Shape::box_from_corners(
            dvec3(seam_x_mm, 0.0, 0.0),
            layout.dimensions_mm,
        ))
        .into();
    for step_index in 0..parameters.step_count {
        let center_y_mm = (f64::from(step_index) + 0.5) * layout.tread_depth_mm;
        let floor_z_mm = f64::from(step_index) * parameters.step_rise_mm;
        let floor_count = if step_index == 0 { 1 } else { 2 };
        for floor_index in 0..floor_count {
            let z_mm = if floor_index == 0 { 0.0 } else { floor_z_mm };
            let origin_mm = dvec3(seam_x_mm, center_y_mm, z_mm);
            let tongue = dovetail(
                origin_mm,
                DVec3::X,
                DVec3::Y,
                DVec3::Z * parameters.thickness_mm,
                0.0,
            )?;
            let pocket = dovetail(
                origin_mm - DVec3::Z * 0.1,
                DVec3::X,
                DVec3::Y,
                DVec3::Z * (parameters.thickness_mm + 0.2),
                JOINT_CLEARANCE_MM,
            )?;
            left = left.union(&tongue).into();
            right = right.subtract(&pocket).into();
        }
    }
    let split_half =
        |half: Shape, side: Side, names: [&'static str; 2]| -> Result<[PrintPart; 2], String> {
            let mut front: Shape = half
                .intersect(&Shape::box_from_corners(
                    DVec3::ZERO,
                    dvec3(
                        layout.dimensions_mm.x,
                        front_depth_mm,
                        layout.dimensions_mm.z,
                    ),
                ))
                .into();
            let mut rear: Shape = half
                .intersect(&Shape::box_from_corners(
                    dvec3(0.0, front_depth_mm, 0.0),
                    layout.dimensions_mm,
                ))
                .into();
            let x_mm = match side {
                Side::Left => 0.0,
                Side::Right => parameters.rack_width_mm - parameters.thickness_mm,
            };
            for height_fraction in [0.25, 0.75] {
                let origin_mm = dvec3(x_mm, front_depth_mm, front_height_mm * height_fraction);
                let tongue = dovetail(
                    origin_mm,
                    DVec3::Y,
                    DVec3::Z,
                    DVec3::X * parameters.thickness_mm,
                    0.0,
                )?;
                let pocket = dovetail(
                    origin_mm - DVec3::X * 0.1,
                    DVec3::Y,
                    DVec3::Z,
                    DVec3::X * (parameters.thickness_mm + 0.2),
                    JOINT_CLEARANCE_MM,
                )?;
                front = front.union(&tongue).into();
                rear = rear.subtract(&pocket).into();
            }
            Ok([
                PrintPart {
                    name: names[0],
                    shape: front.clean(),
                    side,
                },
                PrintPart {
                    name: names[1],
                    shape: rear.clean(),
                    side,
                },
            ])
        };
    let [front_left, rear_left] = split_half(left, Side::Left, ["front-left", "rear-left"])?;
    let [front_right, rear_right] = split_half(right, Side::Right, ["front-right", "rear-right"])?;
    let parts = [front_left, rear_left, front_right, rear_right];
    for part in &parts {
        part.on_bed()?;
    }
    Ok(parts)
}

pub fn assemble_parts(parts: &[PrintPart; 4]) -> Shape {
    assert!(parts.iter().all(|part| !part.name.is_empty()));
    let mut assembled = parts[0].shape.translated(DVec3::ZERO);
    for part in &parts[1..] {
        assembled = assembled.union(&part.shape).into();
    }
    let assembled = assembled.clean();
    assert!(assembled.faces().next().is_some());
    assembled
}

pub fn build_fit_samples(thickness_mm: f64) -> Result<[PrintPart; 4], String> {
    assert!(thickness_mm.is_finite());
    assert!(thickness_mm > 0.0);
    let mut samples = Vec::with_capacity(4);
    for (direction, transverse, extrusion, female_side, names) in [
        (
            DVec3::X,
            DVec3::Y,
            DVec3::Z,
            Side::Right,
            ["fit-width-male", "fit-width-female"],
        ),
        (
            DVec3::Y,
            DVec3::Z,
            DVec3::X,
            Side::Left,
            ["fit-depth-male", "fit-depth-female"],
        ),
    ] {
        let origin_mm = direction * 12.0 + transverse * 12.0;
        let male: Shape = Shape::box_from_corners(
            DVec3::ZERO,
            direction * 12.0 + transverse * 24.0 + extrusion * thickness_mm,
        )
        .union(&dovetail(
            origin_mm,
            direction,
            transverse,
            extrusion * thickness_mm,
            0.0,
        )?)
        .into();
        let female: Shape = Shape::box_from_corners(
            direction * 12.0,
            direction * 24.0 + transverse * 24.0 + extrusion * thickness_mm,
        )
        .subtract(&dovetail(
            origin_mm - extrusion * 0.1,
            direction,
            transverse,
            extrusion * (thickness_mm + 0.2),
            JOINT_CLEARANCE_MM,
        )?)
        .into();
        samples.push(PrintPart {
            name: names[0],
            shape: male.clean(),
            side: Side::Left,
        });
        samples.push(PrintPart {
            name: names[1],
            shape: female.clean(),
            side: female_side,
        });
    }
    samples
        .try_into()
        .map_err(|_| "expected four fit samples".into())
}

fn dovetail(
    origin_mm: DVec3,
    direction: DVec3,
    transverse: DVec3,
    extrusion_mm: DVec3,
    clearance_mm: f64,
) -> Result<Shape, String> {
    assert!(origin_mm.is_finite());
    assert!(clearance_mm >= 0.0);
    assert_eq!(direction.dot(transverse), 0.0);
    assert_eq!(extrusion_mm.dot(direction), 0.0);
    assert_eq!(extrusion_mm.dot(transverse), 0.0);
    let points = [
        (-1.0, -JOINT_NECK_HALF_MM - clearance_mm),
        (
            JOINT_LENGTH_MM + clearance_mm,
            -JOINT_HEAD_HALF_MM - clearance_mm,
        ),
        (
            JOINT_LENGTH_MM + clearance_mm,
            JOINT_HEAD_HALF_MM + clearance_mm,
        ),
        (-1.0, JOINT_NECK_HALF_MM + clearance_mm),
    ]
    .map(|(length_mm, width_mm)| origin_mm + direction * length_mm + transverse * width_mm);
    let wire = Wire::from_ordered_points(points).map_err(|error| error.to_string())?;
    Ok(wire.to_face().extrude(extrusion_mm).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PARAMETERS;

    #[test]
    fn rejects_parts_without_room_for_joints_or_the_print_bed() {
        for parameters in [
            RackParameters {
                step_count: 1,
                ..PARAMETERS
            },
            RackParameters {
                step_count: 3,
                step_rise_mm: 10.0,
                ..PARAMETERS
            },
            RackParameters {
                step_rise_mm: 70.0,
                jar_height_mm: 20.0,
                ..PARAMETERS
            },
        ] {
            assert!(parameters.layout().is_ok());
            assert!(build_parts(parameters).is_err());
        }
    }

    #[test]
    fn dovetails_and_assembly_moves_have_no_material_collisions()
    -> Result<(), Box<dyn std::error::Error>> {
        let parts = build_parts(PARAMETERS)?;
        let samples = build_fit_samples(PARAMETERS.thickness_mm)?;
        for [male, female] in samples.as_chunks::<2>().0 {
            assert!(collision_volume_mm3(&male.shape, &male.shape)? > 0.0);
            assert!(collision_volume_mm3(&male.shape, &female.shape)? < 1e-6);
        }
        for (front, rear, direction) in [
            (&parts[0], &parts[1], -DVec3::X),
            (&parts[2], &parts[3], DVec3::X),
        ] {
            for offset_index in 0..=12 {
                let offset_mm =
                    direction * f64::from(offset_index) * PARAMETERS.thickness_mm / 12.0;
                assert!(
                    collision_volume_mm3(&front.shape, &rear.shape.translated(offset_mm))? < 1e-6
                );
            }
        }
        let left: Shape = parts[0].shape.union(&parts[1].shape).into();
        let right: Shape = parts[2].shape.union(&parts[3].shape).into();
        for lift_index in 0..=36 {
            let lift_mm = DVec3::Z * f64::from(lift_index) * 5.0;
            assert!(collision_volume_mm3(&left, &right.translated(lift_mm))? < 1e-6);
        }
        let blank = build_rack(PARAMETERS)?;
        let outside: Shape = assemble_parts(&parts).subtract(&blank).into();
        assert_eq!(outside.faces().count(), 0);
        Ok(())
    }

    fn collision_volume_mm3(first: &Shape, second: &Shape) -> Result<f64, opencascade::Error> {
        let intersection: Shape = first.intersect(second).into();
        if intersection.faces().next().is_none() {
            return Ok(0.0);
        }
        let mesh = intersection.mesh()?;
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.indices.len() % 3, 0);
        let origin_mm = mesh
            .vertices
            .iter()
            .copied()
            .fold(DVec3::splat(f64::INFINITY), DVec3::min);
        let volume_mm3 = mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                let [first, second, third] = triangle.map(|index| mesh.vertices[index] - origin_mm);
                first.dot(second.cross(third)) / 6.0
            })
            .sum::<f64>()
            .abs();
        assert!(volume_mm3.is_finite());
        Ok(volume_mm3)
    }
}
