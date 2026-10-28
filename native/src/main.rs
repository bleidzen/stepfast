#![cfg_attr(not(test), windows_subsystem = "windows")]
mod grid;
mod import;
mod measure;
mod progress;
mod render;
mod spacemouse;
use eframe::{egui, egui_glow, glow};
use render::{Camera, Renderer};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

fn main() -> eframe::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|s| s == "--import") {
        if args.len() != 5 {
            std::process::exit(2);
        }
        let output = PathBuf::from(&args[3]);
        let result =
            std::panic::catch_unwind(|| import::load(&PathBuf::from(&args[2]), args[4] == "fine"))
                .unwrap_or_else(|_| {
                    Err("STEP-geometrin kunde inte behandlas av importmotorn".into())
                });
        match result {
            Ok(mesh) => {
                if std::fs::write(&output, bincode::serialize(&mesh).unwrap()).is_err() {
                    std::process::exit(3);
                }
            }
            Err(error) => {
                let _ = std::fs::write(output.with_extension("error"), error);
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    let file = args.get(1).map(PathBuf::from);
    eframe::run_native(
        "STEP / FAST",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_icon(egui::IconData {
                    rgba: include_bytes!("../assets/stepfast.rgba").to_vec(),
                    width: 256,
                    height: 256,
                })
                .with_inner_size([1280.0, 820.0])
                .with_min_inner_size([720.0, 480.0])
                .with_drag_and_drop(true),
            renderer: eframe::Renderer::Glow,
            depth_buffer: 24,
            multisampling: 4,
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(Viewer::new(cc, file)?))),
    )
}

