use crate::import::Mesh;
use eframe::glow::{self, HasContext};
use glam::{Mat4, Quat, Vec3};

pub struct Camera {
    pub target: Vec3,
    pub rotation: Quat,
    pub distance: f32,
    pub radius: f32,
}
impl Default for Camera {
    fn default() -> Self {
        let mut s = Self {
            target: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            distance: 4.0,
            radius: 1.0,
        };
        s.iso();
        s
    }
}
impl Camera {
    pub fn home(&mut self, aspect: f32) {
        self.iso();
        self.fit(aspect);
    }
    pub fn iso(&mut self) {
        self.rotation = Quat::from_mat4(
            &Mat4::look_at_rh(Vec3::new(1.0, -1.0, 0.8), Vec3::ZERO, Vec3::Z).inverse(),
        );
    }
    pub fn fit(&mut self, aspect: f32) {
        self.target = Vec3::ZERO;
        self.distance = self.radius * 1.2 / (0.35_f32.min((0.35_f32.tan() * aspect).atan())).sin();
    }
    pub fn matrix(&self, aspect: f32) -> Mat4 {
        let position = self.target + self.rotation * Vec3::new(0.0, 0.0, self.distance);
        let view = Mat4::from_rotation_translation(self.rotation, position).inverse();
        Mat4::perspective_rh_gl(
            0.7,
            aspect.max(0.01),
            self.radius * 0.0001,
            (self.distance + self.radius * 100.0).max(self.radius),
        ) * view
    }
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.rotation = (Quat::from_axis_angle(Vec3::Z, -dx * 0.007)
            * self.rotation
            * Quat::from_rotation_x(-dy * 0.007))
        .normalize();
    }
    pub fn pan(&mut self, dx: f32, dy: f32, height: f32) {
        self.target +=
            self.rotation * Vec3::new(-dx, dy, 0.0) * (self.distance * 0.73 / height.max(1.0));
    }
    pub fn zoom(&mut self, amount: f32) {
        self.distance = (self.distance * (-amount * 0.002).exp())
            .clamp(self.radius * 0.001, self.radius * 100.0);
    }
    pub fn space(&mut self, axes: [f32; 6], dt: f32, speed: f32) {
        let scale = dt * speed;
        // Object navigation: the model follows the cap in screen space.
        // HID Y is depth and HID Z points down; depth changes zoom, not pivot.
        self.target +=
            self.rotation * Vec3::new(-axes[0], axes[2], 0.0) * self.distance * 0.35 * scale;
        self.distance = (self.distance * (axes[1] * scale).exp())
            .clamp(self.radius * 0.001, self.radius * 100.0);
        self.rotation = (self.rotation
            * Quat::from_euler(
                glam::EulerRot::XYZ,
                -axes[3] * scale,
                axes[5] * scale,
                -axes[4] * scale,
            ))
        .normalize();
    }
}

pub struct Renderer {
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    count: i32,
}
pub fn visible_ranges(bodies: &[[u32; 2]], hidden: &[usize]) -> Vec<[u32; 2]> {
    let mut ranges: Vec<[u32; 2]> = Vec::new();
    for (id, range) in bodies.iter().enumerate() {
        if hidden.contains(&id) {
            continue;
        }
        if let Some(last) = ranges.last_mut() {
            if last[0] + last[1] == range[0] {
                last[1] += range[1];
                continue;
            }
        }
        ranges.push(*range);
    }
    ranges
}

