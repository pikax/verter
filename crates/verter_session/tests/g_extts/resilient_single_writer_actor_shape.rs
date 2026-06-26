//! Guard: `resilient_single_writer_actor_shape`.
//!
//! The resilient provider's desired editor state (the open/loaded file set, path
//! configs, workspace folders, and carrier registrations) is owned by ONE
//! single-writer actor task — task-local, never behind a shared lock. The retired
//! design stored that state in shared lock-guarded maps and replayed from a
//! crash-time SNAPSHOT-then-SWAP, which has a TOCTOU window: a close/retract/edit
//! racing the snapshot could replay a removed carrier or stale bytes. This static
//! guard AST-parses `resilient.rs` and FAILS if that shape returns:
//!
//! 1. `DesiredState`'s state fields (`files`, `path_configs`, `workspace_folders`,
//!    `carrier_registrations`) must be PLAIN owned collections (`HashMap`/`Vec`) —
//!    never `Arc`/`Mutex`/`RwLock`-wrapped.
//! 2. NO struct field anywhere in `resilient.rs` may be a lock (`Mutex`/`RwLock`)
//!    wrapping a desired-state collection or the `DesiredState`/`CachedCarrier`/
//!    `CachedFile` types (the shared-lock-guarded-map prerequisite for
//!    snapshot-then-swap). The live-provider cell `Arc<RwLock<Option<Arc<P>>>>` is
//!    ALLOWED — it wraps an `Option`, not a desired-state collection.
//! 3. The single-writer actor structure is present: an `enum Command` with a
//!    `GoLive` variant and an `async fn run_actor`.
//! 4. `#![deny(clippy::await_holding_lock)]` is in force for BOTH
//!    `verter_type_runtime` and `verter_lsp` (no synchronous guard may be held
//!    across an `.await` on the replay / membership paths).
//!
//! AST type-shape checks (not a text scan) make the guard robust to whitespace, a
//! `use … as Lock` alias on the wrapper, or `Mutex` vs `RwLock` swaps.
//!
//! DISCRIMINATING: [`actor_shape_self_test_discriminates`] proves the field-shape
//! check FIRES on a synthetic `carrier_registrations: Arc<RwLock<HashMap<…>>>`
//! field and on a `Arc<Mutex<DesiredState>>` field, and ACCEPTS the real
//! task-local shape + the legitimate `Arc<RwLock<Option<…>>>` live cell.

use std::path::PathBuf;

use quote::ToTokens;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_workspace_file(rel: &str) -> String {
    std::fs::read_to_string(workspace_root().join(rel))
        .unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

const RESILIENT_RS: &str = "crates/verter_type_runtime/src/resilient.rs";
const RUNTIME_LIB_RS: &str = "crates/verter_type_runtime/src/lib.rs";
const LSP_LIB_RS: &str = "crates/verter_lsp/src/lib.rs";

/// The `DesiredState` fields that must stay plain owned collections.
const DESIRED_STATE_FIELDS: &[&str] = &[
    "files",
    "path_configs",
    "workspace_folders",
    "carrier_registrations",
];

/// Types whose wrapping in a `Mutex`/`RwLock` is the snapshot-then-swap shape.
const FORBIDDEN_LOCKED: &[&str] = &[
    "HashMap",
    "Vec",
    "DesiredState",
    "CachedCarrier",
    "CachedFile",
];

/// The outermost path-segment ident of a `Type::Path` (`Arc<…>` → `"Arc"`).
fn outer_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// The first generic TYPE argument of a `Type::Path`'s last segment
/// (`Arc<RwLock<X>>`'s `Arc` segment → `RwLock<X>`).
fn first_type_arg(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    })
}

/// Peel a leading `Arc<…>` (`Arc<RwLock<X>>` → `RwLock<X>`); other types pass
/// through unchanged.
fn peel_arc(ty: &syn::Type) -> &syn::Type {
    if outer_ident(ty).as_deref() == Some("Arc") {
        first_type_arg(ty).unwrap_or(ty)
    } else {
        ty
    }
}

/// If `ty` (after peeling `Arc`) is a lock (`Mutex`/`RwLock`), the wrapped type.
fn lock_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let peeled = peel_arc(ty);
    match outer_ident(peeled).as_deref() {
        Some("Mutex") | Some("RwLock") => first_type_arg(peeled),
        _ => None,
    }
}

