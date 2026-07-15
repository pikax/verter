//! LEGACY_GATE_SELF — Guard: declared-only component-meta surface is fully
//! removed.
//!
//! Asserts via `syn::parse_file` against the canonical declaration sites:
//!
//! - `ComponentMetaQueryKind` enum (or any successor) declared in
//!   `crates/verter_session/src/component_meta_result_db.rs` does NOT have
//!   a variant named `Compat`. If the enum collapsed to a single variant,
//!   it must have been deleted entirely.
//! - No `match` arm in any production source file under
//!   `crates/verter_session/src` reads a `Compat` variant of the query
//!   kind.
//! - The legacy `get_declared_component_meta` family (`get_declared_component_meta`,
//!   `get_declared_component_meta_with_resolution`, `get_declared_component_meta_payload`)
//!   is not present as a function name in any production source file.
//!
//! Self-exclusion: the first 5 lines of this file contain `LEGACY_GATE_SELF`
//! so the recursive walk skips this file.

use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::Visibility;

const RETIRED_DECLARED_FN_NAMES: &[&str] = &[
    "get_declared_component_meta",
    "get_declared_component_meta_with_resolution",
    "get_declared_component_meta_payload",
];

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join("CLAUDE.md").exists() && p.join("crates").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "unable to locate workspace root from {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

fn is_self_excluded(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.lines().take(5).any(|l| l.contains("LEGACY_GATE_SELF"))
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Skip target dir, node_modules, .git, and the tests/ directory
            // itself (we are scanning production source only).
            if name == "target" || name == "node_modules" || name == ".git" || name == "tests" {
                continue;
            }
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
            && !is_self_excluded(&path)
        {
            out.push(path);
        }
    }
}

#[test]
fn component_meta_query_kind_has_no_compat_variant() {
    let root = workspace_root();
    let path = root
        .join("crates")
        .join("verter_session")
        .join("src")
        .join("component_meta_result_db.rs");
    if !path.exists() {
        // The file may have been deleted entirely if the enum collapsed and
        // its module was inlined. That is also an acceptable state.
        return;
    }
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let file = syn::parse_file(&text).unwrap_or_else(|e| {
        panic!("failed to parse {}: {e}", path.display());
    });

    for item in &file.items {
        let syn::Item::Enum(enum_item) = item else {
            continue;
        };
        if enum_item.ident != "ComponentMetaQueryKind" {
            continue;
        }
        for variant in &enum_item.variants {
            let name = variant.ident.to_string();
            assert!(
                name != "Compat",
                "ComponentMetaQueryKind retains forbidden `Compat` variant in {}",
                path.display()
            );
        }
    }
}

struct CompatArmVisitor {
    matches: Vec<String>,
}

impl<'ast> Visit<'ast> for CompatArmVisitor {
    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        let pattern_text = quote::quote!(#arm).to_string();
        // Match `ComponentMetaQueryKind :: Compat` patterns regardless of
        // whitespace; quote::ToTokens emits canonical spacing.
        if pattern_text.contains("ComponentMetaQueryKind :: Compat")
            || pattern_text.contains("ComponentMetaQueryKind::Compat")
        {
            self.matches.push(pattern_text);
        }
        syn::visit::visit_arm(self, arm);
    }
}

#[test]
fn no_match_arm_reads_compat_variant() {
    let root = workspace_root();
    let src = root.join("crates").join("verter_session").join("src");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_rs_files(&src, &mut files);

    let mut offenders: Vec<(PathBuf, String)> = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        // Necessary-condition pre-filter: a `ComponentMetaQueryKind::Compat`
        // match arm MUST contain the substring `Compat`. A file without it
        // cannot host the offending arm, so skip the `syn` parse + AST walk
        // entirely. This cannot hide a violation — the substring is a strict
        // prerequisite for the pattern this test searches for.
        if !text.contains("Compat") {
            continue;
        }
        let Ok(file) = syn::parse_file(&text) else {
            continue;
        };
        let mut visitor = CompatArmVisitor {
            matches: Vec::new(),
        };
        visitor.visit_file(&file);
        for m in visitor.matches {
            offenders.push((path.clone(), m));
        }
    }

    assert!(
        offenders.is_empty(),
        "found {} match arm(s) reading `ComponentMetaQueryKind::Compat`:\n{}",
        offenders.len(),
        offenders
            .iter()
            .map(|(p, arm)| format!("  {} :: {}", p.display(), arm))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn no_get_declared_component_meta_function_remains() {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    collect_rs_files(
        &root.join("crates").join("verter_session").join("src"),
        &mut files,
    );
    collect_rs_files(
        &root.join("crates").join("verter_napi").join("src"),
        &mut files,
    );

    let mut offenders: Vec<(PathBuf, String)> = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        // Necessary-condition pre-filter: every retired function name in
        // `RETIRED_DECLARED_FN_NAMES` begins with `get_declared_component_meta`,
        // so a file declaring any of them MUST contain that substring. Skip the
        // `syn` parse + item walk for files that lack it. The substring is a
        // strict prerequisite for the declarations this test searches for, so
        // filtering cannot hide a real offender.
        if !text.contains("get_declared_component_meta") {
            continue;
        }
        let Ok(file) = syn::parse_file(&text) else {
            continue;
        };
        for item in &file.items {
            collect_fn_offenders(item, path, &mut offenders);
        }
    }

    assert!(
        offenders.is_empty(),
        "found {} legacy declared component-meta function declaration(s):\n{}",
        offenders.len(),
        offenders
            .iter()
            .map(|(p, name)| format!("  {} :: fn {}", p.display(), name))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn collect_fn_offenders(item: &syn::Item, path: &Path, offenders: &mut Vec<(PathBuf, String)>) {
    match item {
        syn::Item::Fn(fn_item) => {
            let name = fn_item.sig.ident.to_string();
            if RETIRED_DECLARED_FN_NAMES.contains(&name.as_str()) {
                offenders.push((path.to_path_buf(), name));
            }
        }
        syn::Item::Impl(impl_block) => {
            for item in &impl_block.items {
                if let syn::ImplItem::Fn(method) = item {
                    let name = method.sig.ident.to_string();
                    if RETIRED_DECLARED_FN_NAMES.contains(&name.as_str()) {
                        // Only count `pub` and `pub(crate)` methods — `pub(super)`
                        // and private methods are not part of the public surface.
                        let is_public = matches!(method.vis, Visibility::Public(_))
                            || matches!(&method.vis, Visibility::Restricted(r) if r.path.is_ident("crate"));
                        if is_public {
                            offenders.push((path.to_path_buf(), name));
                        }
                    }
                }
            }
        }
        syn::Item::Mod(mod_item) => {
            if let Some((_, items)) = &mod_item.content {
                for inner in items {
                    collect_fn_offenders(inner, path, offenders);
                }
            }
        }
        _ => {}
    }
}