// Alternating cool and warm accents against the navy workspace. Further bodies
// use golden-angle hues rather than repeating the first eight colors.
fn body_color(index: usize) -> [f32; 3] {
    const PALETTE: [[f32; 3]; 8] = [
        [0.36, 0.76, 0.88],
        [0.94, 0.65, 0.43],
        [0.65, 0.60, 0.89],
        [0.40, 0.81, 0.68],
        [0.89, 0.53, 0.65],
        [0.51, 0.68, 0.95],
        [0.86, 0.78, 0.46],
        [0.58, 0.80, 0.81],
    ];
    if index < PALETTE.len() {
        return PALETTE[index];
    }
    let h = (0.56 + index as f32 * 0.618034).fract() * 6.0;
    let v = 0.86;
    let p = 0.39;
    let q = v - (v - p) * h.fract();
    let t = p + (v - p) * h.fract();
    match h as usize {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hidden_ranges() {
        let bodies = [[0, 3], [3, 6], [9, 3]];
        let mut hidden = vec![1, 0];
        assert_eq!(visible_ranges(&bodies, &hidden), vec![[9, 3]]);
        hidden.pop();
        assert_eq!(visible_ranges(&bodies, &hidden), vec![[0, 3], [9, 3]]);
        hidden.pop();
        assert_eq!(visible_ranges(&bodies, &hidden), vec![[0, 12]]);
        assert!(visible_ranges(&bodies, &[0, 1, 2]).is_empty());
    }
    #[test]
    fn palette_and_zoom_defaults() {
        for i in 0..32 {
            let color = body_color(i);
            assert!(color.iter().all(|c| *c >= 0.0 && *c <= 1.0));
            for j in 0..i {
                assert_ne!(color, body_color(j));
            }
        }
        assert_eq!(
            crate::spacemouse::Settings::default().apply([1.0; 6]),
            [1.0, -1.0, 1.0, 1.0, 1.0, 1.0]
        );
    }
    #[test]
    fn space_pan_follows_cap_and_zoom_preserves_pivot() {
        let mut camera = Camera::default();
        camera.rotation = Quat::IDENTITY;
        camera.space([1.0, 0.0, 0.0, 0.0, 0.0, 0.0], 0.1, 1.0);
        assert!(camera.target.x < 0.0); // Model moves right on screen.
        camera.space([0.0, 0.0, -1.0, 0.0, 0.0, 0.0], 0.1, 1.0);
        assert!(camera.target.y < 0.0); // Lifting cap moves model up.
        let target = camera.target;
        let distance = camera.distance;
        camera.space([0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 0.1, 1.0);
        assert_eq!(camera.target, target);
        assert!(camera.distance > distance);
        let settings = crate::spacemouse::Settings {
            invert: [true, false, true, false, false, true],
        };
        assert_eq!(settings.apply([1.0; 6]), [-1.0, 1.0, -1.0, 1.0, 1.0, -1.0]);
    }
    #[test]
    fn home_resets_pan_rotation_and_zoom() {
        let mut camera = Camera::default();
        camera.radius = 10.0;
        let iso = camera.rotation;
        camera.orbit(80.0, -24.0);
        camera.pan(100.0, 20.0, 800.0);
        camera.zoom(100.0);
        camera.home(1.5);
        assert!(camera.rotation.abs_diff_eq(iso, 1e-6));
        assert_eq!(camera.target, Vec3::ZERO);
        assert!(camera.distance > camera.radius * 2.0);
    }
}
impl Renderer {
    pub unsafe fn new(gl: &glow::Context) -> Result<Self, String> {
        let program = gl.create_program()?;
        for (kind, source) in [
            (glow::VERTEX_SHADER, include_str!("vertex.glsl")),
            (glow::FRAGMENT_SHADER, include_str!("fragment.glsl")),
        ] {
            let shader = gl.create_shader(kind)?;
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                return Err(gl.get_shader_info_log(shader));
            }
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(gl.get_program_info_log(program));
        }
        let vao = gl.create_vertex_array()?;
        let vbo = gl.create_buffer()?;
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        for (i, offset) in [(0, 0), (1, 12), (2, 24)] {
            gl.enable_vertex_attrib_array(i);
            gl.vertex_attrib_pointer_f32(i, 3, glow::FLOAT, false, 36, offset);
        }
        gl.bind_vertex_array(None);
        Ok(Self {
            program,
            vao,
            vbo,
            count: 0,
        })
    }
    pub unsafe fn upload(&mut self, gl: &glow::Context, mesh: &Mesh) {
        let mut colored = Vec::with_capacity(mesh.vertices.len() / 6 * 9);
        for (i, range) in mesh.bodies.iter().enumerate() {
            let color = body_color(i);
            for vertex in mesh.vertices
                [range[0] as usize * 6..(range[0] as usize + range[1] as usize) * 6]
                .chunks_exact(6)
            {
                colored.extend_from_slice(vertex);
                colored.extend_from_slice(&color);
            }
        }
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(&colored),
            glow::STATIC_DRAW,
        );
        self.count = (mesh.vertices.len() / 6) as i32;
    }
    pub unsafe fn paint(
        &self,
        gl: &glow::Context,
        matrix: Mat4,
        wire: bool,
        highlights: &[[u32; 2]],
        visible: &[[u32; 2]],
    ) {
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LEQUAL);
        gl.depth_mask(true);
        gl.clear_depth_f32(1.0);
        gl.clear(glow::DEPTH_BUFFER_BIT);
        gl.disable(glow::CULL_FACE);
        gl.disable(glow::BLEND);
        gl.use_program(Some(self.program));
        gl.uniform_matrix_4_f32_slice(
            gl.get_uniform_location(self.program, "mvp").as_ref(),
            false,
            &matrix.to_cols_array(),
        );
        gl.bind_vertex_array(Some(self.vao));
        let tint = gl.get_uniform_location(self.program, "tint");
        let selected = gl.get_uniform_location(self.program, "selected");
        gl.uniform_1_i32(selected.as_ref(), 0);
        gl.uniform_3_f32(tint.as_ref(), 0.69, 0.82, 0.72);
        if wire {
            gl.polygon_mode(glow::FRONT_AND_BACK, glow::LINE);
        }
        for range in visible {
            gl.draw_arrays(glow::TRIANGLES, range[0] as i32, range[1] as i32);
        }
        gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
        gl.enable(glow::POLYGON_OFFSET_FILL);
        gl.polygon_offset(-1.0, -1.0);
        gl.uniform_1_i32(selected.as_ref(), 1);
        for (i, face) in highlights.iter().enumerate() {
            let color = if i == 0 {
                [0.2, 0.65, 1.0]
            } else {
                [1.0, 0.65, 0.15]
            };
            gl.uniform_3_f32(tint.as_ref(), color[0], color[1], color[2]);
            gl.draw_arrays(glow::TRIANGLES, face[0] as i32, face[1] as i32);
        }
        gl.disable(glow::POLYGON_OFFSET_FILL);
        gl.bind_vertex_array(None);
        gl.disable(glow::DEPTH_TEST);
    }
    pub unsafe fn destroy(&self, gl: &glow::Context) {
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
        gl.delete_buffer(self.vbo);
    }
}
