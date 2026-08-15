// Native process entry only; the library crate owns plugins, commands, and startup.
// Keep this attribute so a release build does not open an extra Windows console.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    app_lib::run();
}
