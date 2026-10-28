use crate::import::Mesh;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use glam::{Mat4, Quat, Vec3};

pub struct Grid {
    pub visible: bool,
    pub alpha: f32,
    reference: Quat,
    min: Vec3,
    max: Vec3,
}
impl Default for Grid {
    fn default() -> Self {
        Self {
            visible: false,
            alpha: 0.0,
            reference: Quat::IDENTITY,
            min: Vec3::ZERO,
            max: Vec3::ZERO,
        }
    }
}
impl Grid {
    pub fn reset(&mut self, mesh: &Mesh, rotation: Quat) {
        self.min = Vec3::splat(f32::INFINITY);
        self.max = Vec3::splat(f32::NEG_INFINITY);
        for v in mesh.vertices.chunks_exact(6) {
            let p = Vec3::new(v[0], v[1], v[2]);
            self.min = self.min.min(p);
            self.max = self.max.max(p);
        }
        self.show(rotation);
    }
    pub fn show(&mut self, rotation: Quat) {
        self.visible = true;
        self.alpha = 1.0;
        self.reference = rotation;
    }
    pub fn update(&mut self, rotation: Quat, dt: f32) -> bool {
        let angle = 2.0 * self.reference.dot(rotation).abs().clamp(0.0, 1.0).acos();
        if self.visible && angle > 5.0_f32.to_radians() {
            self.visible = false;
        }
        let target = if self.visible { 1.0 } else { 0.0 };
        self.alpha += (target - self.alpha).clamp(-dt / 0.25, dt / 0.25);
        (self.alpha - target).abs() > 0.001
    }
    pub fn draw(&self, painter: &egui::Painter, rect: Rect, matrix: Mat4, labels: bool) {
        if self.alpha <= 0.001 {
            return;
        }
        let project = |v: Vec3| -> Option<Pos2> {
            let p = matrix * v.extend(1.0);
            if p.w <= 0.0 {
                return None;
            }
            let p = p.truncate() / p.w;
            if p.z < -1.0 || p.z > 1.0 {
                return None;
            }
            Some(egui::pos2(
                rect.left() + (p.x + 1.0) * rect.width() * 0.5,
                rect.top() + (1.0 - p.y) * rect.height() * 0.5,
            ))
        };
        let color = |rgb: [u8; 3], alpha: f32| {
            Color32::from_rgba_unmultiplied(
                rgb[0],
                rgb[1],
                rgb[2],
                (alpha * self.alpha * 255.0) as u8,
            )
        };
        let line = |a: Vec3, b: Vec3, c: Color32, width: f32| {
            if let (Some(a), Some(b)) = (project(a), project(b)) {
                painter.line_segment([a, b], Stroke::new(width, c));
            }
        };
        let size = self.max - self.min;
        let span = size.max_element().max(0.001);
        let margin = span * 0.12;
        let z = self.min.z - span * 0.004;
        let lo = self.min - Vec3::new(margin, margin, 0.0);
        let hi = self.max + Vec3::new(margin, margin, 0.0);
        let step = grid_step(span);
        if !labels {
            for axis in 0..2 {
                let start = (lo[axis] / step).floor() as i32;
                let end = (hi[axis] / step).ceil() as i32;
                for i in start..=end {
                    let v = i as f32 * step;
                    let (a, b) = if axis == 0 {
                        (Vec3::new(v, lo.y, z), Vec3::new(v, hi.y, z))
                    } else {
                        (Vec3::new(lo.x, v, z), Vec3::new(hi.x, v, z))
                    };
                    line(
                        a,
                        b,
                        color([85, 142, 180], if i % 5 == 0 { 0.36 } else { 0.14 }),
                        1.0,
                    );
                }
            }
            return;
        }
        let dimensions = [
            (
                Vec3::new(self.min.x, lo.y, z),
                Vec3::new(self.max.x, lo.y, z),
                format!("X  {:.2} mm", size.x),
                [115, 212, 239],
            ),
            (
                Vec3::new(hi.x, self.min.y, z),
                Vec3::new(hi.x, self.max.y, z),
                format!("Y  {:.2} mm", size.y),
                [135, 225, 193],
            ),
            (
                Vec3::new(hi.x, hi.y, self.min.z),
                Vec3::new(hi.x, hi.y, self.max.z),
                format!("Z  {:.2} mm", size.z),
                [190, 172, 245],
            ),
        ];
        for (a, b, text, rgb) in dimensions {
            let c = color(rgb, 0.95);
            line(a, b, c, 1.5);
            if let (Some(a), Some(b)) = (project(a), project(b)) {
                let direction = (b - a).normalized();
                let tick = egui::vec2(-direction.y, direction.x) * 5.0;
                painter.line_segment([a - tick, a + tick], Stroke::new(1.5, c));
                painter.line_segment([b - tick, b + tick], Stroke::new(1.5, c));
                let center = a + (b - a) * 0.5 + egui::vec2(0.0, -15.0);
                let galley = painter.layout_no_wrap(text, egui::FontId::proportional(14.0), c);
                let pos = center - galley.size() * 0.5;
                painter.rect_filled(
                    Rect::from_min_size(pos, galley.size()).expand(4.0),
                    4.0,
                    color([10, 24, 43], 0.95),
                );
                painter.galley(pos, galley, c);
            }
        }
        painter.text(
            rect.left_bottom() + egui::vec2(16.0, -16.0),
            egui::Align2::LEFT_BOTTOM,
            format!("XY GRID · {step} mm"),
            egui::FontId::proportional(12.0),
            color([130, 170, 198], 0.8),
        );
    }
}
fn grid_step(span: f32) -> f32 {
    let target = (span / 35.0).max(0.001);
    let base = 10.0_f32.powf(target.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|n| n * base)
        .find(|n| *n >= target)
        .unwrap()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fades_after_five_degrees_and_can_rearm() {
        let mut grid = Grid::default();
        grid.show(Quat::IDENTITY);
        grid.update(Quat::from_rotation_z(4.0_f32.to_radians()), 0.1);
        assert!(grid.visible);
        let rotated = Quat::from_rotation_z(6.0_f32.to_radians());
        grid.update(rotated, 0.1);
        assert!(!grid.visible);
        assert!(grid.alpha > 0.0 && grid.alpha < 1.0);
        grid.update(rotated, 0.2);
        assert_eq!(grid.alpha, 0.0);
        grid.show(rotated);
        grid.update(rotated, 0.1);
        assert!(grid.visible);
    }
    #[test]
    fn adaptive_grid_is_bounded() {
        for span in [0.01, 1.0, 100.0, 1e6] {
            assert!(span / grid_step(span) <= 35.01);
        }
    }
}
