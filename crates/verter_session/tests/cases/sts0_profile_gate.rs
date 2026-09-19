//! STS0 Svelte projection profile gate — the live runtime-correctness proof
//! behind `tests/sfc-projection/STS0` (protocol.mjs executes these named
//! tests; the frozen charter §14 forbids a documentation-only runtime claim,
//! and §10 forbids a duplicate script-body checker, so no virtual projector
//! exists on this path).
//!
//! Every `SvelteProjectionPolicy` profile row's evidence fixture is driven
//! through the OWNED CCA1I `SvelteProjectionBackend` (`project_ide`, with an
//! admission-carved consume-once execution grant), and the REAL generated
//! carrier is checked on BOTH claimed engines: ts-js 6.0.3 (the STP1 pin at
//! `packages/playground`) and ts-native 7.0.2 (the workspace root install).
//! Corruption twins corrupt the evidence template AND the generated carrier
//! itself — engine-checked rows must fail on both engines, js-unchecked rows
//! must not report. Publication is consumed for real: the host declaration
//! carrier (declarations-published rows) is checked with an actual consumer
//! on each engine, and the module exports carried by the projection
//! (module-exports-published rows) are declaration-emitted with renaming
//! twins that must lose the published symbol.
//!
//! The gate is data-driven from the STS0 product JSON — adding a profile row
//! adds a gate case with no edit here. Engines follow the same skip/hard-fail
//! policy as `svelte_typecheck_gate`: hermetic machines without the pinned
//! installs skip with a note; `CI`/`VERTER_REQUIRE_TYPECHECKER` makes a
//! missing engine a hard failure so the gate cannot silently skip where it
//! is meant to run.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde_json::Value;
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest,
    SvelteCompileRequest,
};
use verter_compiler::framework_common::registered_carrier_projection::project_registered_accepted;
use verter_compiler::framework_common::{
    FrameworkHostIntegrationBackend, FrameworkParseArtifact, ProductExecutionGrant,
    ProjectionBackend, SvelteHostIntegrationBackend, SvelteHostMultiProductDemand,
};
use verter_compiler::svelte::{SvelteProjectionBackend, SvelteProjectionInputs};
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::FileLanguage;
use verter_session::{HostConfig, PublicApiMode, UpsertRequest, VerterHost};

/// The crate's shared gate-fixture root (vendored hermetic `svelte` types).
fn gate_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cases/svelte_typecheck_gate")
}

/// The workspace root (`<ws>/crates/verter_session` → `<ws>`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate is <ws>/crates/verter_session")
        .to_path_buf()
}

/// The STS0 node directory holding the ratified products and probes.
fn sts0_dir() -> PathBuf {
    workspace_root().join("tests/sfc-projection/STS0")
}

/// True when the environment REQUIRES the pinned engines to be present
/// (CI runs or an explicit opt-in) — a missing engine is then a hard failure.
fn require_engines() -> bool {
    fn truthy(name: &str) -> bool {
        std::env::var_os(name).is_some_and(|v| {
            let v = v.to_string_lossy();
            let v = v.trim();
            !v.is_empty() && !v.eq_ignore_ascii_case("0") && !v.eq_ignore_ascii_case("false")
        })
    }
    truthy("CI") || truthy("VERTER_REQUIRE_TYPECHECKER")
}

fn skip_note(name: &str) {
    eprintln!(
        "SKIP {name}: a pinned STS0 engine launcher was not found under node_modules \
         (hermetic machine); run on a machine with the pinned installs to exercise the gate"
    );
}

