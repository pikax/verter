//! The LIVE source-side walk for the TS7 `TypeExpr`-projection oracle
//! (`docs/arch/u0-oracle-harness-design.md` §Q2 — "The source-side walk's
//! concrete entry API + return shape").
//!
//! [`resolve_source_declarations`] is the generator-side helper the two-sided
//! admission gate calls to bind a typed [`SourceLocator`]
//! (`reference_canonical`, `reference_name`, `symbol_space`) to its REAL
//! defining declaration contributor(s), producing the [`SourceWalkResult`] the
//! [`super::admission`] predicate consumes directly.
//!
//! It is **NOT a second resolution engine** (CLAUDE.md "Exactly one
//! type-resolution engine"). It asks the ONE shared resolver — through the
//! [`ResolverContext`] facade and the `ShallowFileState` symbol inventory it
//! owns — three navigation questions and nothing more:
//!
//! 1. **Binding / import / reexport / barrel routing.** A name is followed to
//!    its ultimate defining declaration through the shared graph's
//!    already-resolved cross-file edges: `ShallowFileState::import_targets`
//!    (an `import { X } from "./barrel"` local), `ExportTarget::Reexport`
//!    (`export { X } from "./leaf"`), and `ExportTarget::Local` aliases
//!    (`export { Foo as Bar }`). These edges carry canonical target IDs the
//!    shared resolver resolved at `ShallowFileState` construction — consuming
//!    them is reuse of the one engine, not a parallel walk.
//! 2. **Merged-contributor enumeration.** A `MergedDecl` (same-name interfaces)
//!    surfaces every contributor body via `TypeDeclBody::contributors()`, paired
//!    by ORDINAL with the parse-time `RawSourceSurface` contributor vector
//!    (the `raw_source_surfaces_for` capture). EVERY contributor is returned
//!    so the admission predicate can check ALL — a single clean contributor does
//!    NOT admit a merge whose peer carries a REJECT construct (§Q2).
//! 3. **Type-vs-value space selection.** `symbol_space` selects the TYPE
//!    (`symbols`) vs VALUE (`value_symbols`) inventory, so `typeof f` walks
//!    `f`'s VALUE declaration and a type query walks the TYPE declaration.
//!
//! The COMBINED admission surface per contributor is the
//! `(RawSourceSurface raw facts, already-lowered body `TypeExpr`)` pair — both
//! read off the existing shallow artifact, never re-resolved or re-parsed.
//!
//! The walk is **transitive** through `typeof` referents
//! (`RawSourceSurface::transitive_referents`, captured at parse from `typeof x`
//! type queries) and terminates under a VISITED-SET cycle guard keyed by the
//! `(canonical, name, symbol_space)` declaration identity: re-entering an
//! already-visited key is a [`SourceWalkResult::Cycle`] and REJECTS the
//! `(row, query)` (a cyclic source surface is never admitted via this harness —
//! it routes to the future structured oracle, never a hung or best-effort
//! admit). A locator that does not bind to a defining declaration in the
//! controlled fixture set — or a bound contributor whose parse-time capture is
//! absent / cannot be paired with its lowered body — is
//! [`SourceWalkResult::Unresolved`] and likewise REJECTS.
//!
//! This module is `#[cfg(test)]`-only (the whole `typeinfo_tests` tree is), so
//! it never enters the resolver build closure or the production type-resolution
//! hot path — the `tsgo`-forbidden-at-runtime and one-engine invariants hold:
//! it touches NO tsgo and adds NO query-time resolution path.

use rustc_hash::FxHashSet;

use verter_compiler::utils::oxc::script::raw_surface::SymbolSpace;
use verter_type_expr::{PrimitiveName, TypeExpr};

use crate::resolver_core::{ExportTarget, ResolverContext};

use super::admission::{SourceContributor, SourceWalkResult};

