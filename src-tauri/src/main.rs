// Keep the development console available, but prevent installed release builds
// from opening an extra terminal window when launched from an MSI/NSIS shortcut.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ytharbor_lib::run();
}
