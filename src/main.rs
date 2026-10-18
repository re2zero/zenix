#![windows_subsystem = "windows"]

/// Thin binary entry point. The module tree and the startup path live in the
/// library, so the tree is compiled once (the lib target) instead of twice.
fn main() {
    zenix::run();
}
