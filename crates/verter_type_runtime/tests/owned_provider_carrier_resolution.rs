//! LIVE, NON-VACUOUS proof that the OWNED dual-surface tsgo provider resolves a
//! bare carrier import (`import B from "./B.vue"`) to the **declaration carrier**
//! (`B.d.vue.ts`) through tsgo's own module resolver — WITHOUT any provider-side
//! import string-rewriting.
//!
//! These tests characterize the resolution mechanism the OWNED tsgo path relies
//! on in production:
//!  - the compiler emits the BARE `./B.vue` specifier for in-project carrier
//!    imports (the in-project specifier is not suffixed);
//!  - tsgo (bundler `moduleResolution`) strips the `.vue` carrier extension and
//!    probes `B.d.vue.ts` -> `B.vue.ts` -> `B.vue.tsx`, landing on the didOpened
//!    DECLARATION carrier (`.d.vue.ts` wins the probe — it is the proactively
//!    emitted + opened declaration carrier, gate-locked by
//!    `component_bare_import_resolves_to_declaration_carrier`);
//!  - the declaration carrier re-exports the component's public type from its
//!    `.verter.ts` public-API surface, so a consumer sees the real member type.
//!
//! A BARE `./B.vue` specifier (the form the deleted provider-side rewriter would
//! have targeted) resolves to `B.d.vue.ts` by tsgo's native probe — so the
//! provider-side rewriter is not load-bearing on this path.
//!
//! NON-VACUOUS: drives a real tsgo. `semantic_diagnostics_for_carrier` is empty
//! for a non-member, so a bare "no diagnostics" assertion would be vacuous; each
//! positive assertion checks a REAL type-flow result (a deliberate TS2322 proving
//! the imported public member type was consumed). Under `VERTER_REQUIRE_TSGO` a
//! missing engine is a HARD failure (a skip would be a vacuous pass).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use verter_type_runtime::traits::TypeProvider;
use verter_type_runtime::tsgo::ipc::find_tsgo_binary_canonical;
use verter_type_runtime::tsgo::{TsgoOwnedProvider, TsgoTypeProvider};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crates/")
        .to_path_buf()
}

/// Discover the engine through the canonical discovery (honors `VERTER_TSGO_BIN`
/// first, then the workspace `node_modules`, then the npm/npx cache). Honors
/// `VERTER_REQUIRE_TSGO` (a skip under that env is a vacuous-pass failure).
fn engine_or_skip() -> Option<PathBuf> {
    match find_tsgo_binary_canonical(Some(&workspace_root())) {
        Ok(p) => Some(PathBuf::from(p)),
        Err(e) => {
            if std::env::var("VERTER_REQUIRE_TSGO").is_ok() {
                panic!(
                    "VERTER_REQUIRE_TSGO is set but tsgo was not found: {e}. \
                     A skip would be a vacuous pass."
                );
            }
            eprintln!("[skip] tsgo engine not found ({e}); set VERTER_REQUIRE_TSGO to require it");
            None
        }
    }
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn tempdir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "verter_owned_carrier_res_{}_{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// A configured project (`moduleResolution: Bundler`) whose `src/**/*` `include`
/// makes the on-disk consumer a member. The carrier companions are off-disk
/// overlays opened through the provider.
fn write_fixture(dir: &Path) -> PathBuf {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let tsconfig = dir.join("tsconfig.json");
    std::fs::write(
        &tsconfig,
        r#"{
  "compilerOptions": {
    "strict": true,
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "jsx": "preserve",
    "noEmit": true,
    "skipLibCheck": true,
    "allowArbitraryExtensions": true
  },
  "include": ["src/**/*"]
}
"#,
    )
    .unwrap();
    tsconfig
}

async fn build_owned_provider(exe: &Path, dir: &Path, tsconfig: &Path) -> TsgoOwnedProvider {
    let root_uri = format!("file:///{}", slash(dir).trim_start_matches('/'));
    let lsp = TsgoTypeProvider::spawn(&exe.to_string_lossy(), &root_uri)
        .await
        .expect("spawn tsgo --lsp");
    TsgoOwnedProvider::attach(Arc::new(lsp), slash(tsconfig), exe)
        .await
        .expect("attach --api checker (one process)")
}