/// Read the exact `version` field of the package.json inside `dir`.
fn package_version(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let key = text.find("\"version\"")?;
    let after = &text[key..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim_start();
    let quote = rest.find('"')? + 1;
    let tail = &rest[quote..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}

/// One claimed engine: the ts-js pin runs its `tsc.js` launcher through
/// `node`; the ts-native pin is the platform engine binary the STP1 matrix
/// names (`@typescript/typescript-<platform>-<arch>`, `lib/tsc`), executed
/// directly — no `.bin` shim or `tsc.js` wrapper dependence.
#[derive(Clone)]
struct Engine {
    label: &'static str,
    launcher: PathBuf,
    through_node: bool,
}

impl Engine {
    /// Resolve + version-assert the engine, or `None` (skip / hard-fail).
    fn resolve(
        label: &'static str,
        expected: &'static str,
        package_dir: PathBuf,
    ) -> Option<Engine> {
        let found = package_version(&package_dir);
        let missing = match found.as_deref() {
            Some(version) if version == expected => false,
            Some(other) => panic!(
                "the STS0 profile gate requires {label} {expected} but found {other} at {}",
                package_dir.display()
            ),
            None => true,
        };
        let launcher = if missing {
            None
        } else if label == "ts-js" {
            package_dir
                .join("lib")
                .join("tsc.js")
                .is_file()
                .then(|| (package_dir.join("lib").join("tsc.js"), true))
        } else {
            resolve_native_tsc(expected)
        };
        match launcher {
            Some((launcher, through_node)) => Some(Engine {
                label,
                launcher,
                through_node,
            }),
            None => {
                assert!(
                    !require_engines(),
                    "the STS0 profile gate REQUIRES the pinned engines here \
                     (CI / VERTER_REQUIRE_TYPECHECKER is set) but {label} was not resolvable \
                     from {}. A silent skip would mask Svelte profile regressions — run \
                     `pnpm install` or unset the env var for a local dev skip.",
                    package_dir.display()
                );
                None
            }
        }
    }
}

/// Resolve the pinned native engine binary: the hoisted platform package
/// first, then the pnpm store entries (`node_modules/.pnpm/@typescript+
/// typescript-<platform>-<arch>@<version>/…`), keeping the entry whose
/// owning package matches the pinned version exactly.
fn resolve_native_tsc(expected: &str) -> Option<(PathBuf, bool)> {
    let platform = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    let pkg_name = format!("@typescript/typescript-{platform}-{arch}");
    let exe = if cfg!(windows) { "tsc.exe" } else { "tsc" };
    let root = workspace_root().join("node_modules");

    let hoisted = root.join(&pkg_name).join("lib").join(exe);
    if hoisted.is_file() {
        return Some((hoisted, false));
    }

    let store = root.join(".pnpm");
    let mut candidates: Vec<(String, PathBuf)> = std::fs::read_dir(&store)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let scoped = name.replace('+', "/");
            let prefix = format!("{}@", pkg_name);
            if !scoped.starts_with(&prefix) {
                return None;
            }
            let version = scoped[prefix.len()..].to_string();
            let launcher = entry
                .path()
                .join("node_modules")
                .join(&pkg_name)
                .join("lib")
                .join(exe);
            launcher.is_file().then_some((version, launcher))
        })
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates
        .into_iter()
        .find(|(version, _)| version == expected)
        .map(|(_, launcher)| (launcher, false))
}

