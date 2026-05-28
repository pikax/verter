//! Typeinfo-surface JSDoc-provenance CHARACTERIZATION, driven through the
//! [`crate::VerterHost::resolve_shallow_surface`] accessor + its span-rich
//! [`crate::typeinfo::TypeInfoSurface`] projection.
//!
//! These tests CHARACTERIZE the public typeinfo surface's structural JSDoc
//! attach ([`TypeInfoSurface::with_member_jsdoc_spans`]) against the three hard
//! provenance scenarios that the JSDoc-provenance fixes (P2-2 / P2-3 / P2-4)
//! also target. The public surface is immune to all three bug classes BY
//! CONSTRUCTION: it locates a member's JSDoc STRUCTURALLY from the member's
//! DECLARATION origin (`SurfaceMemberOrigin::canonical_file`, which U1 made
//! survive substitution) + the member's own name-token span — never from the
//! member's VALUE-node origin (which moves under substitution) and never from a
//! file-wide textual first-match (which a decoy declaration could capture).
//!
//! Because the surface attach is already structural, these surface assertions
//! pass both before and after the P2 source fixes — they are CHARACTERIZATION
//! of the landed surface behavior, NOT the discriminating P2 fix-gate. The
//! DISCRIMINATING P2-2 / P2-3 / P2-4 regressions (which FAIL pre-fix / PASS
//! post-fix) live in `tests/jsdoc_provenance_p2.rs`, driving the component-meta
//! LAZY imported-macro-surface rail (`member_display_jsdoc`) — the surface that
//! actually carried the value-node / `?`-only-matcher bugs the P2s fix.
//!
//! - Generic inheritance: a generic inherited member's `value` node points at
//!   the type-arg in the DERIVED file post-substitution, yet the surface
//!   attributes its JSDoc to the base `declaration_origin`.
//! - Duplicate name + same value type: two declarations whose members intern to
//!   the same value node are disambiguated by each member's OWN name span.
//! - Definite assignment: a class `/** doc */ foo!: string` field attaches its
//!   JSDoc from the name-token offset (the `!` follows the name, so the
//!   leading-comment walk is unaffected).
//!
//! Each test SLICES the declaring file's source at the reported span and
//! compares to the expected token.

use super::support::*;
use crate::typeinfo::{CanonicalSpan, TypeInfoSurface, TypeInfoSurfaceMember};

/// Slice the canonical-span out of `source`, asserting it references
/// `expected_file` (so a cross-file member cannot silently slice the wrong
/// file — the cross-file provenance guard).
fn slice<'a>(source: &'a str, span: &CanonicalSpan, expected_file: &str) -> &'a str {
    assert_eq!(
        span.file.as_ref(),
        expected_file,
        "span must reference file {expected_file}, got {}",
        span.file
    );
    &source[span.span.start as usize..span.span.end as usize]
}

fn member<'a>(surface: &'a TypeInfoSurface, name: &str) -> &'a TypeInfoSurfaceMember {
    surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == name)
        .unwrap_or_else(|| {
            panic!(
                "member `{name}` must be on the surface; got {:?}",
                surface
                    .members
                    .iter()
                    .map(|m| m.name.as_ref())
                    .collect::<Vec<_>>()
            )
        })
}

// ---------------------------------------------------------------------------
// P2-2 — generic inherited-member JSDoc origin.
//
// `interface Base<T> { /** base doc */ x: T }` in base.ts; a derived interface
// instantiates `Base<string>`. After substitution the inherited `x` member's
// VALUE node points at `string` in the DERIVED file's type-arg position, so a
// `member.value`-origin JSDoc lookup would read derived.ts (and find no JSDoc,
// or the wrong one). The surface must attribute `x`'s JSDoc via the member's
// `declaration_origin` (= base.ts), slicing `base doc` from base.ts.
// ---------------------------------------------------------------------------

