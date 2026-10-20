//! Build script — makes the pinned herdr daemon available alongside zenix.
//!
//! zenix does not build the daemon. `vendor/herdr-pins.tsv` pins a released
//! herdr binary by version and SHA-256; `scripts/fetch-herdr.sh` downloads that
//! release into `res/herdr`, and this script copies the seed to
//! `target/<profile>/herdr` where the client looks for it.
//!
//! A release build refuses a seed that does not match the pin: the seed is a
//! package input, and shipping a daemon nobody can name the version of is how
//! the submodule era ended (tree pinned v0.9.3, machines ran 0.9.5).

use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let profile = env::var("PROFILE").unwrap();
    let name = if cfg!(target_os = "windows") {
        "herdr.exe"
    } else {
        "herdr"
    };

    // Destination: target/<profile>/herdr (alongside the zenix binary)
    let dest = out_dir
        .parent()
        .unwrap() // .../build/<crate>-<hash>/
        .parent()
        .unwrap() // .../build/
        .parent()
        .unwrap() // .../target/<profile>/
        .join(name);
    let seed = manifest.join("res").join(name);

    let pin = platform_pin(&manifest.join("vendor").join("herdr-pins.tsv"));
    if let Some(pin) = &pin {
        println!("cargo:rustc-env=HERDR_UPSTREAM_VERSION={}", pin.version);
    }
    println!("cargo:rerun-if-changed=res/{name}");
    println!("cargo:rerun-if-changed=vendor/herdr-pins.tsv");
    println!("cargo:rerun-if-changed=build.rs");
    // rust-i18n's macro reads `locales/` while it expands, so cargo cannot see
    // the dependency on its own: without this line an edited translation
    // rebuilds nothing and the stale strings ship.
    println!("cargo:rerun-if-changed=locales");

    if seed.is_file() {
        if let Some(pin) = &pin {
            verify(&seed, pin, profile == "release");
        }
        copy_if_stale(&seed, &dest);
        println!("cargo:rustc-env=HERDR_BINARY={}", dest.display());
        return;
    }

    // A release on a platform that has a pinned asset must ship that asset: a
    // package with no daemon cannot connect on first run. A platform with no
    // pinned row (Windows ships a zip, and its installer deliberately bundles
    // no seed) falls through to the installed binary, as it always has.
    if profile == "release"
        && let Some(pin) = &pin
    {
        panic!(
            "res/{name} is missing — run scripts/fetch-herdr.sh (pinned {})",
            pin.version
        );
    }

    // No seed: fall back to whatever daemon is installed. The client also looks
    // on PATH at runtime, so this is a convenience, not a contract.
    match find_system_herdr(name) {
        Some(system) => {
            copy_if_stale(&system, &dest);
            eprintln!("copied system herdr from {}", system.display());
        }
        None => eprintln!(
            "NOTE: res/{name} is missing and no herdr is installed.\n\
             Run scripts/fetch-herdr.sh for the pinned daemon.\n\
             zenix will fall back to PATH lookup at runtime."
        ),
    }
    println!("cargo:rustc-env=HERDR_BINARY={}", dest.display());
}

struct Pin {
    version: String,
    sha256: String,
}

/// The row of `vendor/herdr-pins.tsv` that matches this build host.
///
/// `None` on a platform whose release asset is not a runnable binary (Windows
/// ships a zip) — such a build gets no version claim rather than a wrong one.
fn platform_pin(pins: &std::path::Path) -> Option<Pin> {
    let asset = asset_name()?;
    let table = fs::read_to_string(pins).ok()?;
    for line in table.lines() {
        let mut fields = line.split('\t');
        let (Some(version), Some(row_asset), Some(sha256)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if row_asset == asset {
            return Some(Pin {
                version: version.to_owned(),
                sha256: sha256.to_owned(),
            });
        }
    }
    None
}

fn asset_name() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "herdr-linux-x86_64",
        ("linux", "aarch64") => "herdr-linux-aarch64",
        ("macos", "x86_64") => "herdr-macos-x86_64",
        ("macos", "aarch64") => "herdr-macos-aarch64",
        _ => return None,
    })
}

/// Compare the seed with the pin. A release build must match; a dev build only
/// warns, because working against a newer daemon is a normal thing to do.
fn verify(seed: &std::path::Path, pin: &Pin, required: bool) {
    match sha256_of(seed) {
        Some(actual) if actual == pin.sha256 => {}
        Some(actual) => {
            let message = format!(
                "res/{} is {actual}, but vendor/herdr-pins.tsv pins {} ({})",
                file_name(seed),
                pin.version,
                pin.sha256
            );
            if required {
                panic!("{message} — run scripts/fetch-herdr.sh");
            }
            println!("cargo:warning={message}");
        }
        None => println!(
            "cargo:warning=res/{} not verified: no sha256 tool on this machine",
            file_name(seed)
        ),
    }
}

fn file_name(path: &std::path::Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("herdr")
}

/// The platform's own checksum tool rather than a hashing dependency: this runs
/// once per build, and a missing tool degrades to "not verified".
fn sha256_of(path: &std::path::Path) -> Option<String> {
    let (program, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("shasum", &["-a", "256"])
    } else {
        ("sha256sum", &[])
    };
    let output = Command::new(program).args(args).arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace().next().map(str::to_owned)
}

fn copy_if_stale(source: &std::path::Path, dest: &std::path::Path) {
    if let (Ok(from), Ok(to)) = (fs::metadata(source), fs::metadata(dest)) {
        let same_size = from.len() == to.len();
        let up_to_date = match (from.modified(), to.modified()) {
            (Ok(from), Ok(to)) => to >= from,
            _ => false,
        };
        if same_size && up_to_date {
            return;
        }
    }
    let _ = fs::create_dir_all(dest.parent().unwrap());
    fs::copy(source, dest).unwrap_or_else(|error| panic!("copying {}: {error}", source.display()));
    eprintln!("herdr seed → {}", dest.display());
}

fn find_system_herdr(name: &str) -> Option<PathBuf> {
    let home = home_dir();
    #[cfg(unix)]
    let candidates = [
        home.join(".local/bin").join(name),
        home.join(".cargo/bin").join(name),
    ];
    #[cfg(not(unix))]
    let candidates = [home.join(".cargo/bin").join(name)];
    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
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
