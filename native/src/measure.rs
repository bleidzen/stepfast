use crate::import::Mesh;
use glam::{DVec3 as V, Mat4, Vec3};
type Triangle = [V; 3];
#[derive(Clone, Debug)]
pub struct Measurement {
    pub a: V,
    pub b: V,
    pub distance: f64,
}
fn triangle(mesh: &Mesh, start: usize) -> Triangle {
    std::array::from_fn(|i| {
        let v = &mesh.vertices[(start + i) * 6..];
        V::new(v[0] as f64, v[1] as f64, v[2] as f64)
    })
}

fn ray_triangle(origin: V, direction: V, t: Triangle) -> Option<f64> {
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let p = direction.cross(e2);
    let det = e1.dot(p);
    if det.abs() <= 1e-12 * e1.length() * e2.length() * direction.length() {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - t[0];
    let u = s.dot(p) * inv;
    if !(-1e-10..=1.0 + 1e-10).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = direction.dot(q) * inv;
    if v < -1e-10 || u + v > 1.0 + 1e-10 {
        return None;
    }
    let distance = e2.dot(q) * inv;
    if distance >= 0.0 {
        Some(distance)
    } else {
        None
    }
}
pub fn body_for_face(mesh: &Mesh, face: usize) -> usize {
    let start = mesh.faces[face][0];
    mesh.bodies
        .partition_point(|range| range[0] <= start)
        .saturating_sub(1)
}
pub fn pick(mesh: &Mesh, matrix: Mat4, ndc: [f32; 2], hidden: &[usize]) -> Option<usize> {
    let inverse = matrix.inverse();
    let a = inverse
        .project_point3(Vec3::new(ndc[0], ndc[1], -1.0))
        .as_dvec3();
    let b = inverse
        .project_point3(Vec3::new(ndc[0], ndc[1], 1.0))
        .as_dvec3();
    let direction = (b - a).normalize();
    let mut best = f64::INFINITY;
    let mut selected = None;
    for (face, range) in mesh.faces.iter().enumerate() {
        if hidden.contains(&body_for_face(mesh, face)) {
            continue;
        }
        for start in (range[0] as usize..(range[0] + range[1]) as usize).step_by(3) {
            if let Some(distance) = ray_triangle(a, direction, triangle(mesh, start)) {
                if distance < best {
                    best = distance;
                    selected = Some(face);
                }
            }
        }
    }
    selected
}
fn point_triangle(p: V, t: Triangle) -> V {
    let [a, b, c] = t;
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let sum = va + vb + vc;
    if sum.abs() < f64::MIN_POSITIVE {
        return a;
    }
    a + ab * (vb / sum) + ac * (vc / sum)
}
fn segment_pair(p: V, q: V, r: V, s: V) -> (V, V) {
    let d1 = q - p;
    let d2 = s - r;
    let delta = p - r;
    let a = d1.length_squared();
    let e = d2.length_squared();
    let f = d2.dot(delta);
    let (mut u, mut v);
    if a < 1e-24 && e < 1e-24 {
        return (p, r);
    }
    if a < 1e-24 {
        u = 0.0;
        v = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(delta);
        if e < 1e-24 {
            v = 0.0;
            u = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denominator = a * e - b * b;
            u = if denominator > 1e-24 {
                ((b * f - c * e) / denominator).clamp(0.0, 1.0)
            } else {
                0.0
            };
            v = (b * u + f) / e;
            if v < 0.0 {
                v = 0.0;
                u = (-c / a).clamp(0.0, 1.0);
            } else if v > 1.0 {
                v = 1.0;
                u = ((b - c) / a).clamp(0.0, 1.0);
            }
        }
    }
    (p + d1 * u, r + d2 * v)
}
fn consider(best: &mut Measurement, a: V, b: V) {
    let distance = a.distance(b);
    if distance < best.distance {
        *best = Measurement { a, b, distance };
    }
}
fn triangle_pair(a: Triangle, b: Triangle, best: &mut Measurement) {
    for i in 0..3 {
        if let Some(t) = ray_triangle(a[i], a[(i + 1) % 3] - a[i], b) {
            if t <= 1.0 {
                let p = a[i] + (a[(i + 1) % 3] - a[i]) * t;
                consider(best, p, p);
                return;
            }
        }
        if let Some(t) = ray_triangle(b[i], b[(i + 1) % 3] - b[i], a) {
            if t <= 1.0 {
                let p = b[i] + (b[(i + 1) % 3] - b[i]) * t;
                consider(best, p, p);
                return;
            }
        }
        consider(best, a[i], point_triangle(a[i], b));
        consider(best, point_triangle(b[i], a), b[i]);
        for j in 0..3 {
            let (p, q) = segment_pair(a[i], a[(i + 1) % 3], b[j], b[(j + 1) % 3]);
            consider(best, p, q);
        }
    }
}
struct Node {
    min: V,
    max: V,
    kind: Kind,
}
enum Kind {
    Leaf(Vec<Triangle>),
    Branch(Box<Node>, Box<Node>),
}
impl Node {
    fn new(mut triangles: Vec<Triangle>) -> Self {
        let mut min = V::splat(f64::INFINITY);
        let mut max = V::splat(f64::NEG_INFINITY);
        for tri in &triangles {
            for p in tri {
                min = min.min(*p);
                max = max.max(*p);
            }
        }
        let kind = if triangles.len() <= 8 {
            Kind::Leaf(triangles)
        } else {
            let size = max - min;
            let axis = if size.x >= size.y && size.x >= size.z {
                0
            } else if size.y >= size.z {
                1
            } else {
                2
            };
            triangles.sort_unstable_by(|a, b| {
                ((a[0] + a[1] + a[2])[axis]).total_cmp(&((b[0] + b[1] + b[2])[axis]))
            });
            let right = triangles.split_off(triangles.len() / 2);
            Kind::Branch(Box::new(Self::new(triangles)), Box::new(Self::new(right)))
        };
        Self { min, max, kind }
    }
    fn lower_bound(&self, other: &Self) -> f64 {
        (self.min - other.max)
            .max(other.min - self.max)
            .max(V::ZERO)
            .length()
    }
    fn visit(&self, other: &Self, best: &mut Measurement) {
        if self.lower_bound(other) >= best.distance {
            return;
        }
        match (&self.kind, &other.kind) {
            (Kind::Leaf(a), Kind::Leaf(b)) => {
                for x in a {
                    for y in b {
                        triangle_pair(*x, *y, best);
                    }
                }
            }
            (Kind::Branch(a, b), _) => {
                let (a, b) = if a.lower_bound(other) < b.lower_bound(other) {
                    (a, b)
                } else {
                    (b, a)
                };
                a.visit(other, best);
                b.visit(other, best);
            }
            (_, Kind::Branch(a, b)) => {
                let (a, b) = if self.lower_bound(a) < self.lower_bound(b) {
                    (a, b)
                } else {
                    (b, a)
                };
                self.visit(a, best);
                self.visit(b, best);
            }
        }
    }
}
pub fn between(mesh: &Mesh, a: usize, b: usize) -> Option<Measurement> {
    let face = |id: usize| {
        let range = mesh.faces[id];
        (range[0] as usize..(range[0] + range[1]) as usize)
            .step_by(3)
            .map(|i| triangle(mesh, i))
            .filter(|t| (t[1] - t[0]).cross(t[2] - t[0]).length_squared() > 1e-24)
            .collect::<Vec<_>>()
    };
    let a = face(a);
    let b = face(b);
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let mut best = Measurement {
        a: a[0][0],
        b: b[0][0],
        distance: a[0][0].distance(b[0][0]),
    };
    Node::new(a).visit(&Node::new(b), &mut best);
    Some(best)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picking_through_hidden_body() {
        let mut mesh = Mesh::default();
        for z in [-0.5, 0.5] {
            for [x, y] in [[-1.0, -1.0], [1.0, -1.0], [0.0, 1.0]] {
                mesh.vertices.extend_from_slice(&[x, y, z, 0.0, 0.0, 1.0]);
            }
        }
        mesh.faces = vec![[0, 3], [3, 3]];
        mesh.bodies = mesh.faces.clone();
        assert_eq!(pick(&mesh, Mat4::IDENTITY, [0.0, 0.0], &[]), Some(0));
        assert_eq!(pick(&mesh, Mat4::IDENTITY, [0.0, 0.0], &[0]), Some(1));
        assert_eq!(pick(&mesh, Mat4::IDENTITY, [0.0, 0.0], &[0, 1]), None);
        assert_eq!(pick(&mesh, Mat4::IDENTITY, [0.9, 0.9], &[]), None);
    }
    #[test]
    fn parallel_faces_and_intersection() {
        let a = [V::ZERO, V::X * 10.0, V::Y * 10.0];
        let b = a.map(|p| p + V::Z * 5.0);
        let mut best = Measurement {
            a: V::ZERO,
            b: V::ZERO,
            distance: f64::INFINITY,
        };
        triangle_pair(a, b, &mut best);
        assert!((best.distance - 5.0).abs() < 1e-10);
        let crossing = [
            V::new(1.0, 1.0, -1.0),
            V::new(1.0, 1.0, 1.0),
            V::new(2.0, 1.0, 1.0),
        ];
        triangle_pair(a, crossing, &mut best);
        assert_eq!(best.distance, 0.0);
    }
    #[test]
    fn edge_to_edge_minimum() {
        let (a, b) = segment_pair(
            V::new(-1.0, 0.0, 0.0),
            V::new(1.0, 0.0, 0.0),
            V::new(0.0, -1.0, 2.0),
            V::new(0.0, 1.0, 2.0),
        );
        assert!((a.distance(b) - 2.0).abs() < 1e-10);
    }
}
