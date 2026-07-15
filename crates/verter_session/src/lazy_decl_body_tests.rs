//! Demand-scoped declaration-body lowering contract.
//!
//! `IndexedReady` is a shallow declaration index: publishing a file's
//! artifact lowers ZERO declaration bodies; a semantic query lowers
//! exactly the declaration closure it actually walks, through the
//! shared lazy body service; concurrent first-touch of one symbol
//! lowers it once; a content edit invalidates the body memo; an
//! overlay body never serves a base read; fact emission at publish
//! computes no body-derived hashes.
//!
//! Counters are host-owned `MetaProvenance` atomics — deterministic
//! observability, no wall-clock. `decl_bodies_lowered` increments once
//! per declaration contributor whose body is lowered to typed IR on
//! behalf of this host.

use std::sync::Arc;

use crate::semantic_query::ProjectionMode;
use crate::types::{HostConfig, MetaProvenanceSnapshot, UpsertRequest};
use crate::VerterHost;

fn make_host() -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(canonical_id)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

fn snap(host: &VerterHost) -> MetaProvenanceSnapshot {
    host.provenance().snapshot()
}

/// Miniature `slow.ts` shape: ONE requested symbol plus INDEPENDENT
/// filler decls (each body references nothing) the resolve must never
/// lower. 5 declaration bodies total.
const SCRATCH: &str = "export type Unrelated = { a: 1 };\n\
     type Var0 = { v: 0 };\n\
     type Var1 = { v: 1 };\n\
     type Var2 = { v: 2 };\n\
     type Var3 = { v: 3 };\n\
     export const ValueUnrelated: { a: 1 } = { a: 1 };\n\
     const Val0: { v: 0 } = { v: 0 };\n\
     const Val1: { v: 1 } = { v: 1 };\n\
     const Val2: { v: 2 } = { v: 2 };\n\
     const Val3: { v: 3 } = { v: 3 };\n";
const SCRATCH_DECLS: u64 = 10;
const SCRATCH_ID: &str = "/workspace/src/scratch.ts";

/// Dependency-chain fixture: `A → B → C` plus two unreachable decls.
/// The walked closure of `A` under `Expanded` is exactly {A, B, C}.
const CHAIN: &str = "export type A = B & { a: 1 };\n\
     type B = C;\n\
     type C = { c: 1 };\n\
     type D = { d: 1 };\n\
     type E = { e: 1 };\n";
const CHAIN_ID: &str = "/workspace/src/chain.ts";

/// Publishing the canonical post-parse artifact is INDEX work: imports,
/// exports, symbol names/kinds/spans, member headers. It must lower
/// zero declaration bodies — bodies lower on first semantic demand.
#[test]
fn indexed_ready_publish_lowers_zero_decl_bodies() {
    let host = make_host();
    upsert(&host, SCRATCH_ID, SCRATCH);
    host.provenance().reset();

    let indexed = host
        .ensure_indexed_ready(SCRATCH_ID)
        .expect("artifact must materialise");
    assert!(
        indexed.shallow_state.has_type_symbol("Unrelated"),
        "the published index must inventory the Unrelated symbol"
    );
    assert!(
        indexed.shallow_state.has_type_symbol("Var3"),
        "the published index must inventory every top-level symbol"
    );
    assert!(
        indexed.shallow_state.has_value_symbol("ValueUnrelated"),
        "the published index must inventory exported value symbols"
    );
    assert!(
        indexed.shallow_state.has_value_symbol("Val3"),
        "the published index must inventory every top-level value symbol"
    );

    let provenance = snap(&host);
    assert_eq!(
        provenance.decl_bodies_lowered, 0,
        "publishing IndexedReady must lower ZERO declaration bodies \
         (the shallow index carries names/kinds/spans/member headers \
         only); got {} bodies lowered at publish",
        provenance.decl_bodies_lowered,
    );
}

