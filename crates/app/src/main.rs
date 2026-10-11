//! SidAcid. Plan 1 ships only the offline demo renderer; the GUI arrives
//! in Plan 2.

mod audio;
mod document;

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("render-demo") => {
            let path = args.get(2).map(String::as_str).unwrap_or("demo.wav");
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
        _ => {
            eprintln!("usage: sidacid render-demo [out.wav]");
            ExitCode::from(2)
        }
    }
}
