use std::path::{Path, PathBuf};

pub const TARGET_RATE: u32 = 48000;

const AUDIO_EXTENSIONS: [&str; 6] = ["mp3", "flac", "wav", "aac", "m4a", "ogg"];

pub fn scan_folder(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error + Send + Sync>> {
    fn visit(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit(&path, out)?;
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
            {
                out.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    let started = std::time::Instant::now();
    match visit(dir, &mut files) {
        Ok(()) => {}
        Err(e) => {
            log::error!("failed to scan {}: {e}", dir.display());
            return Err(Box::new(e));
        }
    }
    files.sort();
    log::info!(
        "scanned {}: {} audio files in {:.1}s",
        dir.display(),
        files.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(files)
}