/// The component `B`'s PUBLIC-API surface (`.verter.ts`): a real public member
/// type the consumer must see. This is the redirect-reached API carrier.
const B_VERTER_TS: &str = "export interface BProps { label: string; count: number }\n\
     declare const _default: new () => { $props: BProps };\n\
     export default _default;\n";

/// The component `B`'s DECLARATION carrier (`.d.vue.ts`): the
/// bare-import-probe-reachable resolution target (tsgo's probe reaches it FIRST,
/// before `.vue.ts`/`.vue.tsx`). It re-exports the public default + props type
/// from the `.verter.ts` API surface (mirroring the compiler's declaration-carrier
/// scaffolding), so a consumer resolving the bare `./B.vue` import sees the
/// component's public type.
const B_D_VUE_TS: &str = "import _default from \"./B.vue.verter.ts\";\n\
     export type { BProps } from \"./B.vue.verter.ts\";\n\
     export default _default;\n";

/// Open `B`'s companions (API surface + declaration carrier) as off-disk overlays.
async fn open_b_companions(provider: &TsgoOwnedProvider, src_dir: &Path) {
    let verter_path = slash(&src_dir.join("B.vue.verter.ts"));
    let decl_path = slash(&src_dir.join("B.d.vue.ts"));
    provider
        .open_file(&verter_path, B_VERTER_TS)
        .await
        .expect("open B.vue.verter.ts");
    provider
        .open_file(&decl_path, B_D_VUE_TS)
        .await
        .expect("open B.d.vue.ts (declaration carrier)");
}

/// D4 (positive, NET-NEW): a consumer with a BARE `import ... from "./B.vue"`
/// resolves to the didOpened DECLARATION carrier `B.d.vue.ts` and the real public
/// member type `BProps` flows — proven NON-VACUOUSLY by a deliberate TS2322 on a
/// wrong member assignment (an empty/wrong-project result would NOT surface 2322).
///
/// This characterizes that resolution works through tsgo's native carrier probe
/// WITHOUT any provider-side import rewriter: the consumer specifier is bare
/// `./B.vue` (the exact form the deleted rewriter targeted), yet it resolves to
/// the `.d.vue.ts` declaration carrier (which wins tsgo's basename-append probe).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owned_bare_vue_import_resolves_to_declaration_carrier_and_public_member_flows() {
    let Some(exe) = engine_or_skip() else {
        return;
    };
    let dir = tempdir();
    let tsconfig = write_fixture(&dir);
    let src = dir.join("src");
    let provider = build_owned_provider(&exe, &dir, &tsconfig).await;

    // The consumer is a real on-disk `.ts` member of the configured project. Its
    // import specifier is BARE `./B.vue` (no `.tsx`/`.verter.ts` suffix) — the
    // form the deleted provider-side rewriter would have touched. tsgo must
    // resolve it natively to the didOpened `B.d.vue.ts` declaration carrier. A
    // deliberate TS2322 (string -> the `count: number` member) is the non-vacuous
    // proof the public `BProps` type was consumed through
    // `./B.vue` -> `B.d.vue.ts` -> `.verter.ts`.
    let consumer_path = slash(&src.join("Consumer.ts"));
    let consumer_src = "import B from \"./B.vue\";\n\
         import type { BProps } from \"./B.vue\";\n\
         export const used = B;\n\
         const props: BProps = { label: \"x\", count: \"definitely not a number\" };\n\
         export const out = props;\n";
    std::fs::write(src.join("Consumer.ts"), consumer_src).expect("write consumer");

    open_b_companions(&provider, &src).await;
    provider
        .open_file(&consumer_path, consumer_src)
        .await
        .expect("open consumer");

    let diags = tokio::time::timeout(
        Duration::from_secs(30),
        provider.semantic_diagnostics_for_carrier(&consumer_path),
    )
    .await
    .expect("--api semantic diagnostics timed out")
    .expect("--api semantic diagnostics");

    // NON-VACUOUS: the deliberate TS2322 on `count: "..."` proves the real public
    // member type `BProps` (with `count: number`) flowed from the resolved carrier.
    // An unresolved import (TS2307) or empty/wrong-project result would NOT produce
    // a 2322 on the `count` member.
    assert!(
        diags.iter().any(|d| d.code.as_deref() == Some("2322")),
        "the public member type `BProps.count: number` must flow through the bare \
         `./B.vue` -> `B.d.vue.ts` declaration-carrier resolution (deliberate TS2322 on \
         the string assignment proves tsgo consumed the carrier's public type); got: {diags:?}"
    );
    // And the import itself resolved — no TS2307 (carrier-not-found).
    assert!(
        !diags.iter().any(|d| d.code.as_deref() == Some("2307")),
        "the bare `./B.vue` import must RESOLVE to the didOpened declaration carrier \
         (no TS2307); got: {diags:?}"
    );

    provider.shutdown().await.expect("shutdown");
    let _ = std::fs::remove_dir_all(&dir);
}

