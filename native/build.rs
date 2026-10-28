fn main() {
    if std::env::var("TARGET").unwrap() == "x86_64-pc-windows-gnu" {
        let icon =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/stepfast-icon.obj");
        println!("cargo:rustc-link-arg={}", icon.display());
        println!("cargo:rerun-if-changed=assets/stepfast-icon.obj");
    }
    // Rust's bundled MinGW libraries omit shlwapi, used by egui's link opener.
    // Generate just its import descriptor; the implementation is Windows' DLL.
    if std::env::var("TARGET").unwrap() == "x86_64-pc-windows-gnu" {
        let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let sysroot = std::process::Command::new(std::env::var_os("RUSTC").unwrap())
            .args(["--print", "sysroot"])
            .output()
            .unwrap();
        let root = String::from_utf8(sysroot.stdout).unwrap();
        let tool = std::path::Path::new(root.trim())
            .join("lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained/dlltool.exe");
        let def = output.join("shlwapi.def");
        std::fs::write(&def, "LIBRARY shlwapi.dll\nEXPORTS\nAssocQueryStringW\n").unwrap();
        let status = std::process::Command::new(tool)
            .args(["-d"])
            .arg(def)
            .arg("-l")
            .arg(output.join("libshlwapi.a"))
            .status()
            .unwrap();
        assert!(status.success());
        println!("cargo:rustc-link-search=native={}", output.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