/// Cold-resolving one dependency-free symbol lowers exactly that one
/// declaration body — never the file's other `N - 1` bodies.
#[test]
fn resolve_unrelated_symbol_lowers_only_demanded_decl() {
    let host = make_host();
    upsert(&host, SCRATCH_ID, SCRATCH);
    host.provenance().reset();

    let node = host.resolve_named_symbol(SCRATCH_ID, "Unrelated", Some(ProjectionMode::Expanded));
    assert!(node.is_some(), "Unrelated must resolve");

    let provenance = snap(&host);
    assert_eq!(
        provenance.decl_bodies_lowered,
        1,
        "cold-resolving the dependency-free `Unrelated` must lower \
         exactly ONE declaration body — not the file's other {} \
         unrelated bodies; got {}",
        SCRATCH_DECLS - 1,
        provenance.decl_bodies_lowered,
    );
}

/// Positive control proving the counter counts the WALKED closure: a
/// symbol with same-file dependencies lowers its transitive dependency
/// chain — and still never the unreachable decls.
#[test]
fn resolve_with_dependencies_lowers_exactly_the_walked_closure() {
    let host = make_host();
    upsert(&host, CHAIN_ID, CHAIN);
    host.provenance().reset();

    let node = host.resolve_named_symbol(CHAIN_ID, "A", Some(ProjectionMode::Expanded));
    assert!(node.is_some(), "A must resolve");

    let provenance = snap(&host);
    // The engine's walked closure for `A = B & {{ a: 1 }}` is {A, B}:
    // the published projection keeps the heritage `Ref B`'s own
    // reference (`C`) as a shallow carrier (publication is
    // shallow-by-default), so `C` is NOT part of the walked closure —
    // and the unreachable `D`/`E` never lower under any mode.
    assert_eq!(
        provenance.decl_bodies_lowered, 2,
        "resolving `A` (→ B) must lower exactly the walked closure \
         {{A, B}} — never the carrier-preserved C, never the \
         unreachable D/E; got {}",
        provenance.decl_bodies_lowered,
    );
    assert!(
        provenance.decl_bodies_lowered < 5,
        "the file's full declaration set must NOT lower"
    );
}

/// Concurrent cold first-touch of the same symbol lowers its body ONCE
/// (per-entry singleflight or idempotent race — never duplicate
/// lowering published twice).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn lazy_decl_body_singleflight_lowers_once() {
    let host = make_host();
    upsert(&host, SCRATCH_ID, SCRATCH);
    host.provenance().reset();

    let barrier = Arc::new(std::sync::Barrier::new(8));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let host = Arc::clone(&host);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            host.resolve_named_symbol(SCRATCH_ID, "Unrelated", Some(ProjectionMode::Expanded))
        }));
    }
    for handle in handles {
        assert!(
            handle
                .join()
                .expect("resolver thread must not panic")
                .is_some(),
            "every concurrent caller must resolve Unrelated"
        );
    }

    let provenance = snap(&host);
    assert_eq!(
        provenance.decl_bodies_lowered, 1,
        "8 concurrent cold resolves of ONE symbol must lower its body \
         exactly once; got {}",
        provenance.decl_bodies_lowered,
    );
}