/// Every `(struct_name, field_name, field_type)` in the parsed file.
fn struct_fields(file: &syn::File) -> Vec<(String, String, syn::Type)> {
    let mut out = Vec::new();
    for item in &file.items {
        let syn::Item::Struct(s) = item else { continue };
        let syn::Fields::Named(named) = &s.fields else {
            continue;
        };
        for field in &named.named {
            if let Some(ident) = &field.ident {
                out.push((s.ident.to_string(), ident.to_string(), field.ty.clone()));
            }
        }
    }
    out
}

/// Run every shape check over `resilient.rs`'s parsed AST, returning violations.
fn actor_shape_violations(file: &syn::File) -> Vec<String> {
    let mut v = Vec::new();
    let fields = struct_fields(file);

    // RULE 1: DesiredState's state fields are plain owned collections.
    let mut seen_desired_fields = 0usize;
    for (struct_name, field_name, ty) in &fields {
        if struct_name != "DesiredState" {
            continue;
        }
        if DESIRED_STATE_FIELDS.contains(&field_name.as_str()) {
            seen_desired_fields += 1;
            let outer = outer_ident(ty).unwrap_or_default();
            if !matches!(outer.as_str(), "HashMap" | "Vec") {
                v.push(format!(
                    "DesiredState::{field_name} must be a PLAIN owned collection (HashMap/Vec), not \
                     `{}` — a lock/Arc-wrapped desired-state map is the snapshot-then-swap shape",
                    ty.to_token_stream()
                ));
            }
        }
    }
    if seen_desired_fields != DESIRED_STATE_FIELDS.len() {
        v.push(format!(
            "expected DesiredState to carry all {} task-local state fields ({DESIRED_STATE_FIELDS:?}); \
             found {seen_desired_fields} — the actor-owned desired-state set is missing/renamed",
            DESIRED_STATE_FIELDS.len()
        ));
    }

    // RULE 2: no field anywhere is a lock wrapping a desired-state collection /
    // the desired-state types. The `Arc<RwLock<Option<Arc<P>>>>` live cell is fine
    // (its lock wraps an `Option`).
    for (struct_name, field_name, ty) in &fields {
        if let Some(inner) = lock_inner(ty) {
            let inner_outer = outer_ident(inner).unwrap_or_default();
            if FORBIDDEN_LOCKED.contains(&inner_outer.as_str()) {
                v.push(format!(
                    "{struct_name}::{field_name} is a shared lock wrapping `{inner_outer}` \
                     (`{}`) — desired editor state must be task-local to the single-writer actor, \
                     not a lock-guarded map a crash-time snapshot-then-swap would replay from",
                    ty.to_token_stream()
                ));
            }
        }
    }

    // RULE 3: the single-writer actor structure is present.
    let has_command_enum = file
        .items
        .iter()
        .any(|item| matches!(item, syn::Item::Enum(en) if en.ident == "Command"));
    let has_golive = file.items.iter().any(|item| match item {
        syn::Item::Enum(en) if en.ident == "Command" => {
            en.variants.iter().any(|var| var.ident == "GoLive")
        }
        _ => false,
    });
    let has_run_actor = file.items.iter().any(|item| match item {
        syn::Item::Fn(f) => f.sig.ident == "run_actor" && f.sig.asyncness.is_some(),
        _ => false,
    });
    if !has_command_enum {
        v.push("the single-writer `enum Command` is missing from resilient.rs".to_string());
    }
    if !has_golive {
        v.push("the `Command::GoLive` replay-then-install variant is missing".to_string());
    }
    if !has_run_actor {
        v.push("the `async fn run_actor` single-writer loop is missing".to_string());
    }

    v
}

/// True iff `file`'s inner attributes deny `clippy::await_holding_lock`.
fn denies_await_holding_lock(file: &syn::File) -> bool {
    file.attrs.iter().any(|attr| {
        attr.path().is_ident("deny")
            && attr
                .meta
                .to_token_stream()
                .to_string()
                .contains("await_holding_lock")
    })
}

fn parse(rel: &str) -> syn::File {
    syn::parse_file(&read_workspace_file(rel)).unwrap_or_else(|e| panic!("{rel} must parse: {e}"))
}