fn claimed_engines() -> Vec<Engine> {
    let root = workspace_root();
    [
        Engine::resolve(
            "ts-js",
            "6.0.3",
            root.join("packages/playground/node_modules/typescript"),
        ),
        Engine::resolve("ts-native", "7.0.2", root.join("node_modules/typescript")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// One policy profile row, as ratified by the STS0 product.
struct ProfileRow {
    id: String,
    file_kind: String,
    dialect: String,
    checking: String,
    publishing: String,
    evidence_path: String,
    published_symbols: Vec<String>,
}

fn load_profile_rows() -> Vec<ProfileRow> {
    let text = std::fs::read_to_string(
        sts0_dir()
            .join("products")
            .join("svelte-projection-policy.json"),
    )
    .expect("the STS0 SvelteProjectionPolicy product must be readable");
    let value: Value = serde_json::from_str(&text).expect("the policy product must parse");
    value["profiles"]
        .as_array()
        .expect("policy.profiles is an array")
        .iter()
        .map(|row| ProfileRow {
            id: row["id"].as_str().unwrap_or_default().to_string(),
            file_kind: row["fileKind"].as_str().unwrap_or_default().to_string(),
            dialect: row["dialect"].as_str().unwrap_or_default().to_string(),
            checking: row["checking"].as_str().unwrap_or_default().to_string(),
            publishing: row["publishing"].as_str().unwrap_or_default().to_string(),
            evidence_path: row["evidencePath"].as_str().unwrap_or_default().to_string(),
            published_symbols: row["publishedSymbols"]
                .as_array()
                .map(|symbols| {
                    symbols
                        .iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

fn evidence_text(row: &ProfileRow) -> String {
    std::fs::read_to_string(workspace_root().join(&row.evidence_path)).unwrap_or_else(|e| {
        panic!(
            "evidence fixture {} must be readable: {e}",
            row.evidence_path
        )
    })
}

/// A registered parse artifact for a Svelte source (the admission path the
/// backend consumes) — mirroring the CCA1I backend contract test.
fn svelte_artifact(canonical: &str, source: &str) -> FrameworkParseArtifact {
    let language = FileLanguage::svelte();
    let source_authority = RegisteredSourceAuthority::new().expect("source authority");
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new(canonical),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            language.clone(),
            Arc::from(source),
        )
        .expect("registered source");
    let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
    let config = CarrierGrammarConfig::Svelte;
    grammar_authority
        .register_carrier_grammar(
            language,
            FrameworkAdapterSemanticVersion::new(1).expect("adapter version"),
            CarrierParserGrammarVersion::new(1).expect("grammar version"),
            config.clone(),
        )
        .expect("grammar registration");
    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &config)
        .expect("accepted source");
    project_registered_accepted(&accepted)
        .expect("registered projection")
        .into_framework_parse_artifact()
}

const GRANT_MINT: &str =
    "<script lang=\"ts\">\nlet count = $state(1);\n</script>\n<div>{count}</div>\n";

/// A genuine consume-once IDE grant minted through the registered Svelte
/// host-integration backend — the only out-of-crate grant source.
fn ide_grant() -> ProductExecutionGrant {
    let artifact = svelte_artifact("file:///sts0-grant-mint.svelte", GRANT_MINT);
    SvelteHostIntegrationBackend::registered()
        .admit_host_products(
            &artifact,
            SvelteHostMultiProductDemand {
                products: vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
                ..Default::default()
            },
        )
        .expect("the grant-mint admission issues")
        .into_execution_grants()
        .projection
        .expect("the projection leg was admitted")
}

fn ide_only_request(filename: &str) -> CompileRequest {
    CompileRequest::new(
        vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        Some(filename.to_string()),
        None,
        false,
        false,
    )
    .expect("ide-only request constructs")
}

/// Project a `.svelte` source through the OWNED CCA1I backend and return the
/// real generated carrier (code, is_jsx).
fn project_through_owned_backend(source: &str) -> (String, bool) {
    let artifact = svelte_artifact("file:///sts0-profile.svelte", source);
    let companion = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            source,
            &artifact,
            &ide_only_request("Sts0Profile.svelte"),
            &SvelteProjectionInputs,
        )
        .unwrap_or_else(|e| panic!("the owned backend must project the profile fixture: {e:?}"));
    (companion.ide.code.to_string(), companion.ide.is_jsx)
}

/// The pinned framework's own rune ambient (`node_modules/svelte` is the
/// pinned-official-tooling population; its types declare the runes the
/// `.svelte.ts`/`.svelte.js` module surfaces check against). Version-gated
/// to the pinned 5.56.10 so a drifted install fails loudly, not silently.
fn pinned_svelte_rune_ambient() -> Option<String> {
    let svelte_dir = workspace_root().join("node_modules/svelte");
    let version = package_version(&svelte_dir)?;
    assert_eq!(
        version, "5.56.10",
        "the STS0 profile gate reads the rune ambient from the pinned svelte install, \
         but found version {version}"
    );
    Some(
        std::fs::read_to_string(svelte_dir.join("types/index.d.ts"))
            .expect("the pinned svelte install ships types/index.d.ts"),
    )
}

/// The rune ambient extra file for rows whose surface checks runes without
/// declaring them itself (the engine-checked `.svelte.js` module).
fn rune_ambient_extra(row: &ProfileRow) -> Vec<(&'static str, String)> {
    if row.file_kind == ".svelte.js" && row.checking == "engine-checked" {
        pinned_svelte_rune_ambient()
            .map(|ambient| vec![("sts0-svelte-runenv.d.ts", ambient)])
            .unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// The checked surface for a profile row: the real backend carrier for
/// `.svelte` rows, or the module file itself for the module-file kinds.
#[derive(Clone)]
struct CarrierSpec {
    entry: String,
    text: String,
    check_js: bool,
}

fn slug(id: &str) -> String {
    id.replace(['-', '.'], "_")
}

fn carrier_for(row: &ProfileRow) -> CarrierSpec {
    carrier_from_evidence(row, evidence_text(row))
}

fn carrier_from_evidence(row: &ProfileRow, evidence: String) -> CarrierSpec {
    let check_js = row.checking == "engine-checked" && row.dialect == "js";
    if row.file_kind == ".svelte" {
        let (code, is_jsx) = project_through_owned_backend(&evidence);
        let ext = if is_jsx { "jsx" } else { "tsx" };
        CarrierSpec {
            entry: format!("Sts0{}.svelte.{ext}", slug(&row.id)),
            text: code,
            check_js,
        }
    } else {
        let ext = if row.dialect == "js" { "js" } else { "ts" };
        CarrierSpec {
            entry: format!("Sts0{}.{ext}", slug(&row.id)),
            text: evidence,
            check_js,
        }
    }
}

/// The type corruption appended to module-file evidence and to generated
/// carriers (post-projection). The TS form uses a wrong-typed binding; the
/// JS form stays syntactically valid JavaScript (a missing-property access)
/// so an unchecked surface reports nothing for SYNTACTIC reasons either.
const TYPE_CORRUPTION_TS: &str = "\nconst __sts0_corruption: number = \"sts0-type-corruption\";\n";
const TYPE_CORRUPTION_JS: &str = "\nconst __sts0_corruption = \"sts0\".sts0MissingProperty;\n";

fn type_corruption_for(dialect: &str) -> &'static str {
    if dialect == "js" {
        TYPE_CORRUPTION_JS
    } else {
        TYPE_CORRUPTION_TS
    }
}

/// The template corruption appended to `.svelte` evidence: an interpolation
/// of an unknown global — the owned projection must carry it into the
/// carrier and every engine-checked row must flag it.
const TEMPLATE_CORRUPTION: &str = "\n<p>{sts0MissingGlobal.sts0MissingMember}</p>\n";

fn assert_failed(run: &EngineRun, engine: &str, row: &str, kind: &str) {
    assert!(
        !run.ok,
        "{kind} must be caught for profile {row} on {engine}:\n{}\n{}",
        run.output, run.declaration_text
    );
    assert!(
        run.output.contains("error TS"),
        "{kind} for profile {row} on {engine} failed without a TypeScript diagnostic:\n{}",
        run.output
    );
}

/// One engine run over a hermetic temp project: the pinned
/// `@verter/svelte-jsx` shim paths-mapped at its in-repo home, the vendored
/// hermetic `svelte` declarations inside the project, optional extra files
/// (a consumer, a self-import sidecar, the rune ambient), and a tsconfig
/// mirroring the live provider defaults (`jsx: preserve`, the project-level
/// `vue` import source the carrier pragma must override, `allowJs`/`checkJs`
/// per profile, optional declaration emit).
struct EngineRun {
    ok: bool,
    output: String,
    declaration_text: String,
}

fn run_engine(
    engine: &Engine,
    launcher: &Path,
    carrier: &CarrierSpec,
    extra_files: &[(&str, String)],
    declaration: bool,
) -> Option<EngineRun> {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    std::fs::write(root.join(&carrier.entry), &carrier.text).expect("write entry");
    for (rel, content) in extra_files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create extra parent");
        }
        std::fs::write(path, content).expect("write extra");
    }

    // Vendored hermetic `svelte` inside the temp project.
    let vendor_src = gate_fixture_dir().join("vendor_svelte");
    let vendor_dst = root.join("node_modules/svelte");
    std::fs::create_dir_all(&vendor_dst).expect("svelte vendor dir");
    for file in std::fs::read_dir(&vendor_src)
        .expect("vendor dir readable")
        .flatten()
    {
        if file.path().is_file() {
            std::fs::copy(file.path(), vendor_dst.join(file.file_name()))
                .expect("copy svelte vendor file");
        }
    }

    let shim = workspace_root()
        .join("packages/svelte-jsx")
        .to_string_lossy()
        .replace('\\', "/");
    let vendor = vendor_src.to_string_lossy().replace('\\', "/");
    let allow_js = carrier.entry.ends_with(".js")
        || carrier.entry.ends_with(".jsx")
        || carrier.check_js
        || extra_files
            .iter()
            .any(|(rel, _)| rel.ends_with(".js") || rel.ends_with(".jsx"));
    let js_opts = if allow_js {
        "\n    \"allowJs\": true,"
    } else {
        ""
    };
    let check_opts = if carrier.check_js {
        "\n    \"checkJs\": true,"
    } else {
        ""
    };
    let decl_opts = if declaration {
        "\n    \"declaration\": true,\n    \"emitDeclarationOnly\": true,\n    \"outDir\": \"types\","
    } else {
        "\n    \"noEmit\": true,"
    };
    let tsconfig = format!(
        r#"{{
  "compilerOptions": {{
    "module": "esnext",
    "target": "esnext",
    "moduleResolution": "bundler",
    "jsx": "preserve",
    "jsxImportSource": "vue",
    "strict": true,
    "skipLibCheck": true,
    "allowImportingTsExtensions": true,{js_opts}{check_opts}{decl_opts}
    "paths": {{
      "@verter/svelte-jsx/jsx-runtime": ["{shim}/jsx-runtime.d.ts"],
      "@verter/svelte-jsx/jsx-dev-runtime": ["{shim}/jsx-dev-runtime.d.ts"],
      "@verter/svelte-jsx/svg/jsx-runtime": ["{shim}/svg/jsx-runtime.d.ts"],
      "@verter/svelte-jsx/svg/jsx-dev-runtime": ["{shim}/svg/jsx-dev-runtime.d.ts"],
      "@verter/svelte-jsx/mathml/jsx-runtime": ["{shim}/mathml/jsx-runtime.d.ts"],
      "@verter/svelte-jsx/mathml/jsx-dev-runtime": ["{shim}/mathml/jsx-dev-runtime.d.ts"],
      "svelte": ["{vendor}/index.d.ts"],
      "svelte/elements": ["{vendor}/elements.d.ts"],
      "svelte/store": ["{vendor}/store.d.ts"],
      "svelte/transition": ["{vendor}/transition.d.ts"],
      "svelte/animate": ["{vendor}/animate.d.ts"],
      "svelte/attachments": ["{vendor}/attachments.d.ts"]
    }}
  }},
  "include": ["**/*.ts", "**/*.tsx", "**/*.js", "**/*.jsx"]
}}"#
    );
    std::fs::write(root.join("tsconfig.json"), tsconfig).expect("write tsconfig");

    let project = root.join("tsconfig.json");
    let output = if engine.through_node {
        Command::new("node")
            .arg(launcher)
            .arg("-p")
            .arg(&project)
            .current_dir(root)
            .output()
            .unwrap_or_else(|e| panic!("run {} through node: {e}", engine.label))
    } else {
        Command::new(launcher)
            .arg("-p")
            .arg(&project)
            .current_dir(root)
            .output()
            .unwrap_or_else(|e| panic!("run {} directly: {e}", engine.label))
    };
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Gather emitted declarations (publication assertions read them).
    let mut declaration_text = String::new();
    gather_declarations(&root.join("types"), &mut declaration_text);

    Some(EngineRun {
        ok: output.status.success(),
        output: combined,
        declaration_text,
    })
}

fn gather_declarations(dir: &Path, out: &mut String) {
    if !dir.is_dir() {
        return;
    }
    for entry in std::fs::read_dir(dir)
        .expect("types dir readable")
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            gather_declarations(&path, out);
        } else if path.extension().is_some_and(|e| e == "ts") {
            out.push_str(&std::fs::read_to_string(path).unwrap_or_default());
            out.push('\n');
        }
    }
}