/// A content edit invalidates the body memo: the post-edit demand
/// lowers the NEW body (it is never served from the superseded
/// content's memo) and the projected type reflects the new content.
#[test]
fn content_edit_invalidates_decl_body_memo() {
    let host = make_host();
    upsert(&host, SCRATCH_ID, SCRATCH);

    let cold = host
        .resolve_named_symbol(SCRATCH_ID, "Unrelated", Some(ProjectionMode::Expanded))
        .expect("cold resolve must succeed");
    let cold_expr = host
        .project_node_to_type_expr_for_test(cold)
        .expect("cold node must project");
    let cold_repr = format!("{cold_expr:?}");
    assert!(
        cold_repr.contains('a') && !cold_repr.contains("edited"),
        "pre-edit projection must reflect the original body: {cold_repr}"
    );

    upsert(
        &host,
        SCRATCH_ID,
        "export type Unrelated = { edited: 2 };\n\
         type Var0 = { v: 0 };\n\
         type Var1 = { v: 1 };\n\
         type Var2 = { v: 2 };\n\
         type Var3 = { v: 3 };\n",
    );
    host.provenance().reset();
    let warm = host
        .resolve_named_symbol(SCRATCH_ID, "Unrelated", Some(ProjectionMode::Expanded))
        .expect("post-edit resolve must succeed");
    let warm_expr = host
        .project_node_to_type_expr_for_test(warm)
        .expect("post-edit node must project");
    let warm_repr = format!("{warm_expr:?}");
    assert!(
        warm_repr.contains("edited"),
        "post-edit projection must reflect the NEW body (the old \
         content's memo must not serve the new whole_hash): {warm_repr}"
    );

    let provenance = snap(&host);
    assert_eq!(
        provenance.decl_bodies_lowered, 1,
        "the post-edit demand must re-lower exactly the demanded \
         declaration under the new content hash; got {}",
        provenance.decl_bodies_lowered,
    );
}

/// Lazy body lowering reuses the scheduler-retained parse snapshot:
/// demanding several DISTINCT symbols of one file performs exactly ONE
/// eval-program parse in total — never a re-parse per body touch.
#[test]
fn lazy_decl_lowering_uses_scheduler_snapshot_not_reparse() {
    let host = make_host();
    upsert(&host, SCRATCH_ID, SCRATCH);
    host.provenance().reset();

    for symbol in ["Unrelated", "Var0", "Var1"] {
        assert!(
            host.resolve_named_symbol(SCRATCH_ID, symbol, Some(ProjectionMode::Expanded))
                .is_some(),
            "{symbol} must resolve"
        );
    }

    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 1,
        "three distinct-symbol demands on one file must share ONE \
         eval-program parse (the retained snapshot) — got {} parses",
        provenance.eval_program_parses,
    );
    assert_eq!(
        provenance.decl_bodies_lowered, 3,
        "three independent symbols demanded ⇒ exactly three bodies \
         lowered; got {}",
        provenance.decl_bodies_lowered,
    );
}

/// A LIVE artifact's retained parse snapshot is LEASE-PINNED, not
/// LRU/budget-evicted: after many OTHER files are resolved (each pinning
/// its own snapshot), demanding a NOT-yet-lowered symbol of the FIRST
/// file reuses its still-pinned snapshot — ZERO additional eval-program
/// parses. A reintroduced LRU/budget cap would evict the first file's
/// snapshot and force a silent re-parse.
#[test]
fn live_artifact_memo_pins_snapshot_across_many_other_files() {
    let host = make_host();

    // Each file declares TWO independent symbols; we cold-lower only the
    // FIRST symbol of each, leaving the SECOND un-lowered.
    const N: usize = 20;
    for i in 0..N {
        upsert(
            &host,
            &format!("/workspace/src/many{i}.ts"),
            &format!("export type A{i} = {{ v: {i} }};\nexport type B{i} = {{ w: {i} }};\n"),
        );
    }
    // Cold-resolve the FIRST symbol of every file — each demand pins that
    // file's memo lease (the memo is retained on its live `IndexedReady`).
    for i in 0..N {
        assert!(
            host.resolve_named_symbol(
                &format!("/workspace/src/many{i}.ts"),
                &format!("A{i}"),
                Some(ProjectionMode::Expanded),
            )
            .is_some(),
            "A{i} must resolve"
        );
    }

    host.provenance().reset();

    // Demand the SECOND, not-yet-lowered symbol of the FIRST file after
    // all 19 others are live. Its memo still pins file 0's snapshot, so
    // this re-uses it: zero new parses, exactly one new body lowered.
    assert!(
        host.resolve_named_symbol(
            "/workspace/src/many0.ts",
            "B0",
            Some(ProjectionMode::Expanded),
        )
        .is_some(),
        "B0 must resolve"
    );

    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 0,
        "demanding a fresh symbol of a LIVE artifact must reuse its \
         lease-pinned snapshot — never re-parse it (no LRU/budget \
         eviction); got {} re-parses",
        provenance.eval_program_parses,
    );
    assert_eq!(
        provenance.decl_bodies_lowered, 1,
        "exactly the one freshly demanded body (`B0`) must lower; got {}",
        provenance.decl_bodies_lowered,
    );
}

