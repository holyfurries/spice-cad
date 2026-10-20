use eframe::egui::{Color32, ColorImage};
use glam::Vec3;

pub fn triangle(
    image: &mut ColorImage,
    depth: &mut [f32],
    points: [Vec3; 3],
    color: Color32,
    remaining: &mut usize,
) -> Option<()> {
    let [width, height] = image.size;
    assert_eq!(depth.len(), image.pixels.len());
    assert!(width <= 800 && height <= 600);
    if points.iter().any(|point| !point.is_finite()) {
        return None;
    }
    let [a, b, c] = points;
    let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if area.abs() < 1e-6 {
        return Some(());
    }
    let minimum = a.min(b).min(c);
    let maximum = a.max(b).max(c);
    let left = minimum.x.floor().clamp(0.0, width as f32) as usize;
    let right = maximum.x.ceil().clamp(0.0, width as f32) as usize;
    let top = minimum.y.floor().clamp(0.0, height as f32) as usize;
    let bottom = maximum.y.ceil().clamp(0.0, height as f32) as usize;
    let visits = (right - left) * (bottom - top);
    if visits > *remaining {
        return None;
    }
    *remaining -= visits;
    for y in top..bottom {
        for x in left..right {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let first = ((b.x - px) * (c.y - py) - (b.y - py) * (c.x - px)) / area;
            let second = ((c.x - px) * (a.y - py) - (c.y - py) * (a.x - px)) / area;
            let third = 1.0 - first - second;
            if first < 0.0 || second < 0.0 || third < 0.0 {
                continue;
            }
            let z = first * a.z + second * b.z + third * c.z;
            let index = y * width + x;
            if z < depth[index] {
                depth[index] = z;
                image.pixels[index] = color;
            }
        }
    }
    Some(())
}
