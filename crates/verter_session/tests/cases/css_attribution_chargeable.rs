//! A27: every CSS-domain attribution counter remains chargeable by a
//! production path. Universe comes from the typed schema, not from
//! `performance-gates.toml`.
//!
//! Mutation (one demonstration per class): deleting a rehomed charge site
//! without a replacement reddens only that counter. Covered by driving
//! production work and asserting each schema CSS-domain site records at
//! least one call.

use std::path::PathBuf;
use std::sync::Arc;

use verter_audit::attribution::{reset, WorkDomain, WorkSite};
use verter_session::{
    CompileProfile, CompileTarget, FileLanguage, HostConfig, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn collect_zero_assertions(value: &toml::Value, out: &mut Vec<String>) {
    match value {
        toml::Value::Table(table) => {
            if let Some(arr) = table
                .get("zero_counter_assertions")
                .and_then(|entry| entry.as_array())
            {
                for item in arr {
                    if let Some(name) = item.as_str() {
                        out.push(name.to_string());
                    }
                }
            }
            for nested in table.values() {
                collect_zero_assertions(nested, out);
            }
        }
        toml::Value::Array(items) => {
            for nested in items {
                collect_zero_assertions(nested, out);
            }
        }
        _ => {}
    }
}

fn css_domain_sites() -> Vec<WorkSite> {
    WorkSite::ALL
        .iter()
        .copied()
        .filter(|site| site.domain() == WorkDomain::Css)
        .collect()
}

fn upsert(host: &VerterHost, id: &str, source: &str, language: FileLanguage) -> String {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(source),
        file_language: language,
        aliases: Vec::new(),
    })
    .expect("upsert")
    .canonical_id
}

fn compile_style(host: &VerterHost, canonical_id: &str) {
    let profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        ..CompileProfile::default()
    };
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical_id.to_string()),
            node_kind: Some(VirtualNodeKind::Style { index: 0 }),
            compile_profile: profile,
        })
        .unwrap_or_else(|error| panic!("style compile must succeed: {error:?}"));
}

/// Chargeability: for every counter in the attribution schema's `Css`
/// domain, a workload performing the work that counter names charges it
/// at least once. Deleting a name from `zero_counter_assertions` does
/// not shrink this universe.
#[test]
fn every_css_domain_counter_is_chargeable_by_production() {
    let sites = css_domain_sites();
    assert!(
        !sites.is_empty(),
        "the schema must declare at least one Css-domain counter"
    );

    reset();

    {
        let host = VerterHost::new_standalone(HostConfig::default());
        let canonical = upsert(
            &host,
            "/workspace/A27.vue",
            "<template><div class=\"card\">x</div></template>\
             <style scoped>.card { color: red; }</style>",
            FileLanguage::vue(),
        );
        compile_style(&host, &canonical);
    }

    {
        let host = VerterHost::new_standalone(HostConfig::default());
        let canonical = upsert(
            &host,
            "/workspace/A27.svelte",
            "<div class=\"card\">x</div>\n<style>.card { color: red; }</style>",
            FileLanguage::svelte(),
        );
        compile_style(&host, &canonical);
    }

    let mut uncharged = Vec::new();
    for site in &sites {
        let sample = verter_audit::attribution::read(*site);
        if sample.calls == 0 {
            uncharged.push(site.id());
        }
    }
    assert!(
        uncharged.is_empty(),
        "CSS-domain counters with no production charge site (a zero assertion that \
         cannot fail because nothing CAN charge them): {uncharged:?}"
    );
}

/// Coverage: every name in `zero_counter_assertions` resolves to a
/// counter in the schema's Css domain.
#[test]
fn asserted_zero_counters_resolve_to_schema_counters() {
    let gates_path = workspace_root().join("performance-gates.toml");
    let text = std::fs::read_to_string(&gates_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", gates_path.display()));
    let parsed: toml::Value = toml::from_str(&text)
        .unwrap_or_else(|error| panic!("parse {}: {error}", gates_path.display()));
    let mut names = Vec::new();
    collect_zero_assertions(&parsed, &mut names);
    assert!(
        !names.is_empty(),
        "zero_counter_assertions must not be an empty list"
    );

    let css_ids: Vec<&str> = css_domain_sites().into_iter().map(WorkSite::id).collect();
    let css_asserted: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| {
            name.starts_with("compiler.css")
                || *name == "compiler.style_analysis"
                || css_ids.contains(name)
        })
        .collect();
    assert!(
        !css_asserted.is_empty(),
        "performance-gates.toml must still list the Css-domain schema counters among \
         zero_counter_assertions (schema Css ids: {css_ids:?})"
    );
    let mut unknown = Vec::new();
    for name in &css_asserted {
        if !css_ids.contains(name) {
            unknown.push(*name);
        }
    }
    assert!(
        unknown.is_empty(),
        "CSS zero_counter_assertions names that do not resolve to a Css-domain schema \
         counter: {unknown:?} (schema Css ids: {css_ids:?})"
    );
}