/// Parse-time fact emission is header-only: publishing an artifact
/// computes NO body-derived semantic hashes. Body-sensitive `Export` /
/// `LocalDecl` facts are produced by the lazy body fact path on first
/// observation — they are absent from the eager publish-time registry,
/// while the header-derived `MemberShape` fact stays eager.
#[test]
fn emit_parse_facts_never_hashes_decl_bodies() {
    use verter_semantic::facts::registry::{FactKey, SymbolSpace};

    let host = make_host();
    upsert(&host, SCRATCH_ID, SCRATCH);
    host.provenance().reset();

    let indexed = host
        .ensure_indexed_ready(SCRATCH_ID)
        .expect("artifact must materialise");
    let artifacts = host
        .project_type_store()
        .indexed()
        .get_artifacts_for_content(SCRATCH_ID, indexed.whole_hash)
        .expect("published artifacts must be readable");

    let export_key = FactKey::Export {
        name: crate::file_artifact_store::InternedName::from("Unrelated"),
        space: SymbolSpace::Type,
    };
    assert!(
        artifacts.facts.lookup(&export_key).is_none(),
        "the publish-time registry must NOT carry the body-derived \
         `Export` semantic hash — body facts are lazy"
    );
    let local_decl_key = FactKey::LocalDecl {
        name: crate::file_artifact_store::InternedName::from("Var0"),
        space: SymbolSpace::Type,
    };
    assert!(
        artifacts.facts.lookup(&local_decl_key).is_none(),
        "the publish-time registry must NOT carry the body-derived \
         `LocalDecl` semantic hash — body facts are lazy"
    );

    let value_export_key = FactKey::Export {
        name: crate::file_artifact_store::InternedName::from("ValueUnrelated"),
        space: SymbolSpace::Value,
    };
    assert!(
        artifacts.facts.lookup(&value_export_key).is_none(),
        "the publish-time registry must NOT carry the body-derived \
         `Export` semantic hash for a VALUE export — body facts are lazy"
    );
    let value_local_decl_key = FactKey::LocalDecl {
        name: crate::file_artifact_store::InternedName::from("Val0"),
        space: SymbolSpace::Value,
    };
    assert!(
        artifacts.facts.lookup(&value_local_decl_key).is_none(),
        "the publish-time registry must NOT carry the body-derived \
         `LocalDecl` semantic hash for a VALUE decl — body facts are lazy"
    );

    // Header-derived facts STAY eager: the member-name shape of
    // `Unrelated` comes from the syntactic member headers.
    let shape_key = FactKey::MemberShape {
        exporter: crate::file_artifact_store::InternedName::from("Unrelated"),
        space: SymbolSpace::Type,
    };
    assert!(
        artifacts.facts.lookup(&shape_key).is_some(),
        "the header-derived `MemberShape` fact must still emit eagerly \
         at publish"
    );

    let provenance = snap(&host);
    assert_eq!(
        provenance.decl_bodies_lowered, 0,
        "fact emission at publish must lower (and therefore hash) ZERO \
         declaration bodies; got {}",
        provenance.decl_bodies_lowered,
    );
}