struct Job {
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<import::Mesh, String>>,
    started: Instant,
    path: PathBuf,
    preview: mpsc::Receiver<import::Mesh>,
    stage: Arc<Mutex<String>>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
fn terminate_import(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Kill only this importer's tree, including its optional OCCT bridge.
        let _ = Command::new("taskkill.exe")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(0x08000000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}
fn start_job(path: PathBuf, fine: bool, ctx: egui::Context) -> Job {
    let (tx, rx) = mpsc::channel();
    let (preview_tx, preview) = mpsc::sync_channel(1);
    let stage = Arc::new(Mutex::new("Läser STEP-strukturen…".to_string()));
    let shared_stage = stage.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let stopped = cancel.clone();
    let input = path.clone();
    std::thread::spawn(move || {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let output = std::env::temp_dir().join(format!(
            "step-fast-{}-{}.mesh",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let run = || -> Result<import::Mesh, String> {
            let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
            command
                .arg("--import")
                .arg(&input)
                .arg(&output)
                .arg(if fine { "fine" } else { "fast" })
                .env("STEP_FAST_PROGRESS", output.with_extension("preview"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command
                .spawn()
                .map_err(|e| format!("Kunde inte starta import: {e}"))?;
            let mut last_preview = None;
            let mut last_poll = Instant::now();
            loop {
                if last_poll.elapsed() >= Duration::from_millis(80) {
                    last_poll = Instant::now();
                    let path = output.with_extension("preview");
                    if let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) {
                        if Some(modified) != last_preview {
                            if let Ok(mesh) = std::fs::read(&path)
                                .map_err(|e| e.to_string())
                                .and_then(|b| import::decode(&b))
                            {
                                let _ = preview_tx.try_send(mesh);
                                last_preview = Some(modified);
                                ctx.request_repaint();
                            }
                        }
                    }
                    if let Ok(text) = std::fs::read_to_string(output.with_extension("status")) {
                        *shared_stage.lock().unwrap() = text;
                    }
                }
                if stopped.load(Ordering::Relaxed) {
                    terminate_import(&mut child);
                    return Err("Avbruten".into());
                }
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if !status.success() {
                            return Err(std::fs::read_to_string(output.with_extension("error"))
                                .unwrap_or_else(|_| "Importprocessen avslutades oväntat".into()));
                        }
                        return import::decode(&std::fs::read(&output).map_err(|e| e.to_string())?);
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                    Err(e) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(e.to_string());
                    }
                }
            }
        };
        let result = run();
        let _ = std::fs::remove_file(&output);
        let _ = std::fs::remove_file(output.with_extension("error"));
        for extension in ["preview", "partial", "status"] {
            let _ = std::fs::remove_file(output.with_extension(extension));
        }
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    Job {
        cancel,
        rx,
        started: Instant::now(),
        path,
        preview,
        stage,
    }
}

struct Viewer {
    renderer: Arc<Mutex<Renderer>>,
    camera: Camera,
    grid: grid::Grid,
    job: Option<Job>,
    file: Option<PathBuf>,
    initial: Option<PathBuf>,
    status: String,
    error: Option<String>,
    triangles: usize,
    parts: usize,
    fine: bool,
    wire: bool,
    space: Arc<Mutex<spacemouse::State>>,
    speed: f32,
    navigation: spacemouse::Settings,
    last_frame: Instant,
    aspect: f32,
    mesh: Option<Arc<import::Mesh>>,
    measuring: bool,
    selected: Vec<usize>,
    hidden: Vec<usize>,
    measurement: Option<measure::Measurement>,
    measure_rx: Option<mpsc::Receiver<Option<measure::Measurement>>>,
}
impl Viewer {
    fn new(
        cc: &eframe::CreationContext<'_>,
        initial: Option<PathBuf>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let gl = cc.gl.as_ref().ok_or("OpenGL kunde inte starta")?;
        let renderer = unsafe { Renderer::new(gl).map_err(std::io::Error::other)? };
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(13, 24, 44);
        visuals.window_fill = egui::Color32::from_rgb(20, 35, 59);
        visuals.selection.bg_fill = egui::Color32::from_rgb(36, 91, 137);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(27, 45, 70);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(38, 70, 103);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(44, 99, 140);
        visuals.override_text_color = Some(egui::Color32::from_rgb(214, 228, 244));
        cc.egui_ctx.set_visuals(visuals);
        Ok(Self {
            renderer: Arc::new(Mutex::new(renderer)),
            camera: Camera::default(),
            grid: grid::Grid::default(),
            job: None,
            file: None,
            initial,
            status: "Redo att öppna".into(),
            error: None,
            triangles: 0,
            parts: 0,
            fine: false,
            wire: false,
            space: spacemouse::start(cc.egui_ctx.clone()),
            speed: 3.0,
            navigation: spacemouse::Settings::load(),
            last_frame: Instant::now(),
            aspect: 1.5,
            mesh: None,
            measuring: false,
            selected: Vec::new(),
            hidden: Vec::new(),
            measurement: None,
            measure_rx: None,
        })
    }
    fn open(&mut self, path: PathBuf, ctx: &egui::Context) {
        if !path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("step") || e.eq_ignore_ascii_case("stp"))
        {
            self.error = Some("Välj en .step- eller .stp-fil".into());
            return;
        }
        self.job = Some(start_job(path.clone(), self.fine, ctx.clone()));
        self.mesh = None;
        self.triangles = 0;
        self.parts = 0;
        self.hidden.clear();
        self.selected.clear();
        self.measurement = None;
        self.measure_rx = None;
        self.grid = grid::Grid::default();
        self.file = Some(path);
        self.error = None;
        self.status = "Läser STEP och bygger geometri…".into();
    }
    fn browse(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("STEP", &["step", "stp"])
            .pick_file()
        {
            self.open(path, ctx);
        }
    }
}
impl eframe::App for Viewer {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(path) = self.initial.take() {
            self.open(path, ctx);
        }
        let preview = self
            .job
            .as_ref()
            .and_then(|job| job.preview.try_iter().last());
        if let Some(mesh) = preview {
            if self.mesh.is_none() {
                self.camera.radius = mesh.radius;
                self.camera.home(self.aspect);
            }
            self.triangles = mesh.vertices.len() / 18;
            self.parts = mesh.parts;
            unsafe {
                self.renderer
                    .lock()
                    .unwrap()
                    .upload(frame.gl().unwrap(), &mesh);
            }
            self.status = format!(
                "Bygger modell · {} färdiga ytor · {} trianglar",
                mesh.faces.len(),
                self.triangles
            );
            self.mesh = Some(Arc::new(mesh));
        }
        let result = self.job.as_ref().and_then(|job| match job.rx.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Importprocessen svarar inte".into()))
            }
            _ => None,
        });
        if let Some(result) = result {
            let job = self.job.take().unwrap();
            match result {
                Ok(mesh) => {
                    self.selected.clear();
                    self.measurement = None;
                    self.measure_rx = None;
                    self.camera.radius = mesh.radius;
                    self.camera.iso();
                    self.camera.fit(self.aspect);
                    self.triangles = mesh.vertices.len() / 18;
                    self.parts = mesh.parts;
                    unsafe {
                        self.renderer
                            .lock()
                            .unwrap()
                            .upload(frame.gl().unwrap(), &mesh);
                    }
                    self.status = format!(
                        "{}  ·  {:.2} s{} · {}",
                        job.path.file_name().unwrap_or_default().to_string_lossy(),
                        job.started.elapsed().as_secs_f32(),
                        if mesh.cached { "  ·  cache" } else { "" },
                        mesh.backend
                    );
                    self.hidden.clear();
                    self.grid.reset(&mesh, self.camera.rotation);
                    self.mesh = Some(Arc::new(mesh));
                }
                Err(error) => {
                    self.mesh = None;
                    self.triangles = 0;
                    self.parts = 0;
                    self.status = "Kunde inte öppna filen".into();
                    self.error = Some(error);
                }
            }
        }
        if let Some(rx) = &self.measure_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.measurement = result;
                    self.measure_rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.measure_rx = None;
                    self.status = "Mätningen kunde inte slutföras".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
            self.camera.home(self.aspect);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::M)) {
            self.measuring = !self.measuring;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::O)) {
            self.browse(ctx);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F)) {
            self.camera.fit(self.aspect);
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
        if let Some(path) = dropped {
            self.open(path, ctx);
        }
        let dt = self.last_frame.elapsed().as_secs_f32().min(0.05);
        self.last_frame = Instant::now();
        let mut space_status = "SpaceMouse: väntar på rörelse".to_string();
        if let Ok(mut state) = self.space.lock() {
            if let Some(error) = &state.error {
                space_status = error.clone();
            } else if state.seen {
                space_status = "SpaceMouse: upptäckt".into();
            }
            if state.fit {
                self.camera.home(self.aspect);
                state.fit = false;
            }
            if ctx.input(|i| i.focused)
                && state.last.elapsed() < Duration::from_millis(120)
                && state.axes.iter().any(|v| *v != 0.0)
            {
                self.camera
                    .space(self.navigation.apply(state.axes), dt, self.speed);
                ctx.request_repaint_after(Duration::from_millis(8));
            }
        }
        egui::TopBottomPanel::top("toolbar")
            .exact_height(62.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(14.0);
                    ui.heading(
                        egui::RichText::new("STEP / FAST")
                            .color(egui::Color32::from_rgb(110, 211, 239)),
                    );
                    ui.add_space(20.0);
                    if ui.button("Öppna STEP  [O]").clicked() {
                        self.browse(ctx);
                    }
                    let changed = ui.checkbox(&mut self.fine, "Fin detalj").changed();
                    if changed {
                        if let Some(file) = self.file.clone() {
                            self.open(file, ctx);
                        }
                    }
                    ui.separator();
                    if ui.button("Passa  [F]").clicked() {
                        self.camera.fit(self.aspect);
                    }
                    if ui.button("Hem").clicked() {
                        self.camera.home(self.aspect);
                    }
                    if ui.button("Ovan").clicked() {
                        self.camera.rotation = glam::Quat::IDENTITY;
                        self.camera.fit(self.aspect);
                    }
                    if ui.button("Fram").clicked() {
                        self.camera.rotation =
                            glam::Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
                        self.camera.fit(self.aspect);
                    }
                    ui.checkbox(&mut self.wire, "Trådnät");
                });
            });
        egui::TopBottomPanel::bottom("status")
            .exact_height(34.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(&self.status);
                    if self.triangles > 0 {
                        ui.separator();
                        ui.label(format!(
                            "{} delar · {} trianglar",
                            self.parts, self.triangles
                        ));
                    }
                });
            });
        egui::SidePanel::right("settings")
            .default_width(210.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(20.0);
                ui.label(egui::RichText::new("NAVIGERING").strong());
                ui.add_space(12.0);
                ui.label("Vänsterdra: rotera\nHögerdra: panorera\nScroll: zooma");
                if ui.checkbox(&mut self.grid.visible,"Måttgrid · mm").changed() && self.grid.visible { self.grid.show(self.camera.rotation); }
                ui.weak("Totalmått X/Y/Z · tonas bort vid rotation");
                ui.label("Tab: dölj kropp under pekaren\nShift+Tab: återställ senast dolda");
                if !self.hidden.is_empty() {
                    ui.label(format!("{} dolda kroppar", self.hidden.len()));
                    if ui.button("Visa alla kroppar").clicked() { self.hidden.clear(); }
                }
                ui.add_space(20.0);
                ui.separator();
                ui.add_space(12.0);
                ui.label(space_status);
                ui.label("Känslighet");
                ui.add(egui::Slider::new(&mut self.speed, 0.1..=8.0));
                ui.collapsing("3D-mus: riktningar", |ui| {
                    ui.weak("Objektläge: modellen följer handen.");
                    let mut changed = false;
                    for (i, label) in ["Vänd vänster/höger", "Vänd zoom", "Vänd upp/ned", "Vänd lutning", "Vänd roll", "Vänd vridning"].iter().enumerate() {
                        changed |= ui.checkbox(&mut self.navigation.invert[i], *label).changed();
                    }
                    if ui.button("Återställ riktningar").clicked() {
                        self.navigation = spacemouse::Settings::default(); changed = true;
                    }
                    if changed { if let Err(error) = self.navigation.save() { self.status = format!("Kunde inte spara musinställningar: {error}"); } }
                });
                ui.add_space(20.0);
                ui.separator();
                ui.checkbox(&mut self.measuring,"Mät två ytor [M]");
                if self.measuring {
                    ui.label(match self.selected.len(){0=>"Klicka på den första ytan.",1=>"Klicka på den andra ytan.",_=>"Klicka på en yta för en ny mätning."});
                    ui.weak("Dra för att rotera mellan valen.");
                }
                for (i,face) in self.selected.iter().enumerate(){ui.colored_label(if i==0{egui::Color32::from_rgb(75,180,255)}else{egui::Color32::from_rgb(255,180,70)},format!("Yta {}: #{}",i+1,face+1));}
                if self.measure_rx.is_some(){ui.spinner();ui.label("Beräknar avstånd…");}
                if let Some(m)=&self.measurement {
                    let unit=self.mesh.as_ref().map(|m|m.unit.as_str()).unwrap_or("");
                    ui.heading(format!("≈ {:.3} {unit}",m.distance));
                    ui.label("Minsta avstånd mellan ytorna");
                    ui.weak("Beräknat på triangulerade ytor. Fin detalj ger bättre noggrannhet på krökta ytor.");
                }
                if !self.selected.is_empty()&&ui.button("Rensa mätning").clicked(){self.selected.clear();self.measurement=None;self.measure_rx=None;}
                ui.add_space(20.0);
                ui.weak("Native Rust / OpenGL\nFilerna stannar på datorn.");
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(8, 17, 33)))
            .show(ctx, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                self.aspect = rect.width() / rect.height().max(1.0);
                if response.dragged_by(egui::PointerButton::Primary) {
                    let d = ctx.input(|i| i.pointer.delta());
                    self.camera.orbit(d.x, d.y);
                }
                if response.dragged_by(egui::PointerButton::Secondary)
                    || response.dragged_by(egui::PointerButton::Middle)
                {
                    let d = ctx.input(|i| i.pointer.delta());
                    self.camera.pan(d.x, d.y, rect.height());
                }
                if response.hovered() {
                    self.camera.zoom(ctx.input(|i| i.smooth_scroll_delta.y));
                }
                if self.triangles > 0 {
                    let renderer = self.renderer.clone();
                    let matrix = self.camera.matrix(self.aspect);
                    if self.grid.update(self.camera.rotation, dt) {
                        ctx.request_repaint();
                    }
                    self.grid
                        .draw(&ui.painter().with_clip_rect(rect), rect, matrix, false);
                    let wire = self.wire;
                    if response.hovered() && self.job.is_none() {
                        let tab = ctx.input(|i| {
                            i.events.iter().find_map(|event| match event {
                                egui::Event::Key {
                                    key: egui::Key::Tab,
                                    pressed: true,
                                    repeat: false,
                                    modifiers,
                                    ..
                                } if !modifiers.ctrl && !modifiers.alt && !modifiers.command => {
                                    Some(*modifiers)
                                }
                                _ => None,
                            })
                        });
                        if let Some(modifiers) = tab {
                            ctx.memory_mut(|m| {
                                if let Some(id) = m.focused() {
                                    m.surrender_focus(id);
                                }
                            });
                            ctx.input_mut(|i| i.consume_key(modifiers, egui::Key::Tab));
                            if modifiers.shift {
                                self.hidden.pop();
                            } else if let (Some(mesh), Some(point)) =
                                (&self.mesh, response.hover_pos())
                            {
                                let ndc = [
                                    (point.x - rect.left()) / rect.width() * 2.0 - 1.0,
                                    1.0 - (point.y - rect.top()) / rect.height() * 2.0,
                                ];
                                if let Some(face) = measure::pick(mesh, matrix, ndc, &self.hidden) {
                                    self.hidden.push(measure::body_for_face(mesh, face));
                                    self.selected.clear();
                                    self.measurement = None;
                                    self.measure_rx = None;
                                }
                            }
                        }
                    }
                    if self.measuring && response.clicked() && self.job.is_none() {
                        if let (Some(mesh), Some(point)) =
                            (&self.mesh, response.interact_pointer_pos())
                        {
                            let ndc = [
                                (point.x - rect.left()) / rect.width() * 2.0 - 1.0,
                                1.0 - (point.y - rect.top()) / rect.height() * 2.0,
                            ];
                            if let Some(face) = measure::pick(mesh, matrix, ndc, &self.hidden) {
                                if self.selected.len() == 2 {
                                    self.selected.clear();
                                    self.measurement = None;
                                    self.measure_rx = None;
                                }
                                if !self.selected.contains(&face) {
                                    self.selected.push(face);
                                }
                                if self.selected.len() == 2 {
                                    let mesh = mesh.clone();
                                    let a = self.selected[0];
                                    let b = self.selected[1];
                                    let (tx, rx) = mpsc::channel();
                                    self.measure_rx = Some(rx);
                                    let context = ctx.clone();
                                    std::thread::spawn(move || {
                                        let _ = tx.send(measure::between(&mesh, a, b));
                                        context.request_repaint();
                                    });
                                }
                            }
                        }
                    }
                    let highlights: Vec<_> = self
                        .mesh
                        .as_ref()
                        .map(|m| self.selected.iter().map(|i| m.faces[*i]).collect())
                        .unwrap_or_default();
                    let visible = self
                        .mesh
                        .as_ref()
                        .map(|mesh| render::visible_ranges(&mesh.bodies, &self.hidden))
                        .unwrap_or_default();
                    ui.painter().add(egui::PaintCallback {
                        rect,
                        callback: Arc::new(egui_glow::CallbackFn::new(
                            move |_info, painter| unsafe {
                                renderer.lock().unwrap().paint(
                                    painter.gl(),
                                    matrix,
                                    wire,
                                    &highlights,
                                    &visible,
                                );
                            },
                        )),
                    });
                    self.grid
                        .draw(&ui.painter().with_clip_rect(rect), rect, matrix, true);
                    if let Some(measurement) = &self.measurement {
                        let project = |v: glam::DVec3| {
                            let p = matrix * v.as_vec3().extend(1.0);
                            if p.w <= 0.0 {
                                return None;
                            }
                            let p = p.truncate() / p.w;
                            Some(egui::pos2(
                                rect.left() + (p.x + 1.0) * 0.5 * rect.width(),
                                rect.top() + (1.0 - p.y) * 0.5 * rect.height(),
                            ))
                        };
                        if let (Some(a), Some(b)) = (project(measurement.a), project(measurement.b))
                        {
                            let color = egui::Color32::from_rgb(255, 220, 100);
                            let painter = ui.painter().with_clip_rect(rect);
                            painter.line_segment([a, b], egui::Stroke::new(2.0, color));
                            painter.circle_filled(a, 4.0, color);
                            painter.circle_filled(b, 4.0, color);
                            painter.text(
                                a + (b - a) * 0.5 + egui::vec2(0.0, -16.0),
                                egui::Align2::CENTER_CENTER,
                                format!(
                                    "≈ {:.3} {}",
                                    measurement.distance,
                                    self.mesh.as_ref().unwrap().unit
                                ),
                                egui::FontId::proportional(18.0),
                                color,
                            );
                        }
                    }
                } else {
                    ui.painter().text(
                        rect.center() - egui::vec2(0.0, 38.0),
                        egui::Align2::CENTER_CENTER,
                        "Från fil till form.",
                        egui::FontId::proportional(42.0),
                        egui::Color32::from_rgb(110, 211, 239),
                    );
                    ui.painter().text(
                        rect.center() + egui::vec2(0.0, 20.0),
                        egui::Align2::CENTER_CENTER,
                        "Släpp en STEP-fil här eller tryck O",
                        egui::FontId::proportional(18.0),
                        egui::Color32::GRAY,
                    );
                }
            });
        if self.job.is_some() {
            egui::Window::new("Öppnar modell")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(self.job.as_ref().unwrap().stage.lock().unwrap().clone());
                    });
                    if ui.button("Avbryt").clicked() {
                        self.job = None;
                        self.mesh = None;
                        self.triangles = 0;
                        self.parts = 0;
                        self.status = "Avbruten".into();
                    }
                });
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if let Some(error) = self.error.clone() {
            egui::Window::new("Kunde inte öppna")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(error);
                    if ui.button("Stäng").clicked() {
                        self.error = None;
                    }
                });
        }
    }
    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        self.job = None;
        if let Some(gl) = gl {
            unsafe {
                self.renderer.lock().unwrap().destroy(gl);
            }
        }
    }
}
