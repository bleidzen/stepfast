use crate::import::Mesh;
use std::{fs, path::PathBuf};
fn path() -> Option<PathBuf> {
    std::env::var_os("STEP_FAST_PROGRESS").map(PathBuf::from)
}
pub fn stage(text: &str) {
    if let Some(p) = path() {
        let _ = fs::write(p.with_extension("status"), text);
    }
}
pub fn enabled() -> bool {
    path().is_some()
}
pub fn mesh(mesh: &Mesh) {
    if let Some(p) = path() {
        if let Ok(bytes) = bincode::serialize(mesh) {
            let tmp = p.with_extension("partial");
            if fs::write(&tmp, bytes).is_ok() {
                let _ = fs::remove_file(&p);
                let _ = fs::rename(tmp, p);
            }
        }
    }
}
