//! Build script — makes herdr binary available alongside zenix.
//!
//! Dev profile:   copy system-installed herdr (fast).
//! Release profile: build herdr from submodule (guaranteed version match).

use std::{
    env, fs,
    path::PathBuf,
    process::Command,
};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let herdr_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("herdr");
    let profile = env::var("PROFILE").unwrap();

    let name = if cfg!(target_os = "windows") { "herdr.exe" } else { "herdr" };

    // Destination: target/<profile>/herdr (alongside zenix binary)
    let dest = out_dir
        .parent().unwrap()      // .../build/<crate>-<hash>/
        .parent().unwrap()      // .../build/
        .parent().unwrap()      // .../target/<profile>/
        .join(name);

    // Cached from prior build
    if dest.exists() {
        emit(&dest);
        return;
    }

    if profile == "release" {
        #[cfg(target_os = "linux")]
        {
            build_herdr(&herdr_dir, name, &dest);
            let seed = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("res").join(name);
            let _ = fs::create_dir_all(seed.parent().unwrap());
            fs::copy(&dest, &seed).expect("failed to copy herdr seed to res/");
            eprintln!("herdr seed → {}", seed.display());
        }
        #[cfg(not(target_os = "linux"))]
        {
            if let Some(sys) = find_system_herdr() {
                let _ = fs::create_dir_all(dest.parent().unwrap());
                fs::copy(&sys, &dest).expect("failed to copy system herdr");
                eprintln!("copied system herdr from {}", sys.display());
            } else {
                eprintln!("NOTE: herdr not found. Install it to PATH for full functionality.");
            }
        }
    } else {
        // Dev: copy system herdr for speed; fall back to submodule build
        if let Some(sys) = find_system_herdr() {
            let _ = fs::create_dir_all(dest.parent().unwrap());
            fs::copy(&sys, &dest).expect("failed to copy system herdr");
            eprintln!("copied system herdr from {}", sys.display());
        } else if env::var("HERDR_BUILD").is_ok() {
            #[cfg(target_os = "linux")]
            {
                build_herdr(&herdr_dir, name, &dest);
            }
            #[cfg(not(target_os = "linux"))]
            {
                eprintln!(
                    "NOTE: HERDR_BUILD is Linux-only. Install herdr to PATH on this platform."
                );
            }
        } else {
            eprintln!(
                "NOTE: herdr binary not found.\n\
                 Install herdr, or run with HERDR_BUILD=1 to build from submodule.\n\
                 zenix will fall back to PATH lookup at runtime."
            );
        }
    }

    emit(&dest);
}

fn build_herdr(herdr_dir: &std::path::Path, name: &str, dest: &std::path::Path) {
    eprintln!("building herdr from submodule...");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(herdr_dir)
        .env("CARGO_TARGET_DIR", herdr_dir.join("target"))
        .status();

    match status {
        Ok(s) if s.success() => {
            let src = herdr_dir.join("target/release").join(name);
            if src.exists() {
                let _ = fs::create_dir_all(dest.parent().unwrap());
                fs::copy(&src, dest).expect("failed to copy herdr binary");
                eprintln!("herdr built → {}", dest.display());
            } else {
                panic!("herdr build succeeded but binary missing at {}", src.display());
            }
        }
        Ok(s) => panic!("herdr build failed: {}", s),
        Err(e) => panic!("cargo build for herdr failed: {}", e),
    }
}

fn emit(dest: &std::path::Path) {
    println!("cargo:rustc-env=HERDR_BINARY={}", dest.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=herdr/Cargo.lock");
}

fn find_system_herdr() -> Option<PathBuf> {
    let home = home_dir();
    let name = if cfg!(target_os = "windows") { "herdr.exe" } else { "herdr" };
    let candidates = [
        home.join(".local/bin").join(name),
        home.join(".cargo/bin").join(name),
    ];
    for c in candidates {
        if c.is_file() { return Some(c); }
    }
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            let c = dir.join(name);
            if c.is_file() { return Some(c); }
        }
    }
    None
}

fn home_dir() -> PathBuf {
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home);
    }
    if let Ok(profile) = env::var("USERPROFILE") {
        return PathBuf::from(profile);
    }
    PathBuf::from("/")
}