#[test]
fn resilient_single_writer_actor_shape() {
    let violations = actor_shape_violations(&parse(RESILIENT_RS));
    assert!(
        violations.is_empty(),
        "resilient.rs single-writer actor shape regressed (snapshot-then-swap prevention):\n  - {}",
        violations.join("\n  - ")
    );
}

#[test]
fn await_holding_lock_denied_on_runtime_and_lsp() {
    for rel in [RUNTIME_LIB_RS, LSP_LIB_RS] {
        assert!(
            denies_await_holding_lock(&parse(rel)),
            "{rel} must carry `#![deny(clippy::await_holding_lock)]` — no synchronous guard may be \
             held across an `.await` on the replay / membership paths"
        );
    }
}

/// DISCRIMINATING self-test: the field-shape checks fire on the retired
/// snapshot-then-swap shapes and accept the real task-local shape.
#[test]
fn actor_shape_self_test_discriminates() {
    // The REAL shape (task-local maps + the Arc<RwLock<Option<…>>> live cell) is
    // CLEAN.
    let real = syn::parse_file(
        "struct DesiredState {\n\
           files: HashMap<String, CachedFile>,\n\
           path_configs: Vec<CachedPathConfig>,\n\
           workspace_folders: Vec<serde_json::Value>,\n\
           carrier_registrations: HashMap<String, CachedCarrier>,\n\
         }\n\
         struct ResilientState { inner: Arc<RwLock<Option<Arc<P>>>> }\n\
         enum Command { GoLive { provider: Arc<P> } }\n\
         async fn run_actor() {}",
    )
    .unwrap();
    assert!(
        actor_shape_violations(&real).is_empty(),
        "the real task-local actor shape (incl. the Arc<RwLock<Option<…>>> live cell) must be clean"
    );

    // A snapshot-then-swap field shape — a shared lock-guarded carrier map — FIRES
    // on BOTH the DesiredState-field rule and the global lock-wrapping rule.
    let locked_map = syn::parse_file(
        "struct DesiredState {\n\
           files: HashMap<String, CachedFile>,\n\
           path_configs: Vec<CachedPathConfig>,\n\
           workspace_folders: Vec<serde_json::Value>,\n\
           carrier_registrations: Arc<RwLock<HashMap<String, CachedCarrier>>>,\n\
         }",
    )
    .unwrap();
    let hits = actor_shape_violations(&locked_map);
    assert!(
        hits.iter()
            .any(|m| m.contains("carrier_registrations") && m.contains("snapshot-then-swap")),
        "a lock-wrapped carrier_registrations map must trip the field-shape rule; got {hits:?}"
    );
    assert!(
        hits.iter()
            .any(|m| m.contains("shared lock wrapping `HashMap`")),
        "a lock-wrapped carrier map must also trip the global lock-wrapping rule; got {hits:?}"
    );

    // A lock wrapping the whole DesiredState FIRES the global rule (the
    // collection-only check would miss it).
    let locked_state = syn::parse_file("struct W { state: Arc<Mutex<DesiredState>> }").unwrap();
    assert!(
        actor_shape_violations(&locked_state)
            .iter()
            .any(|m| m.contains("shared lock wrapping `DesiredState`")),
        "a lock wrapping the whole DesiredState must trip the global lock-wrapping rule"
    );

    // The live-provider cell `Arc<RwLock<Option<…>>>` is NOT a violation (its lock
    // wraps an Option, not a desired-state collection).
    let live_cell = syn::parse_file("struct S { inner: Arc<RwLock<Option<Arc<P>>>> }").unwrap();
    assert!(
        !actor_shape_violations(&live_cell)
            .iter()
            .any(|m| m.contains("shared lock wrapping")),
        "the legitimate Arc<RwLock<Option<…>>> live cell must NOT trip the lock-wrapping rule"
    );

    // The await_holding_lock detector discriminates.
    assert!(denies_await_holding_lock(
        &syn::parse_file("#![deny(clippy::await_holding_lock)]").unwrap()
    ));
    assert!(denies_await_holding_lock(
        &syn::parse_file("#![deny(clippy::all, clippy::await_holding_lock)]").unwrap()
    ));
    assert!(!denies_await_holding_lock(
        &syn::parse_file("#![deny(clippy::all)]").unwrap()
    ));
}
