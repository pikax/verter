//! Real-provider regression coverage for QUOTED hyphenated prop names in the
//! synthesized public API carrier.
//!
//! A `defineProps` type literal declaring a quoted non-identifier member
//! (`"onLate-signal"?: (sig) => void`) must survive into the `.vue.verter.ts`
//! public-API carrier as a QUOTED member of the SINGLE `new(props?: ...)`
//! parameter object. When the key rendered bare (invalid TypeScript), provider
//! error-recovery corrupted the construct signature — a phantom second required
//! `onLate: any` parameter and an `any` instance — which silently DISABLED
//! consumer prop type-checking: a mistyped prop produced NO TS2322.
//!
//! These tests drive the REAL providers over the REAL generated outputs:
//! a `VerterHost` compiles the component to its public-API carrier and a
//! consumer `.vue` to IDE TSX, both are written into a temp project wired to
//! the workspace `node_modules`, and each provider family is asked for the
//! consumer's diagnostics. THE PROOF: the deliberate prop mistype surfaces
//! TS2322 — `$props` typing is restored, not `any`.
//!
//! Hermeticity: each test gracefully skips (early `return` with `eprintln!`)
//! when its provider assets are unavailable, matching the sibling
//! `tsserver_e2e_generated_outputs` / `shared_provider_live` gating.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use verter_session::{
    CompileProfile, CompileTarget, FileLanguage, HostConfig, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

use verter_lsp::tsserver::ipc::TsserverTypeProvider;
use verter_lsp::type_provider::traits::TypeProvider;

use super::tsserver_e2e_generated_outputs::{
    create_test_project_with_workspace_node_modules, tsserver_assets_or_skip,
    workspace_node_modules,
};

/// The component under test: a quoted hyphenated prop BETWEEN two identifier
/// props. `label` is declared AFTER the quoted key so a bare (invalid) render
/// swallows it during provider error-recovery — exactly the reported corruption.
const LATE_SIGNAL_COMP: &str = r#"<script setup lang="ts">
defineProps<{
  onPing?: (payload: { pingCode: string; pingCount: number }) => void;
  "onLate-signal"?: (sig: { sigName: string; sigLevel: number }) => void;
  label?: string;
}>();
</script>

<template>
  <div>late signal comp</div>
</template>
"#;

/// The consumer: passes a MISTYPED `label` (number into `string`) to the
/// component. Post-fix this must surface TS2322 through the carrier; pre-fix
/// the corrupted `any` instance silently accepted it.
const MISTYPE_CONSUMER: &str = r#"<script setup lang="ts">
import LateSignalComp from './LateSignalComp.vue'
</script>

<template>
  <LateSignalComp :label="123" />
</template>
"#;

const COMP_ID: &str = "/src/LateSignalComp.vue";
const CONSUMER_ID: &str = "/src/MistypeConsumer.vue";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crates/")
        .to_path_buf()
}

/// One generated companion to place on disk: `(relative_name, code)`.
struct CompanionFile {
    name: String,
    code: String,
}

/// Discover every companion file name `code` references for `stem`
/// (`<stem>.vue`, `<stem>.vue.verter.ts`, …). A bare `.vue` specifier resolves
/// on disk via TypeScript's specifier+extension lookup to `<stem>.vue.ts` (the
/// same layout the sibling scoped-slot e2e proves green); an explicit
/// `…ts` reference maps to that exact name.
fn referenced_companion_names(code: &str, stem: &str) -> Vec<String> {
    let needle = format!("{stem}.vue");
    let mut names: Vec<String> = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = code[search_from..].find(&needle) {
        let start = search_from + rel;
        let rest = &code[start..];
        let mut end = needle.len();
        let bytes = rest.as_bytes();
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric()
                || bytes[end] == b'.'
                || bytes[end] == b'-'
                || bytes[end] == b'_')
        {
            end += 1;
        }
        let referenced = &rest[..end];
        let name = if referenced.ends_with(".ts") {
            referenced.to_string()
        } else {
            format!("{referenced}.ts")
        };
        if !names.iter().any(|n| n == &name) {
            names.push(name);
        }
        search_from = start + needle.len();
    }
    names
}

