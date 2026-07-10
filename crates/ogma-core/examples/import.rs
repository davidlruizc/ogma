//! Dev smoke test for the audio-file import path:
//! `cargo run -p ogma-core --example import -- <audio-file> <out-dir>`

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(src), Some(dir)) = (args.next(), args.next()) else {
        eprintln!("usage: import <audio-file> <out-dir>");
        std::process::exit(2);
    };
    match ogma_core::recording::import::import_file(src.as_ref(), dir.as_ref()) {
        Ok(r) => {
            println!("duration_ms: {}", r.duration_ms);
            for seg in &r.segments {
                let samples = ogma_core::recording::wav::sample_count(seg).unwrap_or(0);
                println!("  {} ({samples} samples)", seg.display());
            }
        }
        Err(e) => {
            eprintln!("import failed: {e}");
            std::process::exit(1);
        }
    }
}
