//! Build script — makes herdr binary available alongside zenix.
//!
//! Dev profile:   copy system-installed herdr (fast).
//! Release profile: build herdr from submodule (guaranteed version match).

use std::{env, fs, path::PathBuf};

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    let herdr_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("herdr");
    let profile = env::var("PROFILE").unwrap();

    let name = if cfg!(target_os = "windows") {
        "herdr.exe"
    } else {
        "herdr"
    };

    // Destination: target/<profile>/herdr (alongside zenix binary)
    let dest = out_dir
        .parent()
        .unwrap() // .../build/<crate>-<hash>/
        .parent()
        .unwrap() // .../build/
        .parent()
        .unwrap() // .../target/<profile>/
        .join(name);

    // Forced submodule build: wins over the cached copy and the installed-herdr
    // shortcut, so `HERDR_BUILD=1` really does build herdr from the submodule.
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    if env::var("HERDR_BUILD").is_ok() {
        build_herdr(&herdr_dir, name, &dest);
        seed_package(&dest, name, &profile);
        emit(&dest);
        return;
    }

    // Cached from prior build
    if dest.exists() {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
        seed_package(&dest, name, &profile);
        emit(&dest);
        return;
    }

    if profile == "release" {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
        {
            build_herdr(&herdr_dir, name, &dest);
            seed_package(&dest, name, &profile);
        }
        #[cfg(windows)]
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
        // Dev: copy the installed herdr for speed; HERDR_BUILD=1 forced a
        // submodule build above.
        if let Some(sys) = find_system_herdr() {
            let _ = fs::create_dir_all(dest.parent().unwrap());
            fs::copy(&sys, &dest).expect("failed to copy system herdr");
            eprintln!("copied system herdr from {}", sys.display());
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

/// Refresh `res/<name>` — the herdr copy the Linux/macOS packages ship as their
/// seed. A release build always rewrites it: the seed is a package input, and a
/// stale one would install an old daemon on a fresh machine.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
fn seed_package(dest: &std::path::Path, name: &str, profile: &str) {
    if profile != "release" {
        return;
    }
    let seed = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("res")
        .join(name);
    let _ = fs::create_dir_all(seed.parent().unwrap());
    fs::copy(dest, &seed).expect("failed to copy herdr seed to res/");
    eprintln!("herdr seed → {}", seed.display());
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
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
                panic!(
                    "herdr build succeeded but binary missing at {}",
                    src.display()
                );
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
    // rust-i18n's macro reads `locales/` while it expands, so cargo cannot see
    // the dependency on its own: without this line an edited translation
    // rebuilds nothing and the stale strings ship.
    println!("cargo:rerun-if-changed=locales");
}

fn find_system_herdr() -> Option<PathBuf> {
    let home = home_dir();
    let name = if cfg!(target_os = "windows") {
        "herdr.exe"
    } else {
        "herdr"
    };

    #[cfg(unix)]
    let candidates: Vec<PathBuf> = vec![
        home.join(".local/bin").join(name),
        home.join(".cargo/bin").join(name),
    ];

    #[cfg(windows)]
    let candidates: Vec<PathBuf> = vec![home.join(".cargo/bin").join(name)];

    for c in candidates {
        if c.is_file() {
            return Some(c);
        }
    }
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            let c = dir.join(name);
            if c.is_file() {
                return Some(c);
            }
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
    #[cfg(windows)]
    {
        PathBuf::from("C:\\")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/")
    }
}