/// A LOCAL export alias (`export { Foo as Bar }`) is served by the lazy
/// body fact path under its PUBLIC key (`Export(Bar, …)`), while the body
/// it lowers/hashes is the backing local declaration (`Foo`). The fact is
/// absent from the eager publish-time registry (publish stays body-free),
/// and only the demanded backing declarations lower — not the aliases or
/// unrelated decls.
#[test]
fn local_export_alias_lazy_fact_uses_public_key_and_backing_decl() {
    use verter_semantic::facts::registry::{FactKey, SymbolSpace};

    const ALIAS_ID: &str = "/workspace/src/alias.ts";
    let src = "type Foo = { a: 1 };\n\
               export { Foo as Bar };\n\
               const localValue: { v: 1 } = { v: 1 };\n\
               export { localValue as PublicValue };\n\
               type Untouched = { u: 1 };\n";

    let host = make_host();
    upsert(&host, ALIAS_ID, src);
    host.provenance().reset();

    let indexed = host
        .ensure_indexed_ready(ALIAS_ID)
        .expect("artifact must materialise");
    let artifacts = host
        .project_type_store()
        .indexed()
        .get_artifacts_for_content(ALIAS_ID, indexed.whole_hash)
        .expect("published artifacts must be readable");

    let type_alias_key = FactKey::Export {
        name: crate::file_artifact_store::InternedName::from("Bar"),
        space: SymbolSpace::Type,
    };
    let value_alias_key = FactKey::Export {
        name: crate::file_artifact_store::InternedName::from("PublicValue"),
        space: SymbolSpace::Value,
    };

    // Publish stays body-free: the alias `Export` facts are NOT eager.
    assert!(
        artifacts.facts.lookup(&type_alias_key).is_none(),
        "the publish-time registry must NOT carry the body-derived \
         alias `Export(Bar, Type)` fact — body facts are lazy"
    );
    assert!(
        artifacts.facts.lookup(&value_alias_key).is_none(),
        "the publish-time registry must NOT carry the body-derived \
         alias `Export(PublicValue, Value)` fact — body facts are lazy"
    );
    let publish_provenance = snap(&host);
    assert_eq!(
        publish_provenance.decl_bodies_lowered, 0,
        "publishing the alias index must lower ZERO bodies; got {}",
        publish_provenance.decl_bodies_lowered,
    );

    // The lazy path serves the PUBLIC export key, lowering the backing
    // local declaration on demand.
    host.provenance().reset();
    let type_fact = artifacts.facts.lookup_or_compute(&type_alias_key);
    assert!(
        type_fact.is_some(),
        "`Export(Bar, Type)` must be served by the lazy body fact path \
         (it lowers the backing local `Foo`)"
    );
    assert_eq!(
        type_fact.expect("present").key,
        type_alias_key,
        "the emitted fact must preserve the PUBLIC key `Export(Bar, Type)`, \
         never the backing-local key `Export(Foo, Type)`"
    );

    let value_fact = artifacts.facts.lookup_or_compute(&value_alias_key);
    assert!(
        value_fact.is_some(),
        "`Export(PublicValue, Value)` must be served by the lazy body \
         fact path (it lowers the backing local `localValue`)"
    );
    assert_eq!(
        value_fact.expect("present").key,
        value_alias_key,
        "the emitted fact must preserve the PUBLIC value key \
         `Export(PublicValue, Value)`"
    );

    // Only the two demanded backing declarations lowered — never the
    // unrelated `Untouched`, never any phantom alias-named decl.
    let provenance = snap(&host);
    assert_eq!(
        provenance.decl_bodies_lowered, 2,
        "exactly the two demanded backing declarations (`Foo`, \
         `localValue`) must lower — not the unrelated `Untouched`; got {}",
        provenance.decl_bodies_lowered,
    );

    // The public alias name is NOT itself a declaration: probing a
    // phantom `Export(Foo, Type)` (the backing-local name as a public
    // export) must miss — `Foo` is not exported under its own name.
    let phantom_key = FactKey::Export {
        name: crate::file_artifact_store::InternedName::from("Foo"),
        space: SymbolSpace::Type,
    };
    assert!(
        artifacts.facts.lookup_or_compute(&phantom_key).is_none(),
        "`Foo` is exported only as `Bar`; `Export(Foo, Type)` must miss"
    );
}

