//! Embed the compiling git identity into the latency-gate binary so a stale
//! executable cannot claim a later checkout's SHA.

fn git(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn main() {
    println!("cargo:rerun-if-changed=src/css_identities.rs");
    println!("cargo:rerun-if-changed=benches/css_bench.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let sha = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unavailable".to_string());
    let tree = git(&["rev-parse", "HEAD^{tree}"]).unwrap_or_else(|| "unavailable".to_string());
    let dirty = git(&["status", "--porcelain"])
        .map(|status| !status.is_empty())
        .unwrap_or(true);
    println!("cargo:rustc-env=VERTER_BENCH_COMMIT_SHA={sha}");
    println!("cargo:rustc-env=VERTER_BENCH_TREE_ID={tree}");
    println!(
        "cargo:rustc-env=VERTER_BENCH_DIRTY={}",
        if dirty { "1" } else { "0" }
    );
}