/// Compile the component + consumer through the production pipeline and return
/// `(consumer_ide_tsx, companion_files)`.
///
/// The companion file names are DISCOVERED from the consumer's generated TSX
/// (every `LateSignalComp.vue…` / `MistypeConsumer.vue…` companion
/// reference), never hardcoded, so the test tracks the descriptor-owned
/// naming.
fn compile_outputs() -> (String, Vec<CompanionFile>) {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(COMP_ID.to_string()),
        input_id: COMP_ID.to_string(),
        source: Arc::from(LATE_SIGNAL_COMP),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CONSUMER_ID.to_string()),
        input_id: CONSUMER_ID.to_string(),
        source: Arc::from(MISTYPE_CONSUMER),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    let profile = CompileProfile {
        source_map: false,
        target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
        embed_ambient_types: false,
        ..Default::default()
    };

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(COMP_ID.to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })
        .expect("component compilation should succeed");
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(CONSUMER_ID.to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })
        .expect("consumer compilation should succeed");

    let comp_api = host
        .get_public_api(COMP_ID)
        .expect("component public API should resolve")
        .expect("component public API should exist");
    let consumer_api = host
        .get_public_api(CONSUMER_ID)
        .expect("consumer public API should resolve")
        .expect("consumer public API should exist");
    let consumer_ide = host
        .get_ide(CONSUMER_ID, &profile)
        .expect("consumer IDE output should exist");

    let consumer_code = consumer_ide.code.to_string();

    let mut companions: Vec<CompanionFile> = Vec::new();
    let comp_names = referenced_companion_names(&consumer_code, "LateSignalComp");
    assert!(
        !comp_names.is_empty(),
        "consumer IDE TSX must reference the component; got: {consumer_code}"
    );
    for name in comp_names {
        companions.push(CompanionFile {
            name,
            code: comp_api.code.to_string(),
        });
    }
    // The consumer TSX re-exports its own default through its API companion
    // (`export {{ default }} from './MistypeConsumer.vue.verter.ts'`); place it
    // so every reference resolves.
    for name in referenced_companion_names(&consumer_code, "MistypeConsumer") {
        companions.push(CompanionFile {
            name,
            code: consumer_api.code.to_string(),
        });
    }

    (consumer_code, companions)
}

/// Write the generated outputs into the temp project. Returns the consumer's
/// on-disk path and every companion path written.
fn write_outputs(
    tmp: &Path,
    consumer_code: &str,
    companions: &[CompanionFile],
) -> (PathBuf, Vec<PathBuf>) {
    let src_dir = tmp.join("src");
    let consumer_path = src_dir.join("MistypeConsumer.vue.tsx");
    std::fs::write(&consumer_path, consumer_code).expect("consumer IDE should be written");

    let mut companion_paths = Vec::new();
    for companion in companions {
        let path = src_dir.join(&companion.name);
        std::fs::write(&path, &companion.code).expect("API companion should be written");
        companion_paths.push(path);
    }
    (consumer_path, companion_paths)
}