/// A typed locator into the shared declaration graph: the file + name + space a
/// source-side walk binds from. The same triple the shallow inventory keys.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SourceLocator {
    /// The canonical (leading-slash) file id the reference is made FROM.
    pub(crate) reference_canonical: String,
    /// The referenced symbol name as written at the reference site.
    pub(crate) reference_name: String,
    /// TYPE vs VALUE space — selects which inventory the name binds in.
    pub(crate) symbol_space: SymbolSpace,
}

/// Belt-and-suspenders bound on import/reexport hops, independent of the
/// `typeof`-transitive visited-set. A controlled fixture set never chains this
/// deep; the bound just prevents a pathological import cycle from looping.
const MAX_IMPORT_HOPS: usize = 64;

/// Resolve `locator` to its defining declaration contributor(s) through the
/// shared resolver, walking transitively through `typeof` referents under a
/// cycle guard. The returned [`SourceWalkResult`] feeds
/// [`super::admission::admit_source_walk`] directly.
#[allow(dead_code)]
pub(crate) fn resolve_source_declarations<C: ResolverContext>(
    ctx: &C,
    locator: &SourceLocator,
) -> SourceWalkResult {
    let mut visited: FxHashSet<(String, String, SymbolSpace)> = FxHashSet::default();
    let mut contributors: Vec<SourceContributor> = Vec::new();
    match walk(ctx, locator, &mut visited, &mut contributors) {
        WalkOutcome::Ok if contributors.is_empty() => SourceWalkResult::Unresolved,
        WalkOutcome::Ok => SourceWalkResult::Resolved { contributors },
        WalkOutcome::Unresolved => SourceWalkResult::Unresolved,
        WalkOutcome::Cycle => SourceWalkResult::Cycle,
    }
}

/// The internal walk outcome. `Ok` appends contributors to the accumulator;
/// `Unresolved` / `Cycle` short-circuit to the rejecting `SourceWalkResult`.
enum WalkOutcome {
    Ok,
    Unresolved,
    Cycle,
}