/// Whether `text` carries `symbol` as a whole identifier (a renamed
/// `symbol__sts0_removed` must not satisfy a `symbol` publication claim).
fn names_symbol(text: &str, symbol: &str) -> bool {
    let mut from = 0usize;
    while let Some(found) = text[from..].find(symbol) {
        let start = from + found;
        let end = start + symbol.len();
        let before_ok = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '$');
        let after_ok = text[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '$');
        if before_ok && after_ok {
            return true;
        }
        from = start + symbol.len().max(1);
    }
    false
}

/// The host declaration carrier for a `.svelte` source (the owned
/// declaration publication path).
fn host_declaration(source: &str) -> String {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/Sts0Publication.svelte";
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::svelte(),
            aliases: Vec::new(),
        })
        .expect("upsert profile fixture");
    host.get_public_api_with_mode(canonical, PublicApiMode::Declaration, None)
        .expect("Svelte declaration projection")
        .expect("project Svelte declaration carrier")
        .ts_labeled_code()
        .to_string()
}

/// A `.svelte` evidence fixture may self-import its own module context
/// (`import { x } from "./itself.svelte"`); the projection keeps the
/// specifier, and it resolves against the component's own declaration — so
/// the declaration ships as the sidecar the import resolves to.
fn self_import_sidecar(row: &ProfileRow) -> Option<(&'static str, String)> {
    if row.file_kind != ".svelte" {
        return None;
    }
    let evidence = evidence_text(row);
    let own_base = Path::new(&row.evidence_path)
        .file_name()?
        .to_string_lossy()
        .to_string();
    let re = regex_lite(&own_base);
    if !re(&evidence) {
        return None;
    }
    let declaration = host_declaration(&evidence);
    let sidecar_name: &'static str = Box::leak(format!("{}.d.ts", own_base).into_boxed_str());
    Some((sidecar_name, declaration))
}

