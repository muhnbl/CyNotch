use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_rc_exe() -> Option<PathBuf> {
    if let Ok(output) = Command::new("where").arg("rc.exe").output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(first) = stdout.lines().next() {
            let p = PathBuf::from(first.trim());
            if p.exists() {
                return Some(p);
            }
        }
    }

    let kits_root = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if let Ok(entries) = std::fs::read_dir(kits_root) {
        let mut versions: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path().join(r"x64\rc.exe")))
            .filter(|p| p.exists())
            .collect();
        versions.sort();
        if let Some(latest) = versions.pop() {
            return Some(latest);
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=assets/cynotch.ico");
    println!("cargo:rerun-if-changed=build.rs");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let ico_path = manifest_dir.join("assets").join("cynotch.ico");
    if !ico_path.exists() {
        println!("cargo:warning=assets/cynotch.ico missing; skipping PE icon resource.");
        return;
    }

    let Some(rc_exe) = find_rc_exe() else {
        println!("cargo:warning=Windows SDK rc.exe not found; skipping PE icon resource.");
        return;
    };

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let rc_file = out_dir.join("cynotch_icon.rc");
    let res_file = out_dir.join("cynotch_icon.res");

    let ico_escaped = ico_path.to_string_lossy().replace('\\', "/");
    let rc_content = format!("1 ICON \"{}\"\n", ico_escaped);
    std::fs::write(&rc_file, rc_content).expect("Failed to write .rc file");

    let status = Command::new(&rc_exe)
        .args([
            "/nologo",
            "/fo",
            &res_file.to_string_lossy(),
            &rc_file.to_string_lossy(),
        ])
        .status()
        .expect("Failed to invoke rc.exe");

    if !status.success() {
        panic!("rc.exe failed to compile cynotch_icon.rc");
    }

    println!("cargo:rustc-link-arg-bin=cynotch={}", res_file.display());
}