/// TSSERVER — the consumer's mistyped prop must surface TS2322 through the
/// generated carrier, and the carrier itself must parse clean (no syntactic
/// diagnostics from the invalid bare hyphenated member).
#[tokio::test]
async fn quoted_prop_consumer_mistype_surfaces_ts2322_tsserver() {
    let Some((node_path, tsserver_path)) = tsserver_assets_or_skip() else {
        eprintln!("skipping: node or tsserver.js not found");
        return;
    };

    let tmp = std::env::temp_dir().join(format!(
        "verter_quoted_prop_mistype_tsserver_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    if create_test_project_with_workspace_node_modules(&tmp).is_err() {
        eprintln!("skipping: could not create test project with workspace node_modules");
        return;
    }

    let (consumer_code, companions) = compile_outputs();
    let (consumer_path, companion_paths) = write_outputs(&tmp, &consumer_code, &companions);

    let provider = TsserverTypeProvider::spawn(
        &node_path,
        &tsserver_path,
        tmp.to_str().expect("tmp path should be valid UTF-8"),
        None,
        None,
        false,
        None,
    )
    .await
    .expect("tsserver should spawn");

    let consumer_path_str = consumer_path.to_string_lossy().replace('\\', "/");
    for (path, companion) in companion_paths.iter().zip(companions.iter()) {
        let path_str = path.to_string_lossy().replace('\\', "/");
        provider
            .open_file(&path_str, &companion.code)
            .await
            .expect("companion should open");
    }
    provider
        .open_file(&consumer_path_str, &consumer_code)
        .await
        .expect("consumer should open");

    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    let consumer_diags = provider
        .get_diagnostics(&consumer_path_str)
        .await
        .expect("consumer diagnostics should succeed");
    let consumer_codes: Vec<_> = consumer_diags
        .iter()
        .map(|d| (d.code.clone(), d.message.clone()))
        .collect();
    eprintln!("[tsserver] consumer diagnostics = {consumer_codes:?}");

    // THE PROOF: the mistyped `label` (number into `string`) errors — the
    // component's `$props` typing flowed through the carrier, not `any`.
    assert!(
        consumer_diags
            .iter()
            .any(|d| d.code.as_deref() == Some("2322")),
        "the consumer's mistyped prop must surface TS2322 through the generated \
         carrier (a silent pass means the instance degraded to `any`); got {consumer_codes:?}"
    );
    // NEGATIVE: every consumer reference resolves — no TS2307.
    assert!(
        !consumer_diags
            .iter()
            .any(|d| d.code.as_deref() == Some("2307")),
        "every consumer import must resolve (no TS2307); got {consumer_codes:?}"
    );

    // NEGATIVE: the companions themselves parse clean — the quoted hyphenated
    // key never renders bare (which produced TS1xxx syntax errors pre-fix).
    for path in &companion_paths {
        let path_str = path.to_string_lossy().replace('\\', "/");
        let carrier_diags = provider
            .get_diagnostics(&path_str)
            .await
            .expect("carrier diagnostics should succeed");
        let syntax_errors: Vec<_> = carrier_diags
            .iter()
            .filter(|d| {
                d.code
                    .as_deref()
                    .and_then(|c| c.parse::<u32>().ok())
                    .is_some_and(|c| (1000..2000).contains(&c))
            })
            .map(|d| (d.code.clone(), d.message.clone()))
            .collect();
        assert!(
            syntax_errors.is_empty(),
            "the generated carrier must parse clean (quoted keys preserved); got \
             syntax errors {syntax_errors:?} in {path_str}"
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

/// TSGO — the same proof through the typescript-go engine: a batch
/// `--project --noEmit` run over the generated outputs reports TS2322 for the
/// consumer's mistyped prop and NO syntax errors for the carrier.
#[tokio::test]
async fn quoted_prop_consumer_mistype_surfaces_ts2322_tsgo() {
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        verter_tsgo_api::toolchain::validation::Capability::Lsp,
        Some(workspace_root()),
    );
    let Some(tsgo) = (match verter_tsgo_api::toolchain::discovery::resolve(&request).await {
        Ok(resolution) => Some(resolution.path),
        Err(e) => {
            if std::env::var("VERTER_REQUIRE_TSGO").is_ok() {
                panic!(
                    "VERTER_REQUIRE_TSGO is set but tsgo was not found: {e}. \
                     A skip would be a vacuous pass."
                );
            }
            eprintln!("[skip] tsgo engine not found ({e})");
            None
        }
    }) else {
        return;
    };
    if workspace_node_modules().is_none() {
        eprintln!("skipping: workspace node_modules not found");
        return;
    }

    let tmp = std::env::temp_dir().join(format!(
        "verter_quoted_prop_mistype_tsgo_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    if create_test_project_with_workspace_node_modules(&tmp).is_err() {
        eprintln!("skipping: could not create test project with workspace node_modules");
        return;
    }

    let (consumer_code, companions) = compile_outputs();
    let (_consumer_path, _companion_paths) = write_outputs(&tmp, &consumer_code, &companions);

    let output = std::process::Command::new(&tsgo)
        .arg("--project")
        .arg(tmp.join("tsconfig.json"))
        .arg("--noEmit")
        .arg("--pretty")
        .arg("false")
        .current_dir(&tmp)
        .output()
        .expect("tsgo batch typecheck should run");
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("[tsgo] batch output:\n{text}");

    // THE PROOF: TS2322 attributed to the consumer file.
    let consumer_2322 = text
        .lines()
        .any(|line| line.contains("MistypeConsumer.vue.tsx") && line.contains("error TS2322"));
    assert!(
        consumer_2322,
        "tsgo must report TS2322 for the consumer's mistyped prop (a silent pass \
         means the instance degraded to `any`); got:\n{text}"
    );

    // NEGATIVE: every generated file parses clean — no TS1xxx syntax errors
    // (pre-fix the bare hyphenated member produced a storm of them in the
    // carrier).
    let syntax_error = text.lines().any(|line| {
        line.split("error TS")
            .nth(1)
            .and_then(|rest| {
                rest.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u32>()
                    .ok()
            })
            .is_some_and(|code| (1000..2000).contains(&code))
    });
    assert!(
        !syntax_error,
        "the generated outputs must parse clean under tsgo (quoted keys preserved); got:\n{text}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