/// Minimal literal containment check for the self-import specifier
/// (`"./<basename>"`), avoiding a regex dependency in tests.
fn regex_lite(basename: &str) -> impl Fn(&str) -> bool + '_ {
    let double = format!("\"./{basename}\"");
    let single = format!("'./{basename}'");
    move |text: &str| text.contains(&double) || text.contains(&single)
}

#[test]
fn backend_projection_checks_clean_on_both_claimed_engines() {
    let engines = claimed_engines();
    if engines.is_empty() {
        skip_note("STS0 backend projection clean check");
        return;
    }
    for row in load_profile_rows() {
        let carrier = carrier_for(&row);
        let extras = sidecars_for(&row);
        for engine in &engines {
            let Some(run) = run_engine(engine, &engine.launcher, &carrier, &extras, false) else {
                return;
            };
            assert!(
                run.ok,
                "the owned-backend projection of profile {} must check clean on {}:\n{}\n--- carrier:\n{}",
                row.id,
                engine.label,
                run.output,
                carrier.text
            );
        }
    }
}

#[test]
fn evidence_template_corruption_is_caught_on_both_claimed_engines() {
    let engines = claimed_engines();
    if engines.is_empty() {
        skip_note("STS0 evidence template corruption check");
        return;
    }
    for row in load_profile_rows() {
        let evidence = evidence_text(&row);
        let corrupted_evidence = if row.file_kind == ".svelte" {
            format!("{evidence}{TEMPLATE_CORRUPTION}")
        } else {
            format!("{evidence}{}", type_corruption_for(&row.dialect))
        };
        // Re-derive the carrier from the corrupted evidence: the corruption
        // travels THROUGH the owned projection, never around it.
        let carrier = carrier_from_evidence(&row, corrupted_evidence);
        let extras = sidecars_for(&row);
        for engine in &engines {
            let Some(run) = run_engine(engine, &engine.launcher, &carrier, &extras, false) else {
                return;
            };
            if row.checking == "engine-checked" {
                assert_failed(&run, engine.label, &row.id, "evidence corruption");
            } else {
                assert!(
                    run.ok,
                    "js-unchecked profile {} must not report authored-JS corruption on {}:\n{}",
                    row.id, engine.label, run.output
                );
            }
        }
    }
}