fn walk<C: ResolverContext>(
    ctx: &C,
    locator: &SourceLocator,
    visited: &mut FxHashSet<(String, String, SymbolSpace)>,
    acc: &mut Vec<SourceContributor>,
) -> WalkOutcome {
    // 1. Bind the locator to its ultimate DEFINING (canonical, name) through the
    //    shared graph's import / reexport / alias edges.
    let Some((def_canonical, def_name)) = resolve_defining(
        ctx,
        &locator.reference_canonical,
        &locator.reference_name,
        locator.symbol_space,
    ) else {
        return WalkOutcome::Unresolved;
    };

    // 2. Cycle guard on the DEFINING declaration identity. A `typeof` chain that
    //    re-enters an already-visited declaration is a cycle.
    let key = (
        def_canonical.clone(),
        def_name.clone(),
        locator.symbol_space,
    );
    if !visited.insert(key) {
        return WalkOutcome::Cycle;
    }

    // 3. The defining file's shallow inventory. The per-symbol raw-fact
    //    surfaces are a DEMAND product of the declaration-body memo —
    //    captured from exactly the demanded symbol's contributing
    //    statements, never an eager whole-program inventory.
    let Some(shallow) = ctx.shallow_file_state(&def_canonical) else {
        return WalkOutcome::Unresolved;
    };

    // 4. Enumerate the contributor surface(s): the lowered body / value, paired
    //    by ordinal with the parse-time raw-fact contributor vector.
    let surfaces = shallow
        .decl_bodies()
        .raw_surfaces_for(&def_name, locator.symbol_space);
    let lowered_bodies: Vec<TypeExpr> = match locator.symbol_space {
        SymbolSpace::Type => {
            let Some(lowered) = shallow.type_decl(&def_name) else {
                return WalkOutcome::Unresolved;
            };
            lowered.body.contributors().to_vec()
        }
        SymbolSpace::Value => {
            if !shallow.has_value_symbol(&def_name) {
                return WalkOutcome::Unresolved;
            }
            // A value has no `TypeDeclBody` — the lowered-body half is a TYPE-
            // declaration concept (the non-erased `Conditional` / `Mapped` /
            // callable / … `TypeExpr` variants of a type alias / interface). A
            // value's admission surface is instead its `RawSourceSurface`, which
            // the parse-time capture fills from BOTH the initializer (`as const`
            // provenance, overload SET) AND a scan of its type annotation
            // (unique-symbol / non-static-key / `this` / tuple-shape facts), PLUS
            // the transitive `typeof` walk (the annotation's `typeof` referents
            // are FOLLOWED, not rejected — they are the navigation edge, so the
            // annotation is NOT replayed as a lowered body, which would
            // double-reject the very `typeof` we are walking). The body is
            // therefore a neutral admissible placeholder; the raw facts +
            // transitive walk do the discrimination.
            vec![TypeExpr::Primitive(PrimitiveName::Unknown)]
        }
    };

    // The lowered bodies and the parse-time raw-fact contributors are produced by
    // two independent passes over the SAME file in source order. If their counts
    // disagree we cannot pair a contributor to its erased facts — conservatively
    // REJECT (Unresolved) rather than risk pairing the wrong raw facts to a body.
    if surfaces.len() != lowered_bodies.len() {
        return WalkOutcome::Unresolved;
    }

    // Snapshot the transitive next-hops before borrowing `acc`, so the recursive
    // re-entry does not alias the analysis Arc.
    let mut next_hops: Vec<SourceLocator> = Vec::new();
    for (ordinal, (raw_surface, lowered_body)) in surfaces.iter().zip(lowered_bodies).enumerate() {
        // Source-ROOT carve-out SAME-FILE binding: for a carve-out-shaped body
        // (`keyof Root` / `Root["a"]["b"]…` / `Root[keyof Root]` /
        // `typeof Root(.ident)*` / `Util<carve-out-arg>`) resolve the root
        // through the SHARED resolver edges in the SPACE the locator names
        // (TYPE space for keyof/chain/self-index/bare-ref-arg roots, VALUE
        // space for a typeof path head) and stamp the file it defines in
        // PLUS the root declaration's OWN parse-time raw-fact records (every
        // merged contributor). The admission gate admits the operator body
        // ONLY when the stamp equals `def_canonical` AND every root raw
        // surface walks the allowlist clean. The transitive walk below
        // follows `typeof` referents ONLY — it does NOT chase a `keyof` /
        // index root — so this targeted resolution is what proves same-file.
        // A root that does not bind stamps `None` and the gate rejects the
        // operator.
        let carve_out_locator = super::admission::carve_out_root_locator(&lowered_body);
        let carve_out_root = carve_out_locator.and_then(|(root_name, space)| {
            resolve_defining(ctx, &def_canonical, root_name, space)
                .map(|resolved| (resolved, space))
        });
        // A VALUE-space carve-out root whose every raw surface is a LEAF (no
        // transitive `typeof` referents of its own) is SUBSUMED: its raw facts
        // ride inside this contributor (`carve_out_root_surfaces`), so the
        // `typeof` referent naming it is NOT re-walked as a second top-level
        // contributor — that would conflate navigation depth with the queried
        // declaration's merge multiplicity and break the provably-single-
        // contributor class for every admissible `Util<typeof x>` row. A
        // NON-leaf root keeps the hop (one-level subsumption cannot vouch for
        // the deeper chain's lossy facts — conservative default-reject).
        let mut subsumed_referent: Option<&str> = None;
        let (carve_out_root_def, carve_out_root_surfaces) = match carve_out_root {
            Some(((root_canonical, root_name), space)) => {
                let root_surfaces = ctx
                    .shallow_file_state(&root_canonical)
                    .map(|root_shallow| {
                        root_shallow
                            .decl_bodies()
                            .raw_surfaces_for(&root_name, space)
                            .as_ref()
                            .clone()
                    })
                    .unwrap_or_default();
                if space == SymbolSpace::Value
                    && !root_surfaces.is_empty()
                    && root_surfaces
                        .iter()
                        .all(|s| s.transitive_referents.is_empty())
                {
                    subsumed_referent = carve_out_locator.map(|(name, _)| name);
                }
                (Some(root_canonical), root_surfaces)
            }
            None => (None, Vec::new()),
        };
        for referent in &raw_surface.transitive_referents {
            if Some(referent.reference_name.as_str()) == subsumed_referent {
                continue;
            }
            // A `typeof x` referent resolves in VALUE space (the referent is a
            // value whose declaration carries the lossy provenance). The hop is
            // made FROM the defining file (the referent name is written there).
            next_hops.push(SourceLocator {
                reference_canonical: def_canonical.clone(),
                reference_name: referent.reference_name.clone(),
                symbol_space: SymbolSpace::Value,
            });
        }
        acc.push(SourceContributor {
            ordinal: ordinal as u16,
            def_canonical: def_canonical.clone(),
            raw_surface: raw_surface.clone(),
            lowered_body,
            carve_out_root_def,
            carve_out_root_surfaces,
        });
    }

    // 5. Transitive walk through every captured `typeof` referent, under the
    //    shared visited-set. A cycle / unresolved hop propagates and REJECTS.
    for hop in &next_hops {
        match walk(ctx, hop, visited, acc) {
            WalkOutcome::Ok => {}
            other => return other,
        }
    }

    WalkOutcome::Ok
}