#[test]
fn generic_inherited_member_jsdoc_origin_resolves_to_base_declaration_file() {
    const BASE: &str = "/src/base.ts";
    const DERIVED: &str = "/src/derived.ts";
    // The base member `x` is GENERIC (`x: T`) and carries the JSDoc. The decoy
    // text in derived.ts (`derived doc`) must NOT be picked up for `x` — `x` is
    // declared only in base.ts.
    let base_src = "export interface Base<T> {\n  /** base doc */\n  x: T;\n}\n";
    let derived_src = "import type { Base } from './base';\n\
         /** derived doc */\n\
         export interface Derived extends Base<string> {\n  derivedOnly: number;\n}\n";

    let host =
        make_host_with_workspace_files_footprint(&[(BASE, base_src), (DERIVED, derived_src)]);

    let surface = host
        .resolve_shallow_surface(DERIVED, "Derived")
        .expect("Derived must resolve to a one-level surface across the generic heritage edge");

    let x = member(&surface, "x");

    // The inherited generic member `x` ORIGINATES in base.ts (its
    // declaration_origin survives the `Base<string>` substitution), even though
    // its substituted value type (`string`) is written in derived.ts.
    assert_eq!(
        x.origin.canonical_file.as_deref(),
        Some(BASE),
        "the inherited generic member `x` must report its DECLARATION file (base.ts), not the \
         derived file where its substituted type-arg `string` is written"
    );

    // The JSDoc DESCRIPTION span must reference base.ts and slice to `base doc`.
    let desc_span = x.jsdoc_description_span.as_ref().expect(
        "the inherited generic member `x` must carry its base-declared JSDoc description span \
         (attributed via declaration_origin, not the substituted value-node origin)",
    );
    assert_eq!(
        slice(base_src, desc_span, BASE),
        "base doc",
        "the generic inherited member `x`'s JSDoc must slice `base doc` from the BASE file \
         (declaration_origin), NOT the derived file"
    );

    // NEGATIVE: the description span must NOT reference the derived file (which
    // is where a value-node-origin lookup would have landed post-substitution),
    // and must NOT slice the derived decoy text.
    assert_ne!(
        desc_span.file.as_ref(),
        DERIVED,
        "the inherited member's JSDoc must not be attributed to the derived consumer file"
    );
    assert_ne!(
        slice(base_src, desc_span, BASE),
        "derived doc",
        "the inherited member's JSDoc must not pick up the derived interface's decoy doc"
    );
}

// ---------------------------------------------------------------------------
// P2-3 — duplicate-name same-value JSDoc.
//
// Two interface declarations declare the same member name AND the same value
// type. Their member VALUE nodes intern identically, so value-node-identity
// attribution cannot tell them apart and a file-wide first-match captures the
// decoy. The per-member span (each member carries its OWN name span anchored to
// its declaration) disambiguates: querying the surface that DECLARES `right`
// returns the `right` doc, never the `wrong` decoy.
// ---------------------------------------------------------------------------

#[test]
fn duplicate_name_same_value_jsdoc_disambiguates_by_declaration_span() {
    const FILE: &str = "/src/dup.ts";
    // `Decoy.field` and `Real.field` both have member name `field` AND the SAME
    // value type `string` → identical interned value nodes. `Decoy` declares
    // `field` FIRST (a file-wide first-match would attach `wrong`); `Real`
    // declares it second with `right`.
    let src = "export interface Decoy {\n  /** wrong */\n  field: string;\n}\n\
         export interface Real {\n  /** right */\n  field: string;\n}\n";

    let host = make_host_with_workspace_files_footprint(&[(FILE, src)]);

    let surface = host
        .resolve_shallow_surface(FILE, "Real")
        .expect("Real must resolve to a one-level surface");

    let field = member(&surface, "field");

    // The member's declaration / name span must anchor to `Real`'s declaration
    // (the SECOND `field`), not `Decoy`'s. Slice the name span and confirm it
    // sits inside `Real`, after the `Real` keyword.
    let name_span = field
        .name_span
        .as_ref()
        .expect("`field` must carry a name span");
    let real_decl_start = src.find("interface Real").expect("Real must be declared");
    assert!(
        (name_span.span.start as usize) > real_decl_start,
        "the resolved `field` member must be `Real`'s declaration (after `interface Real`), not \
         `Decoy`'s; name span start = {}, Real decl start = {real_decl_start}",
        name_span.span.start
    );

    // The JSDoc must be `right` (from Real), NEVER `wrong` (the Decoy decoy that
    // a value-node / file-wide first-match would have attached).
    let desc_span = field.jsdoc_description_span.as_ref().expect(
        "`Real.field` must carry its OWN JSDoc description span, disambiguated by its declaration \
         span — not the decoy's",
    );
    assert_eq!(
        slice(src, desc_span, FILE),
        "right",
        "the duplicate-name same-value member must resolve to `Real`'s `right` doc via its \
         declaration span; `wrong` would prove value-node / first-match collision"
    );
    assert_ne!(
        slice(src, desc_span, FILE),
        "wrong",
        "the Decoy interface's `wrong` doc must NOT be attached to `Real.field`"
    );
}