/// An overlay (session) body never serves a base read: the overlay
/// artifact owns its own memo, and the base demand lowers the BASE
/// body and projects the base content.
#[test]
fn overlay_decl_body_never_serves_base_read() {
    use crate::session_view::OverlaidView;

    let host = make_host();
    let canonical = "/workspace/src/overlaid.ts";
    upsert(&host, canonical, "export type Shared = { base: 1 };\n");

    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(
        canonical.to_string(),
        Arc::from("export type Shared = { overlay: 2 };\n"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    // Materialise the overlay artifact first — publish is index-only on
    // the overlay lane too.
    host.provenance().reset();
    let overlay = host
        .materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("overlay artifact must materialise");
    assert!(
        overlay.shallow_state.has_type_symbol("Shared"),
        "overlay index must inventory the overlaid symbol"
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.decl_bodies_lowered, 0,
        "overlay artifact publish must lower zero bodies; got {}",
        provenance.decl_bodies_lowered,
    );

    // A BASE resolve after the overlay materialised must project the
    // BASE body — the overlay's memo must never answer a base demand.
    host.provenance().reset();
    let base_node = host
        .resolve_named_symbol(canonical, "Shared", Some(ProjectionMode::Expanded))
        .expect("base resolve must succeed");
    let base_expr = host
        .project_node_to_type_expr_for_test(base_node)
        .expect("base node must project");
    let base_repr = format!("{base_expr:?}");
    assert!(
        base_repr.contains("base"),
        "the base read must project the BASE body, never the overlay \
         body: {base_repr}"
    );
    assert!(
        !base_repr.contains("overlay"),
        "the overlay body must NEVER leak into a base read: {base_repr}"
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.decl_bodies_lowered, 1,
        "the base demand lowers the base body itself (not zero — an \
         overlay-memo hit would be a cross-population leak); got {}",
        provenance.decl_bodies_lowered,
    );
}

/// Editing an enum's MEMBERS (add/rename/remove a variant) — with no
/// other decl change — must move the file's `parse_stable_hash`. Enum
/// member headers are part of the shallow decl skeleton; if the hash did
/// not fold them in, a member edit would keep the same skeleton hash and
/// a warm consumer of the enum would keep the stale member set
/// (under-invalidation). Pre-fix: the hash is unchanged and the asserts
/// FAIL; post-fix: every member edit moves the hash.
#[test]
fn enum_member_edit_moves_parse_stable_hash() {
    use crate::parse_stable_hash::compute_parse_stable_hash;

    const ENUM_ID: &str = "/workspace/src/colors.ts";
    let host = make_host();

    upsert(&host, ENUM_ID, "export enum Color { Red, Green }\n");
    let base = compute_parse_stable_hash(
        &host
            .ensure_indexed_ready(ENUM_ID)
            .expect("artifact must materialise"),
    );

    // Add a variant.
    upsert(&host, ENUM_ID, "export enum Color { Red, Green, Blue }\n");
    let added = compute_parse_stable_hash(
        &host
            .ensure_indexed_ready(ENUM_ID)
            .expect("artifact must materialise"),
    );
    assert_ne!(
        base, added,
        "adding an enum variant MUST move parse_stable_hash"
    );

    // Rename a variant (same count).
    upsert(&host, ENUM_ID, "export enum Color { Red, Green, Cyan }\n");
    let renamed = compute_parse_stable_hash(
        &host
            .ensure_indexed_ready(ENUM_ID)
            .expect("artifact must materialise"),
    );
    assert_ne!(
        added, renamed,
        "renaming an enum variant MUST move parse_stable_hash"
    );

    // Remove a variant.
    upsert(&host, ENUM_ID, "export enum Color { Red, Green }\n");
    let removed = compute_parse_stable_hash(
        &host
            .ensure_indexed_ready(ENUM_ID)
            .expect("artifact must materialise"),
    );
    assert_eq!(
        base, removed,
        "returning to the original member set MUST return to the original \
         parse_stable_hash (member set is the sole varying input here)"
    );
}

/// Parse-time fact emission carries a header-level member-presence fact
/// per enum variant (the same `MemberPresence` rail as type/value member
/// headers). Without it, a downstream member-presence consumer of an enum
/// observes nothing to revalidate. Pre-fix: no enum `MemberPresence` fact
/// is emitted and the lookup FAILS; post-fix: each variant emits one.
#[test]
fn enum_members_emit_header_member_presence_facts() {
    use verter_semantic::facts::registry::{FactKey, MemberKind, SymbolSpace};

    const ENUM_ID: &str = "/workspace/src/enum_facts.ts";
    let host = make_host();
    upsert(&host, ENUM_ID, "export enum Color { Red, Green, Blue }\n");

    let indexed = host
        .ensure_indexed_ready(ENUM_ID)
        .expect("artifact must materialise");
    let emission = crate::fact_emission::emit_parse_facts(&indexed);

    for variant in ["Red", "Green", "Blue"] {
        let key = FactKey::MemberPresence {
            exporter: crate::file_artifact_store::InternedName::from("Color"),
            name: crate::file_artifact_store::InternedName::from(variant),
            space: SymbolSpace::Value,
        };
        assert!(
            emission.facts.lookup(&key).is_some(),
            "enum variant `{variant}` must emit a header-level MemberPresence fact"
        );
    }

    // The presence hash must be variant-discriminating: a different
    // variant name produces a different hash on the same enum.
    let red = crate::file_artifact_store::InternedName::from("Red");
    let green = crate::file_artifact_store::InternedName::from("Green");
    let red_fact = emission
        .facts
        .lookup(&FactKey::MemberPresence {
            exporter: crate::file_artifact_store::InternedName::from("Color"),
            name: red,
            space: SymbolSpace::Value,
        })
        .expect("Red present")
        .semantic_hash;
    let green_fact = emission
        .facts
        .lookup(&FactKey::MemberPresence {
            exporter: crate::file_artifact_store::InternedName::from("Color"),
            name: green,
            space: SymbolSpace::Value,
        })
        .expect("Green present")
        .semantic_hash;
    assert_ne!(
        red_fact, green_fact,
        "distinct enum variants must carry distinct MemberPresence hashes"
    );
    // Sanity: the kind is EnumMember (keeps the helper honest if reused).
    let _ = MemberKind::EnumMember;
}

/// A MERGED enum splits its members across several same-name `enum`
/// declarations (legal TS declaration merging). Editing a member of a
/// LATER declaration — with the first declaration untouched — must move
/// `parse_stable_hash`. Pre-fix the header walk dropped every later
/// declaration's members, so the skeleton hash never moved and a warm
/// enum consumer kept the stale member surface (under-invalidation).
#[test]
fn merged_enum_later_decl_member_edit_moves_parse_stable_hash() {
    use crate::parse_stable_hash::compute_parse_stable_hash;

    const ENUM_ID: &str = "/workspace/src/merged_colors.ts";
    let host = make_host();

    // Members split across two declarations; the contributor COUNT (2) is
    // constant across every edit below, so the only varying hash input is
    // the SECOND declaration's member list.
    upsert(
        &host,
        ENUM_ID,
        "enum Color { Red }\nenum Color { Green = 1 }\n",
    );
    let base = compute_parse_stable_hash(
        &host
            .ensure_indexed_ready(ENUM_ID)
            .expect("artifact must materialise"),
    );

    // Add a member to the SECOND (later) declaration.
    upsert(
        &host,
        ENUM_ID,
        "enum Color { Red }\nenum Color { Green = 1, Blue = 2 }\n",
    );
    let added = compute_parse_stable_hash(
        &host
            .ensure_indexed_ready(ENUM_ID)
            .expect("artifact must materialise"),
    );
    assert_ne!(
        base, added,
        "adding a member to the SECOND enum declaration MUST move \
         parse_stable_hash (pre-fix the later decl's members were dropped)"
    );

    // Rename the second declaration's added member (member count constant).
    upsert(
        &host,
        ENUM_ID,
        "enum Color { Red }\nenum Color { Green = 1, Cyan = 2 }\n",
    );
    let renamed = compute_parse_stable_hash(
        &host
            .ensure_indexed_ready(ENUM_ID)
            .expect("artifact must materialise"),
    );
    assert_ne!(
        added, renamed,
        "renaming a member in the SECOND enum declaration MUST move parse_stable_hash"
    );

    // Returning to the original member set returns the original hash.
    upsert(
        &host,
        ENUM_ID,
        "enum Color { Red }\nenum Color { Green = 1 }\n",
    );
    let restored = compute_parse_stable_hash(
        &host
            .ensure_indexed_ready(ENUM_ID)
            .expect("artifact must materialise"),
    );
    assert_eq!(
        base, restored,
        "returning to the original merged member set MUST return the original hash"
    );
}

/// Parse-time fact emission for a MERGED enum must carry a header-level
/// `MemberPresence` fact for EVERY variant across ALL declarations, and
/// the whole-surface `MemberShape` fact must reconstruct the same surface
/// as the equivalent single-declaration enum. Pre-fix the later
/// declaration's members were dropped, so Green/Blue produced no fact and
/// the merged `MemberShape` diverged from the flattened form.
#[test]
fn merged_enum_emits_member_facts_for_every_declaration() {
    use verter_semantic::facts::registry::{FactKey, SymbolSpace};

    const MERGED_ID: &str = "/workspace/src/merged_enum_facts.ts";
    const SINGLE_ID: &str = "/workspace/src/single_enum_facts.ts";
    let host = make_host();

    upsert(
        &host,
        MERGED_ID,
        "enum Color { Red }\nenum Color { Green = 1, Blue = 2 }\n",
    );
    let merged = crate::fact_emission::emit_parse_facts(
        &host
            .ensure_indexed_ready(MERGED_ID)
            .expect("merged artifact must materialise"),
    );

    // EVERY member across BOTH declarations emits a presence fact.
    for variant in ["Red", "Green", "Blue"] {
        let key = FactKey::MemberPresence {
            exporter: crate::file_artifact_store::InternedName::from("Color"),
            name: crate::file_artifact_store::InternedName::from(variant),
            space: SymbolSpace::Value,
        };
        assert!(
            merged.facts.lookup(&key).is_some(),
            "merged enum variant `{variant}` (from a later declaration) \
             must emit a header-level MemberPresence fact"
        );
    }

    // The merged whole-surface MemberShape must equal the flattened
    // single-declaration enum's MemberShape (identical member set ⇒
    // identical shape hash; the shape hash folds no contributor count).
    upsert(
        &host,
        SINGLE_ID,
        "enum Color { Red, Green = 1, Blue = 2 }\n",
    );
    let single = crate::fact_emission::emit_parse_facts(
        &host
            .ensure_indexed_ready(SINGLE_ID)
            .expect("single artifact must materialise"),
    );
    let shape_key = FactKey::MemberShape {
        exporter: crate::file_artifact_store::InternedName::from("Color"),
        space: SymbolSpace::Value,
    };
    let merged_shape = merged
        .facts
        .lookup(&shape_key)
        .expect("merged enum MemberShape fact")
        .semantic_hash;
    let single_shape = single
        .facts
        .lookup(&shape_key)
        .expect("single enum MemberShape fact")
        .semantic_hash;
    assert_eq!(
        merged_shape, single_shape,
        "the merged enum's member surface must reconstruct the same \
         MemberShape as the equivalent single-declaration enum"
    );
}