/// Follow the shared graph's already-resolved import / reexport / export-alias
/// edges to the ultimate DEFINING `(canonical, name)` of `name` referenced from
/// `canonical`. Returns `None` when the name does not bind to a defining
/// declaration in the controlled fixture set (an unresolved import, a missing
/// leaf, an export with no backing declaration).
fn resolve_defining<C: ResolverContext>(
    ctx: &C,
    canonical: &str,
    name: &str,
    space: SymbolSpace,
) -> Option<(String, String)> {
    let mut cur_canonical = canonical.to_string();
    let mut cur_name = name.to_string();

    for _ in 0..MAX_IMPORT_HOPS {
        let shallow = ctx.shallow_file_state(&cur_canonical)?;

        // (a) An import-local name — follow to the imported module + export name.
        if let Some(target) = shallow.import_targets.get(&cur_name) {
            cur_canonical = target.canonical_id.clone();
            cur_name = target.imported_name.clone();
            continue;
        }

        // (b) An explicit reexport (`export { X } from "./leaf"`) — follow to the
        //     source module + original export name.
        if let Some(ExportTarget::Reexport {
            canonical_id,
            original_name,
            ..
        }) = shallow.exports.get(&cur_name)
        {
            cur_canonical = canonical_id.clone();
            cur_name = original_name.clone();
            continue;
        }

        // (c) A locally-declared symbol in the requested space — the definition.
        let locally_defined = match space {
            SymbolSpace::Type => shallow.has_type_symbol(&cur_name),
            SymbolSpace::Value => shallow.has_value_symbol(&cur_name),
        };
        if locally_defined {
            return Some((cur_canonical, cur_name));
        }

        // (d) An export ALIAS (`export { Foo as Bar }`) bound to a local decl —
        //     resolve the alias to the backing local name and re-test.
        if let Some(ExportTarget::Local { symbol_name }) = shallow.exports.get(&cur_name) {
            if symbol_name != &cur_name {
                let backing = symbol_name.clone();
                let backing_defined = match space {
                    SymbolSpace::Type => shallow.has_type_symbol(&backing),
                    SymbolSpace::Value => shallow.has_value_symbol(&backing),
                };
                if backing_defined {
                    return Some((cur_canonical, backing));
                }
            }
        }

        // No further edge to follow — the name does not bind here.
        return None;
    }

    None
}

#[cfg(test)]
mod tests;