/// D5 (negative discriminator, NET-NEW): with the DECLARATION-carrier didOpen
/// SUPPRESSED (only the `.verter.ts` API surface opened, NOT `B.d.vue.ts`), the
/// bare `import ... from "./B.vue"` must FAIL with TS2307 — tsgo's probe chain
/// (`B.d.vue.ts` -> `B.vue.ts` -> `B.vue.tsx`) never reaches the `.verter.ts`
/// surface, so without the declaration carrier the import is unresolved.
///
/// This proves the resolution depends on the declaration-carrier didOpen (the
/// real mechanism), NOT on any provider-side rewriter: suppressing the carrier
/// genuinely breaks resolution; opening it (D4) fixes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owned_bare_vue_import_fails_closed_when_declaration_carrier_didopen_suppressed() {
    let Some(exe) = engine_or_skip() else {
        return;
    };
    let dir = tempdir();
    let tsconfig = write_fixture(&dir);
    let src = dir.join("src");
    let provider = build_owned_provider(&exe, &dir, &tsconfig).await;

    let consumer_path = slash(&src.join("Consumer.ts"));
    let consumer_src = "import B from \"./B.vue\";\n\
         export const used = B;\n";
    std::fs::write(src.join("Consumer.ts"), consumer_src).expect("write consumer");

    // SUPPRESS the declaration-carrier didOpen: open ONLY the `.verter.ts` API
    // surface, NOT `B.d.vue.ts`. tsgo's bare `./B.vue` probe never reaches
    // `.verter.ts`.
    let verter_path = slash(&src.join("B.vue.verter.ts"));
    provider
        .open_file(&verter_path, B_VERTER_TS)
        .await
        .expect("open B.vue.verter.ts");
    provider
        .open_file(&consumer_path, consumer_src)
        .await
        .expect("open consumer");

    let diags = tokio::time::timeout(
        Duration::from_secs(30),
        provider.semantic_diagnostics_for_carrier(&consumer_path),
    )
    .await
    .expect("--api semantic diagnostics timed out")
    .expect("--api semantic diagnostics");

    // With the declaration carrier suppressed, the bare `./B.vue` import is
    // unresolved: TS2307 (cannot find module). This is the fail-closed proof —
    // and the RED half of the discriminator vs D4 (carrier present -> resolves).
    assert!(
        diags.iter().any(|d| d.code.as_deref() == Some("2307")),
        "with the declaration-carrier `B.d.vue.ts` didOpen SUPPRESSED, the bare \
         `./B.vue` import must FAIL CLOSED with TS2307 (tsgo's probe never reaches \
         the `.verter.ts` API surface); got: {diags:?}"
    );

    provider.shutdown().await.expect("shutdown");
    let _ = std::fs::remove_dir_all(&dir);
}
