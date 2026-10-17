//! SidAcid: a SID-powered acid groovebox. Run without arguments for the
//! app; `sidacid render-demo [out.wav]` renders the demo groove offline.

mod audio;
mod audio_scan;
mod audio_select;
mod document;
mod settings;
mod sysmem;
#[cfg(test)]
mod test_rack;
mod ui;

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        None => run_app(),
        Some("render-demo") => render_demo(args.get(2).map(String::as_str).unwrap_or("demo.wav")),
        Some(_) => {
            eprintln!(
                "usage: sidacid            (open the app)\n       sidacid render-demo [out.wav]"
            );
            ExitCode::from(2)
        }
    }
}

fn run_app() -> ExitCode {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("SidAcid")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    match eframe::run_native(
        "SidAcid",
        options,
        Box::new(|cc| Ok(Box::new(ui::SidAcidApp::new(cc)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("could not open the SidAcid window: {e}");
            ExitCode::FAILURE
        }
    }
}

fn render_demo(path: &str) -> ExitCode {
    let project = acid_engine::demo::demo_project();
    match acid_engine::render::render_patterns_wav(&project, 8, Path::new(path)) {
        Ok(()) => {
            println!("wrote {path}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("render failed: {e}");
            ExitCode::FAILURE
        }
    }
}
