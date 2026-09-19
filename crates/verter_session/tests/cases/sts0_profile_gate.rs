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
use verter_compiler::compile_request::svelte::SvelteRunesRequest;
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
    version: &'static str,
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
                version: expected,
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

/// The pinned platform package identity for the HOST platform:
/// (`@typescript/typescript-<platform>-<arch>`, `lib/tsc(.exe)`).
fn native_platform_package() -> (String, &'static str) {
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
    (
        format!("@typescript/typescript-{platform}-{arch}"),
        if cfg!(windows) { "tsc.exe" } else { "tsc" },
    )
}

/// Resolve the pinned native engine binary under `root`'s node_modules: the
/// hoisted platform package first, then the pnpm store entries
/// (`node_modules/.pnpm/@typescript+typescript-<platform>-<arch>@<version>/…`),
/// keeping the entry whose owning package matches the pinned version exactly.
/// The hoisted launcher is executed ONLY when its own package `version`
/// matches the pin — a stale hoist must never supersede the pinned store
/// binary (STS0-svelte-pin: the executed compiler is the recorded engine).
fn resolve_native_tsc_under(root: &Path, expected: &str) -> Option<(PathBuf, bool)> {
    let (pkg_name, exe) = native_platform_package();
    let hoisted_root = root.join(&pkg_name);
    let hoisted = hoisted_root.join("lib").join(exe);
    if hoisted.is_file() {
        match package_version(&hoisted_root) {
            Some(version) if version == expected => return Some((hoisted, false)),
            other => {
                eprintln!(
                    "STS0-ENGINE-PIN: refusing hoisted {pkg_name} at version {other:?} \
                     (pin is {expected}); using the pnpm store entry"
                );
            }
        }
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

fn resolve_native_tsc(expected: &str) -> Option<(PathBuf, bool)> {
    resolve_native_tsc_under(&workspace_root().join("node_modules"), expected)
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
    .inspect(|engine| {
        // Engine manifest for the STS0 protocol: the live acceptance gate
        // (tests/sfc-projection/STS0/protocol.mjs) requires BOTH pinned
        // engines' manifest lines in the cargo output, so a green run cannot
        // certify itself with an engine silently dropped.
        eprintln!("STS0-ENGINE {} {}", engine.label, engine.version);
    })
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
    semantics: String,
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
            semantics: row["semantics"].as_str().unwrap_or_default().to_string(),
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

fn runes_request(semantics: &str) -> Option<SvelteRunesRequest> {
    match semantics {
        "runes" => Some(SvelteRunesRequest::True),
        "legacy" => Some(SvelteRunesRequest::False),
        _ => Some(SvelteRunesRequest::Infer),
    }
}

fn ide_only_request(filename: &str, semantics: &str) -> CompileRequest {
    CompileRequest::new(
        vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest {
            runes: runes_request(semantics),
            ..Default::default()
        }),
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
fn project_through_owned_backend(source: &str, semantics: &str) -> (String, bool) {
    let artifact = svelte_artifact("file:///sts0-profile.svelte", source);
    let companion = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            source,
            &artifact,
            &ide_only_request("Sts0Profile.svelte", semantics),
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
        let (code, is_jsx) = project_through_owned_backend(&evidence, &row.semantics);
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
    "pretty": false,
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
            .arg("--pretty")
            .arg("false")
            .current_dir(root)
            .output()
            .unwrap_or_else(|e| panic!("run {} through node: {e}", engine.label))
    } else {
        Command::new(launcher)
            .arg("-p")
            .arg(&project)
            .arg("--pretty")
            .arg("false")
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

/// A publication-rename twin holds only when the renamed program still
/// checks *and* the old spelling is gone. An unrelated diagnostic that
/// emits no/partial declarations cannot satisfy the experiment.
fn publication_rename_holds(run: &EngineRun, old_symbol: &str) -> bool {
    run.ok && !names_symbol(&run.declaration_text, old_symbol)
}

#[test]
fn publication_rename_does_not_pass_on_unrelated_check_failure() {
    let unrelated = EngineRun {
        ok: false,
        output: "error TS2322: Type 'string' is not assignable to type 'number'.\n".to_string(),
        declaration_text: String::new(),
    };
    assert!(
        !publication_rename_holds(&unrelated, "moduleAnswer"),
        "an unrelated diagnostic that emits no declarations must not satisfy the publication-rename twin"
    );
    let renamed = EngineRun {
        ok: true,
        output: String::new(),
        declaration_text: "export declare const moduleAnswer__sts0_removed: number;\n".to_string(),
    };
    assert!(
        publication_rename_holds(&renamed, "moduleAnswer"),
        "a clean renamed program that dropped the old spelling must satisfy the publication-rename twin"
    );
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

/// Host-declaration pin: engine-free. Empty `publishedSymbols` never
/// reaches `names_symbol`, so legacy instance rows must pin the
/// declaration-visible prop names the carrier actually emits.
#[test]
fn pinned_declaration_symbols_appear_in_the_host_declaration_carrier() {
    for row in load_profile_rows() {
        if row.publishing != "declarations-published" {
            continue;
        }
        let evidence = evidence_text(&row);
        let declaration = host_declaration(&evidence);
        for symbol in &row.published_symbols {
            assert!(
                names_symbol(&declaration, symbol),
                "the declaration carrier for profile {} must publish {symbol}:\n{declaration}",
                row.id
            );
        }
        if let Some(symbol) = row.published_symbols.first().cloned() {
            let renamed = evidence.replace(symbol.as_str(), &format!("{symbol}__sts0_removed"));
            let renamed_declaration = host_declaration(&renamed);
            assert!(
                !names_symbol(&renamed_declaration, &symbol),
                "renaming published binding {symbol} must remove it from the declaration of profile {}:\n{renamed_declaration}",
                row.id
            );
        }
    }
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
                // An actual declaration consumer on each claimed engine: the
                // consumer imports the PUBLISHED DECLARATION sidecar, never
                // the IDE carrier, so a declaration regression cannot hide
                // behind a still-valid carrier. The pinned symbols must be
                // keys of the consumed props surface, and the
                // @ts-expect-error discriminates a collapsed `any` surface.
                let carrier = carrier_for(&row);
                let pinned = row
                    .published_symbols
                    .iter()
                    .map(|symbol| format!("\"{symbol}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                let consumer = format!(
                    "import Comp from './{DECLARATION_ENTRY_STEM}';\n\
                     import type {{ ComponentProps }} from 'svelte';\n\
                     type Props = ComponentProps<typeof Comp>;\n\
                     const pinnedProps: (keyof Props)[] = [{pinned}];\n\
                     void pinnedProps;\n\
                     // @ts-expect-error the published component surface is concrete, not any\n\
                     const notAny: string = Comp;\n\
                     void notAny;\n"
                );
                let extras = declaration_consumer_sidecars(&row, &consumer, &declaration);
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
                // DIRTY TWIN: collapse ONLY the published declaration's
                // default export to `any` — every pinned symbol spelling and
                // the IDE carrier stay byte-identical, so only the
                // declaration consumer can catch it.
                let corrupted = any_default_export_declaration(&declaration).unwrap_or_else(|| {
                    panic!(
                        "profile {} declaration must be a `declare const …; export default …` \
                         pair for the any-default twin:\n{declaration}",
                        row.id
                    )
                });
                for symbol in &row.published_symbols {
                    assert!(
                        names_symbol(&corrupted, symbol),
                        "the any-default twin for profile {} must preserve the {symbol} spelling:\n{corrupted}",
                        row.id
                    );
                }
                let twin_extras = declaration_consumer_sidecars(&row, &consumer, &corrupted);
                for engine in &engines {
                    let Some(run) =
                        run_engine(engine, &engine.launcher, &carrier, &twin_extras, false)
                    else {
                        return;
                    };
                    assert_failed(&run, engine.label, &row.id, "any-default declaration");
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
                            publication_rename_holds(&run, &symbol),
                            "renaming export {symbol} must still check and drop the old spelling from the emitted declarations of profile {} on {}:\n{}\n--- declarations:\n{}",
                            row.id,
                            engine.label,
                            run.output,
                            run.declaration_text
                        );
                    }
                }
            }
            other => panic!("profile {} claims unknown publishing {other}", row.id),
        }
    }
}

/// The carrier-and-consumer extras shared by every engine run in the
/// publication test: any self-import sidecar the carrier's imports resolve
/// against, plus the consumer file itself.
fn consumer_sidecars(row: &ProfileRow, consumer: &str) -> Vec<(&'static str, String)> {
    let mut extras = sidecars_for(row);
    extras.push(("consumer.ts", consumer.to_string()));
    extras
}

/// The declaration-consumer run reuses the carrier plus the consumer file,
/// the actual host DECLARATION as the module the consumer imports, and any
/// self-import sidecar the carrier's imports resolve against.
fn declaration_consumer_sidecars(
    row: &ProfileRow,
    consumer: &str,
    declaration: &str,
) -> Vec<(&'static str, String)> {
    let mut extras = consumer_sidecars(row, consumer);
    extras.push((DECLARATION_ENTRY, declaration.to_string()));
    extras
}

/// The sidecar file the declaration consumer imports: the PUBLISHED host
/// declaration (a strictly-valid `.d.ts`), not the IDE carrier. The consumer
/// imports the extensionless stem; TS resolves it to this `.d.ts`.
const DECLARATION_ENTRY: &str = "sts0-host-declaration.d.ts";
const DECLARATION_ENTRY_STEM: &str = "sts0-host-declaration";

/// Collapse ONLY the default export's type to `any`: the component const
/// binding that `export default` names has its type intersected with `any`
/// (`T & any` is `any` to the checker), so every spelling — including the
/// prop names inside the `Component<…>` generics — is preserved
/// byte-for-byte. `None` when the declaration is not the expected
/// `declare const …; export default …;` pair — the twin must break loudly
/// rather than corrupt nothing.
fn any_default_export_declaration(declaration: &str) -> Option<String> {
    let marker = "\nexport default ";
    let export_at = declaration.rfind(marker)?;
    let name_start = export_at + marker.len();
    let name_end = name_start + declaration[name_start..].find(';')?;
    let name = &declaration[name_start..name_end];
    let head = format!("declare const {name}:");
    let head_at = declaration.find(&head)?;
    // The type extends to the first statement-terminating `;` at bracket
    // depth zero (both the `import("svelte").Component<…>` spelling and the
    // generic `{ … }` spelling nest, so a plain find would cut early).
    let mut depth = 0usize;
    let mut end = None;
    let mut prev = '\0';
    for (offset, ch) in declaration[head_at..].char_indices() {
        match ch {
            '<' | '{' | '(' | '[' => depth += 1,
            // `=>` carries a `>` that closes no `<`; counting it desyncs
            // the depth and terminates the type inside a generic argument.
            '>' if prev == '=' => {}
            '>' | '}' | ')' | ']' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => {
                end = Some(head_at + offset);
                break;
            }
            _ => {}
        }
        prev = ch;
    }
    let end = end?;
    Some(format!(
        "{}& any;{}",
        &declaration[..end],
        &declaration[end + 1..]
    ))
}

#[test]
fn any_default_export_declaration_does_not_cut_inside_arrow_types() {
    let declaration = concat!(
        "declare const C: import(\"svelte\").Component<",
        "{ a: (x: number) => void; b: (y: number) => void; c: string }",
        ">;\nexport default C;\n",
    );
    let corrupted = any_default_export_declaration(declaration)
        .expect("the Component<{…}> declaration must splice");
    assert!(
        corrupted.contains("c: string }>& any;"),
        "=> must not decrement depth or the splice lands inside the object type:\n{corrupted}"
    );
    assert!(
        !corrupted.contains("void& any"),
        "the any splice must not land on an arrow return:\n{corrupted}"
    );
}

// --- STS0-svelte-pin: the executed native launcher is the pinned one ------

/// A synthetic hoisted platform package under `root` at `version`.
fn fake_hoisted_native_package(root: &Path, version: &str) {
    let (pkg_name, exe) = native_platform_package();
    let dir = root.join(&pkg_name);
    std::fs::create_dir_all(dir.join("lib")).expect("fake package lib dir");
    std::fs::write(
        dir.join("package.json"),
        format!("{{\"name\":\"{pkg_name}\",\"version\":\"{version}\"}}"),
    )
    .expect("fake package.json");
    std::fs::write(dir.join("lib").join(exe), "stub launcher").expect("fake launcher");
}

/// A synthetic pnpm store entry (`.pnpm/<scoped-name>@<version>/…`) at
/// `version`.
fn fake_native_store_entry(root: &Path, version: &str) {
    let (pkg_name, exe) = native_platform_package();
    let entry = root
        .join(".pnpm")
        .join(format!("{}@{}", pkg_name.replace('/', "+"), version))
        .join("node_modules")
        .join(&pkg_name);
    std::fs::create_dir_all(entry.join("lib")).expect("fake store lib dir");
    std::fs::write(
        entry.join("package.json"),
        format!("{{\"name\":\"{pkg_name}\",\"version\":\"{version}\"}}"),
    )
    .expect("fake store package.json");
    std::fs::write(entry.join("lib").join(exe), "stub launcher").expect("fake store launcher");
}

#[test]
fn a_mismatched_hoisted_native_package_is_never_selected() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    fake_hoisted_native_package(root, "0.0.0");
    fake_native_store_entry(root, "7.0.2");
    let (launcher, _) =
        resolve_native_tsc_under(root, "7.0.2").expect("the pinned store entry resolves");
    assert!(
        launcher.starts_with(root.join(".pnpm")),
        "a mismatched hoisted platform package must never supersede the pinned store binary: {}",
        launcher.display()
    );
}

#[test]
fn a_matching_hoisted_native_package_is_admitted() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    fake_hoisted_native_package(root, "7.0.2");
    let (launcher, _) =
        resolve_native_tsc_under(root, "7.0.2").expect("the matching hoisted package resolves");
    let (pkg_name, exe) = native_platform_package();
    assert_eq!(launcher, root.join(pkg_name).join("lib").join(exe));
}

#[test]
fn an_unpinned_hoisted_native_package_alone_is_refused() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    fake_hoisted_native_package(root, "0.0.0");
    assert!(
        resolve_native_tsc_under(root, "7.0.2").is_none(),
        "a version-mismatched hoisted launcher with no pinned store entry must not resolve"
    );
}
