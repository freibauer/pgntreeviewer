// Desktop entry point (Windows / Linux).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    pgntreeviewer::run(pgntreeviewer::native_options())
}