#[test]
fn projected_carrier_corruption_is_caught_on_both_claimed_engines() {
    let engines = claimed_engines();
    if engines.is_empty() {
        skip_note("STS0 projected carrier corruption check");
        return;
    }
    for row in load_profile_rows() {
        if row.checking != "engine-checked" {
            continue; // unchecked surfaces must NOT report; the evidence twin covers them
        }
        let clean = carrier_for(&row);
        let corrupted = CarrierSpec {
            text: format!("{}{}", clean.text, type_corruption_for(&row.dialect)),
            ..clean.clone()
        };
        let extras = sidecars_for(&row);
        for engine in &engines {
            let Some(run) = run_engine(engine, &engine.launcher, &corrupted, &extras, false) else {
                return;
            };
            assert_failed(&run, engine.label, &row.id, "projected-carrier corruption");
        }
    }
}

fn sidecars_for(row: &ProfileRow) -> Vec<(&'static str, String)> {
    let mut extras = rune_ambient_extra(row);
    if let Some(sidecar) = self_import_sidecar(row) {
        extras.push(sidecar);
    }
    extras
}

#[test]
fn publication_surfaces_publish_and_survive_renames_on_both_claimed_engines() {
    let engines = claimed_engines();
    if engines.is_empty() {
        skip_note("STS0 publication surface check");
        return;
    }
    for row in load_profile_rows() {
        let first = row.published_symbols.first().cloned();
        match row.publishing.as_str() {
            "declarations-published" => {
                let evidence = evidence_text(&row);
                let declaration = host_declaration(&evidence);
                for symbol in &row.published_symbols {
                    assert!(
                        names_symbol(&declaration, symbol),
                        "the declaration carrier for profile {} must publish {symbol}:\n{declaration}",
                        row.id
                    );
                }
                // An actual declaration consumer on each claimed engine; the
                // @ts-expect-error discriminates a collapsed `any` surface.
                let carrier = carrier_for(&row);
                let consumer = format!(
                    "import Comp from './{}';\n\
                     import type {{ Component, ComponentProps }} from 'svelte';\n\
                     type Props = ComponentProps<typeof Comp>;\n\
                     type Exports = ReturnType<typeof Comp>;\n\
                     const asComponent: Component<Props, Exports, \"\"> = Comp;\n\
                     void asComponent;\n\
                     // @ts-expect-error the published component surface is concrete, not any\n\
                     const notAny: string = Comp;\n\
                     void notAny;\n",
                    carrier.entry
                );
                let extras = consumer_sidecars(&row, &consumer);
                for engine in &engines {
                    let Some(run) = run_engine(engine, &engine.launcher, &carrier, &extras, false)
                    else {
                        return;
                    };
                    assert!(
                        run.ok,
                        "the declaration consumer for profile {} must check on {}:\n{}\n--- declaration:\n{declaration}",
                        row.id,
                        engine.label,
                        run.output
                    );
                }
                if let Some(symbol) = first {
                    let renamed =
                        evidence.replace(symbol.as_str(), &format!("{symbol}__sts0_removed"));
                    let renamed_declaration = host_declaration(&renamed);
                    assert!(
                        !names_symbol(&renamed_declaration, &symbol),
                        "renaming published binding {symbol} must remove it from the declaration of profile {}:\n{renamed_declaration}",
                        row.id
                    );
                }
            }
            "module-exports-published" => {
                let carrier = carrier_for(&row);
                let extras = sidecars_for(&row);
                for symbol in &row.published_symbols {
                    assert!(
                        names_symbol(&carrier.text, symbol),
                        "the projected module surface of profile {} must keep export {symbol}",
                        row.id
                    );
                }
                for engine in &engines {
                    let Some(run) = run_engine(engine, &engine.launcher, &carrier, &extras, true)
                    else {
                        return;
                    };
                    assert!(
                        run.ok,
                        "the module surface of profile {} must check on {} for declaration emit:\n{}",
                        row.id,
                        engine.label,
                        run.output
                    );
                    for symbol in &row.published_symbols {
                        assert!(
                            names_symbol(&run.declaration_text, symbol),
                            "the emitted declarations for profile {} must publish {symbol} on {}:\n{}",
                            row.id,
                            engine.label,
                            run.declaration_text
                        );
                    }
                }
                if let Some(symbol) = first {
                    let renamed = evidence_text(&row)
                        .replace(symbol.as_str(), &format!("{symbol}__sts0_removed"));
                    let renamed_carrier = carrier_from_evidence(&row, renamed);
                    for engine in &engines {
                        let Some(run) =
                            run_engine(engine, &engine.launcher, &renamed_carrier, &extras, true)
                        else {
                            return;
                        };
                        assert!(
                            !names_symbol(&run.declaration_text, &symbol),
                            "renaming export {symbol} must remove it from the emitted declarations of profile {} on {}:\n{}",
                            row.id,
                            engine.label,
                            run.declaration_text
                        );
                    }
                }
            }
            other => panic!("profile {} claims unknown publishing {other}", row.id),
        }
    }
}

/// The declaration-consumer run reuses the carrier plus the consumer file
/// and any self-import sidecar the carrier's imports resolve against.
fn consumer_sidecars(row: &ProfileRow, consumer: &str) -> Vec<(&'static str, String)> {
    let mut extras = sidecars_for(row);
    extras.push(("consumer.ts", consumer.to_string()));
    extras
}
