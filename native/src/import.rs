use bincode::Options;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use truck_assembly::assy::*;
use truck_meshalgo::prelude::*;
use truck_stepio::r#in::ruststep::{ast::Name, tables::PlaceHolder};
use truck_stepio::r#in::{convert::*, Table};

// SolidWorks/AP214 separates product placements from B-reps using ordinary
// SHAPE_REPRESENTATION_RELATIONSHIP entities. Truck follows assembly transforms,
// but not these same-coordinate-space geometry links. Resolve only ordinary
// links here; transformed relationships remain exclusively with the assembly.
fn resolve_geometry_links(table: &mut Table) {
    use std::collections::{HashMap, HashSet};
    let mut adjacency: HashMap<u64, Vec<u64>> = HashMap::new();
    for relation in table.shape_representation_relationship.values() {
        if let (PlaceHolder::Ref(Name::Entity(a)), PlaceHolder::Ref(Name::Entity(b))) =
            (&relation.rep_1, &relation.rep_2)
        {
            adjacency.entry(*a).or_default().push(*b);
            adjacency.entry(*b).or_default().push(*a);
        }
    }
    let mut visited = HashSet::new();
    for &start in adjacency.keys() {
        if visited.contains(&start) {
            continue;
        }
        let mut pending = vec![start];
        let mut component = Vec::new();
        let mut geometry = Vec::new();
        let mut unique = HashSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            component.push(id);
            if let Some(rep) = table.shape_representation.get(&id) {
                for item in &rep.items {
                    if let PlaceHolder::Ref(Name::Entity(item_id)) = item {
                        if (table.manifold_solid_brep.contains_key(item_id)
                            || table.shell_based_surface_model.contains_key(item_id))
                            && unique.insert(*item_id)
                        {
                            geometry.push(item.clone());
                        }
                    }
                }
            }
            if let Some(neighbors) = adjacency.get(&id) {
                pending.extend(neighbors);
            }
        }
        for id in component {
            if let Some(rep) = table.shape_representation.get_mut(&id) {
                for item in &geometry {
                    if !rep.items.contains(item) {
                        rep.items.push(item.clone());
                    }
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Mesh {
    pub vertices: Vec<f32>, // xyz + normal, triangles in one GPU buffer
    pub radius: f32,
    pub parts: usize,
    pub cached: bool,
    pub faces: Vec<[u32; 2]>, // first vertex, vertex count for each B-rep face
    pub unit: String,
    pub backend: String,
    pub bodies: Vec<[u32; 2]>, // contiguous vertex ranges, one per placed body
}

pub fn decode(bytes: &[u8]) -> Result<Mesh, String> {
    let mesh: Mesh = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(512 * 1024 * 1024)
        .deserialize(bytes)
        .map_err(|e| e.to_string())?;
    if mesh.vertices.is_empty()
        || mesh.vertices.len() % 18 != 0
        || !mesh.radius.is_finite()
        || mesh.radius <= 0.0
        || mesh.vertices.iter().any(|x| !x.is_finite())
        || mesh.faces.is_empty()
        || mesh
            .faces
            .iter()
            .any(|f| f[1] % 3 != 0 || f[0] as usize + f[1] as usize > mesh.vertices.len() / 6)
    {
        return Err("Ogiltig geometri i cache".into());
    }
    let mut end = 0u64;
    for range in &mesh.bodies {
        if range[0] as u64 != end || range[1] % 3 != 0 {
            return Err("Ogiltiga kroppar i cache".into());
        }
        end += range[1] as u64;
    }
    if mesh.bodies.len() != mesh.parts || end != (mesh.vertices.len() / 6) as u64 {
        return Err("Ofullständig kroppgeometri i cache".into());
    }
    Ok(mesh)
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("StepFast")
        .join("mesh-v6")
}

pub fn load(path: &Path, fine: bool) -> Result<Mesh, String> {
    crate::progress::stage("Läser STEP och kontrollerar cache…");
    let bytes = fs::read(path).map_err(|e| format!("Kunde inte läsa filen: {e}"))?;
    let mut hash = blake3::Hasher::new();
    hash.update(b"step-fast-native-v6");
    hash.update(&[fine as u8]);
    hash.update(&bytes);
    let cache = cache_dir().join(format!("{}.mesh", hash.finalize().to_hex()));
    if let Ok(data) = fs::read(&cache) {
        if let Ok(mut mesh) = decode(&data) {
            mesh.cached = true;
            return Ok(mesh);
        }
    }
    let attempt = std::panic::catch_unwind(|| load_truck(bytes, fine));
    let mesh = match attempt {
        Ok(Ok(mesh)) => mesh,
        _ => load_occt(path, fine)?,
    };
    if fs::create_dir_all(cache_dir()).is_ok() {
        if let Ok(data) = bincode::serialize(&mesh) {
            if data.len() < 256 * 1024 * 1024 {
                let tmp = cache.with_extension(format!("{}.tmp", std::process::id()));
                if fs::write(&tmp, data).is_ok() {
                    let _ = fs::rename(&tmp, &cache);
                    let _ = fs::remove_file(tmp);
                }
            }
        }
        prune_cache();
    }
    Ok(mesh)
}

fn load_occt(path: &Path, fine: bool) -> Result<Mesh, String> {
    crate::progress::stage("OpenCascade · läser solider och ytor…");
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let runtime = [
        exe_dir.join("native/occt-runtime"),
        exe_dir.join("occt-runtime"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("occt-runtime"),
    ]
    .into_iter()
    .find(|p| p.join("python/python.exe").exists())
    .ok_or("OpenCascade-reservmotorn saknas. Kör native/setup-occt.ps1.")?;
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let output = std::env::temp_dir().join(format!(
        "stepfast-occt-{}-{}.mesh",
        std::process::id(),
        SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut command = std::process::Command::new(runtime.join("python/python.exe"));
    command
        .arg(runtime.join("import_step.py"))
        .arg(path)
        .arg(&output)
        .arg(if fine { "fine" } else { "fast" });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let result = command
        .output()
        .map_err(|e| format!("OpenCascade-reservmotorn kunde inte starta: {e}"))?;
    if !result.status.success() {
        let _ = fs::remove_file(&output);
        return Err(format!(
            "OpenCascade kunde inte läsa modellen: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    let mesh = fs::read(&output)
        .map_err(|e| e.to_string())
        .and_then(|bytes| decode(&bytes));
    let _ = fs::remove_file(output);
    mesh
}

fn load_truck(bytes: Vec<u8>, fine: bool) -> Result<Mesh, String> {
    let text =
        String::from_utf8(bytes).map_err(|_| "STEP-filen har en textkodning som inte stöds")?;
    if !text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .contains("SI_UNIT(.MILLI.,.METRE.)")
    {
        return Err("Använder OpenCascade för konvertering till millimeter".into());
    }
    crate::progress::stage("Rust · läser ytor och assembly-placeringar…");
    let mut table = Table::from_step(&text).ok_or("Ogiltig eller ofullständig STEP-fil")?;
    resolve_geometry_links(&mut table);
    let assembly = table
        .step_assy()
        .map_err(|e| format!("STEP-strukturen stöds inte: {e}"))?;
    let node_map = |ProductEntity { shape, .. }: &ProductEntity| {
        let mut meshes = Vec::new();
        for shape in shape {
            let shells = match shape {
                ProductShape::Solid(s) => &s.boundaries,
                ProductShape::Shells(s) => s,
                _ => continue,
            };
            for shell in shells {
                let bounds: BoundingBox<Point3> = shell.vertices.iter().copied().collect();
                let mut tolerance =
                    (bounds.diameter() * if fine { 0.0005 } else { 0.003 }).max(1.0e-5);
                for face in &shell.faces {
                    if let truck_stepio::r#in::step_geometry::Surface::ElementarySurface(
                        truck_stepio::r#in::step_geometry::ElementarySurface::Sphere(sphere),
                    ) = &face.surface
                    {
                        tolerance = tolerance.min(sphere.entity().0.radius() * 0.1);
                    }
                }
                let mut triangles = shell.robust_triangulation(tolerance);
                for _ in 0..1 {
                    if triangles.faces.iter().all(|face| face.surface.is_some()) {
                        break;
                    }
                    tolerance *= 0.1;
                    triangles = shell.robust_triangulation(tolerance.max(1.0e-4));
                }
                if triangles.faces.iter().any(|face| face.surface.is_none()) {
                    return NodeEntity {
                        shape: Err(format!(
                            "{} av {} ytor kunde inte trianguleras efter finare omförsök",
                            triangles
                                .faces
                                .iter()
                                .filter(|f| f.surface.is_none())
                                .count(),
                            triangles.faces.len()
                        )),
                        attrs: (),
                    };
                }
                meshes.push(
                    triangles
                        .faces
                        .into_iter()
                        .map(|f| {
                            let m = f.surface.unwrap();
                            if f.orientation {
                                m
                            } else {
                                m.inverse()
                            }
                        })
                        .collect::<Vec<_>>(),
                );
            }
        }
        NodeEntity {
            shape: Ok(meshes),
            attrs: (),
        }
    };
    let edge_map = |EdgeEntity { matrix, .. }: &AssembleEntity| EdgeEntity {
        matrix: Matrix4::try_from(matrix).expect("Ogiltig assembly-transform"),
        attrs: (),
    };
    // Independent component tessellation is distributed across the CPU pool.
    crate::progress::stage("Rust · triangulerar ytor parallellt…");
    let meshed = assembly.par_map(node_map, edge_map);
    let mut positions = Vec::<[f64; 3]>::new();
    let mut parts = 0;
    let mut bodies = Vec::new();
    let mut faces = Vec::new();
    for root in meshed.top_nodes() {
        for path in meshed.paths_iter(root.index()) {
            let transform = path.matrix();
            let node = path.terminal_node();
            for shell in node.shape().as_ref().map_err(Clone::clone)? {
                let body_start = positions.len() as u32;
                parts += 1;
                for mesh in shell {
                    let start = positions.len() as u32;
                    for triangle in mesh.faces().triangle_iter() {
                        for v in triangle {
                            let p = transform.transform_point(mesh.positions()[v.pos]);
                            positions.push([p.x, p.y, p.z]);
                        }
                    }
                    faces.push([start, positions.len() as u32 - start]);
                }
                bodies.push([body_start, positions.len() as u32 - body_start]);
            }
        }
    }
    // Some single-part exporters link the B-rep through a representation
    // relationship not followed by Truck's assembly reader. Read its shells
    // directly only when there are no assembly placements to lose.
    if positions.is_empty()
        && !text.contains("NEXT_ASSEMBLY_USAGE_OCCURRENCE")
        && !text.contains("ITEM_DEFINED_TRANSFORMATION")
    {
        for shell in table.shell.values() {
            let body_start = positions.len() as u32;
            let shell = table
                .to_compressed_shell(shell)
                .map_err(|e| format!("STEP-yta stöds inte: {e}"))?;
            let bounds: BoundingBox<Point3> = shell.vertices.iter().copied().collect();
            let triangles = shell.robust_triangulation(
                (bounds.diameter() * if fine { 0.0005 } else { 0.003 }).max(1.0e-5),
            );
            if triangles.faces.iter().any(|face| face.surface.is_none()) {
                return Err("En STEP-yta kunde inte trianguleras".into());
            }
            parts += 1;
            for face in triangles.faces {
                let mesh = face.surface.unwrap();
                let mesh = if face.orientation {
                    mesh
                } else {
                    mesh.inverse()
                };
                let start = positions.len() as u32;
                for triangle in mesh.faces().triangle_iter() {
                    for v in triangle {
                        let p = mesh.positions()[v.pos];
                        positions.push([p.x, p.y, p.z]);
                    }
                }
                faces.push([start, positions.len() as u32 - start]);
            }
            bodies.push([body_start, positions.len() as u32 - body_start]);
        }
    }
    if positions.is_empty() {
        return Err("Filen innehåller ingen solid eller ytmodell som stöds".into());
    }
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in &positions {
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    let center = std::array::from_fn::<_, 3, _>(|i| (min[i] + max[i]) * 0.5);
    let radius = ((0..3).map(|i| (max[i] - min[i]).powi(2)).sum::<f64>()).sqrt() * 0.5;
    if !radius.is_finite() || radius <= 0.0 {
        return Err("Modellen har ogiltiga mått".into());
    }
    let mut vertices = Vec::with_capacity(positions.len() * 6);
    crate::progress::stage("Rust · bygger visningsgeometri…");
    let mut last_preview = std::time::Instant::now();
    let mut next_face = 0;
    for tri in positions.chunks_exact(3) {
        let p: Vec<glam::Vec3> = tri
            .iter()
            .map(|p| {
                glam::Vec3::new(
                    (p[0] - center[0]) as f32,
                    (p[1] - center[1]) as f32,
                    (p[2] - center[2]) as f32,
                )
            })
            .collect();
        let normal = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        for v in p {
            vertices.extend(v.to_array());
            vertices.extend(normal.to_array());
        }
        let count = (vertices.len() / 6) as u32;
        while next_face < faces.len() && faces[next_face][0] + faces[next_face][1] <= count {
            next_face += 1;
        }
        if crate::progress::enabled()
            && next_face > 0
            && faces[next_face - 1][0] + faces[next_face - 1][1] == count
            && last_preview.elapsed().as_millis() >= 120
        {
            let partial_bodies: Vec<_> = bodies
                .iter()
                .filter(|b| b[0] < count)
                .map(|b| [b[0], b[1].min(count - b[0])])
                .collect();
            crate::progress::mesh(&Mesh {
                vertices: vertices.clone(),
                radius: radius as f32,
                parts: partial_bodies.len(),
                cached: false,
                faces: faces[..next_face].to_vec(),
                unit: "mm".into(),
                backend: "Rust".into(),
                bodies: partial_bodies,
            });
            last_preview = std::time::Instant::now();
        }
    }
    let mesh = Mesh {
        vertices,
        radius: radius as f32,
        parts,
        cached: false,
        faces,
        unit: if text
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .contains("SI_UNIT(.MILLI.,.METRE.)")
        {
            "mm".into()
        } else {
            "filenheter".into()
        },
        backend: "Rust".into(),
        bodies,
    };
    Ok(mesh)
}

fn prune_cache() {
    if let Ok(entries) = fs::read_dir(cache_dir()) {
        let mut entries: Vec<_> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|s| s == "mesh"))
            .filter_map(|e| {
                Some((
                    e.metadata().ok()?.modified().ok()?,
                    e.path(),
                    e.metadata().ok()?.len(),
                ))
            })
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.0));
        let mut total = 0;
        for (i, (_, path, size)) in entries.iter().enumerate() {
            total += size;
            if i >= 8 || total > 512 * 1024 * 1024 {
                let _ = fs::remove_file(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires STEP_FAST_TEST_FILE pointing to a local private file"]
    fn external_step_file() {
        let path = PathBuf::from(
            std::env::var_os("STEP_FAST_TEST_FILE").expect("Set STEP_FAST_TEST_FILE"),
        );
        let start = std::time::Instant::now();
        let mesh = load(&path, false).unwrap();
        assert!(!mesh.vertices.is_empty());
        println!(
            "Imported {} parts, {} triangles in {:.3}s, cache={}",
            mesh.parts,
            mesh.vertices.len() / 18,
            start.elapsed().as_secs_f64(),
            mesh.cached
        );
        assert!(mesh.vertices.iter().all(|v| v.is_finite()));
    }
    #[test]
    fn linked_assembly_preserves_instances_and_transforms() {
        use truck_stepio::r#in::ShapeRepresentationRelationshipHolder;
        fn signature(table: &Table) -> Vec<String> {
            let raw = table.step_assy().unwrap();
            let assembly = raw.par_map(
                |n| n.clone(),
                |e| EdgeEntity {
                    matrix: Matrix4::try_from(&e.matrix).unwrap(),
                    attrs: e.attrs.clone(),
                },
            );
            let mut rows = Vec::new();
            for root in assembly.top_nodes() {
                for path in assembly.paths_iter(root.index()) {
                    let node = path.terminal_node();
                    let count = node
                        .shape()
                        .iter()
                        .filter(|s| !matches!(s, ProductShape::Matrix(_)))
                        .count();
                    if count > 0 {
                        rows.push(format!("{:?}:{count}:{:?}", node.attrs(), path.matrix()));
                    }
                }
            }
            rows.sort();
            rows
        }
        let text = include_str!("../../tests/fixtures/assembly.step");
        let mut table = Table::from_step(text).unwrap();
        let expected = signature(&table);
        assert!(!expected.is_empty());
        let reps: Vec<_> = table.shape_representation.keys().copied().collect();
        let mut id = 1_000_000;
        for rep_id in reps {
            let mut linked = table.shape_representation[&rep_id].clone();
            linked.items.retain(|item| matches!(item, PlaceHolder::Ref(Name::Entity(n)) if table.manifold_solid_brep.contains_key(n)||table.shell_based_surface_model.contains_key(n)));
            if linked.items.is_empty() {
                continue;
            }
            table
                .shape_representation
                .get_mut(&rep_id)
                .unwrap()
                .items
                .retain(|item| !linked.items.contains(item));
            table.shape_representation.insert(id, linked);
            table.shape_representation_relationship.insert(
                id + 1,
                ShapeRepresentationRelationshipHolder {
                    name: String::new(),
                    description: String::new(),
                    rep_1: PlaceHolder::Ref(Name::Entity(rep_id)),
                    rep_2: PlaceHolder::Ref(Name::Entity(id)),
                },
            );
            id += 2;
        }
        assert!(signature(&table).is_empty());
        resolve_geometry_links(&mut table);
        assert_eq!(signature(&table), expected);
        resolve_geometry_links(&mut table);
        assert_eq!(
            signature(&table),
            expected,
            "Resolution must not duplicate geometry"
        );
    }
    #[test]
    fn cube_import_and_cache() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/cube.stp");
        let mesh = load(&path, false).unwrap();
        assert_eq!(mesh.vertices.len() / 18, 12);
        assert!(mesh.radius > 0.0);
        let cached = load(&path, false).unwrap();
        assert!(cached.cached);
        assert_eq!(mesh.vertices, cached.vertices);
    }
    #[test]
    fn corrupt_cache_rejected() {
        assert!(decode(b"bad cache").is_err());
    }
    #[test]
    fn curved_surface_and_assembly() {
        for name in ["cylinder.step", "assembly.step"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures")
                .join(name);
            let mesh = load(&path, false).unwrap();
            let decoded = decode(&bincode::serialize(&mesh).unwrap()).unwrap();
            assert_eq!(decoded.bodies.len(), mesh.parts);
            let mut broken = decoded;
            broken.bodies[0][0] = 3;
            assert!(decode(&bincode::serialize(&broken).unwrap()).is_err());
            assert!(mesh.vertices.len() / 18 > 12, "{name}");
            if name == "assembly.step" {
                assert!(mesh.parts > 1);
            }
        }
    }
}