// ---------------------------------------------------------------------------
// P2-4 — class `/** doc */ foo!: string` definite-assignment field.
//
// The textual matcher accepted `name` → optional `?` → `:` / `(` but NOT `!`
// (definite assignment), so a `foo!: string` class field's JSDoc was missed.
// The surface attaches JSDoc STRUCTURALLY from the member's name-token offset:
// the `!` is AFTER the name, so the backward leading-comment walk is
// unaffected, and `foo`'s JSDoc resolves. This test reaches the class field
// through the public surface and asserts `foo` is present with its JSDoc.
// ---------------------------------------------------------------------------

#[test]
fn class_definite_assignment_field_carries_jsdoc_via_declaration_span() {
    const FILE: &str = "/src/definite.ts";
    // `foo!: string` is a definite-assignment class field; `plain: number` is a
    // normal field (control). `undocumentedDefinite!: boolean` is a
    // definite-assignment field with NO JSDoc (negative control).
    let src = "export class WithDefinite {\n  \
         /** the definite field */\n  \
         foo!: string;\n  \
         /** the plain field */\n  \
         plain: number;\n  \
         undocumentedDefinite!: boolean;\n}\n";

    let host = make_host_with_workspace_files_footprint(&[(FILE, src)]);

    let surface = host
        .resolve_shallow_surface(FILE, "WithDefinite")
        .expect("WithDefinite must resolve to a one-level surface");

    // POSITIVE: the definite-assignment field `foo!: string` is present on the
    // surface (the `!` must not drop the member) with its JSDoc.
    let foo = member(&surface, "foo");
    assert_eq!(
        foo.origin.canonical_file.as_deref(),
        Some(FILE),
        "the definite-assignment field `foo` must report its declaration file"
    );
    let foo_doc = foo.jsdoc_description_span.as_ref().expect(
        "the definite-assignment field `/** doc */ foo!: string` must carry its JSDoc \
         description span (the `!` must not block the leading-comment attach)",
    );
    assert_eq!(
        slice(src, foo_doc, FILE),
        "the definite field",
        "`foo!`'s JSDoc must slice to its doc text; a miss would prove the `!:` matcher gap"
    );

    // CONTROL: the plain field's JSDoc still resolves (no regression).
    let plain = member(&surface, "plain");
    let plain_doc = plain
        .jsdoc_description_span
        .as_ref()
        .expect("the plain field `plain: number` must carry its JSDoc description span");
    assert_eq!(slice(src, plain_doc, FILE), "the plain field");

    // NEGATIVE: a definite-assignment field with NO JSDoc carries no description
    // span (the structural attach must not invent one for `undocumentedDefinite`).
    let undocumented = member(&surface, "undocumentedDefinite");
    assert!(
        undocumented.jsdoc_description_span.is_none(),
        "an undocumented definite-assignment field must carry NO JSDoc description span"
    );
}
