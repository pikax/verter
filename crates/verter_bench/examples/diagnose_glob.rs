//! Minimal glob-only test. Runs `glob::glob(root/**/tsconfig.json)` and
//! prints progress. Verifies whether the bootstrap hang is caused by
//! `glob` walking PNPM's recursive node_modules symlink farm.

use std::time::Instant;

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "d:/dev/personal/verter/.integration-tests/repos/nuxt-ui".into());
    let pattern = format!("{root}/**/tsconfig.json");
    eprintln!("glob pattern: {pattern}");
    let started = Instant::now();
    let mut total = 0usize;
    let mut nm = 0usize;
    let mut last_print = Instant::now();
    match glob::glob(&pattern) {
        Ok(paths) => {
            for entry in paths {
                total += 1;
                if let Ok(p) = entry {
                    if p.to_string_lossy().contains("node_modules") {
                        nm += 1;
                    }
                }
                if last_print.elapsed().as_secs() >= 2 {
                    eprintln!(
                        "[{:>6.1}s] visited {} (in node_modules: {})",
                        started.elapsed().as_secs_f64(),
                        total,
                        nm
                    );
                    last_print = Instant::now();
                }
                if total >= 50_000 {
                    eprintln!(
                        "[{:>6.1}s] reached 50_000 — aborting",
                        started.elapsed().as_secs_f64()
                    );
                    break;
                }
            }
        }
        Err(e) => eprintln!("glob err: {e}"),
    }
    eprintln!(
        "DONE total={} nm={} elapsed={:?}",
        total,
        nm,
        started.elapsed()
    );
}
