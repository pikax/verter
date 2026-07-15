//! Unit coverage for the canonical component-scope facts (OXC scope-tree): runtime-
//! surviving value-binding completeness (const/let/var/function/class/destructure/
//! import/nested), ambient (`declare`) + enum-member erasure, free-reference
//! collection with lexical shadowing, the authored-vs-synthesized store-accessor
//! distinction, top-level-root scoping, and the module→instance scope topology.

use super::*;
use crate::svelte::runtime::client_imports::UserImportSlot;
use crate::svelte::runtime::client_surface_imports::classify_script_imports_from_sources;

/// Build facts for a component with the given module / instance scripts (no
/// template contributions).
fn facts_for(module: Option<&str>, instance: Option<&str>) -> ComponentScopeFacts {
    let alloc = Allocator::default();
    let imports = classify_script_imports_from_sources(module, instance);
    let template_decls = FxHashSet::default();
    let exprs = ExprArena::new();
    build_component_scope_facts(&alloc, module, instance, &imports, &template_decls, &exprs)
        .expect("clean scripts produce facts")
}

#[test]
fn captures_every_source_form_declaration() {
    // A `const`/`let`/`var`/`function`/`class`/destructure inventory — the exact
    // set of source-form declarations the selective scope graph does NOT retain.
    let facts = facts_for(
        None,
        Some(
            "const a = 1;\nlet b = 2;\nvar c = 3;\nfunction d() {}\nclass E {}\nconst { f, g } = h;",
        ),
    );
    for name in ["a", "b", "c", "d", "E", "f", "g"] {
        assert!(
            facts.source_declarations.contains(name),
            "source declaration `{name}` missing: {facts:?}"
        );
    }
    // `h` is undeclared here (the destructure source) → a FREE reference, not a
    // declaration.
    assert!(!facts.source_declarations.contains("h"));
    assert!(facts.free_references.contains("h"));
}

#[test]
fn captures_import_locals_as_declarations_and_roots() {
    let facts = facts_for(None, Some("import Foo from './x.js';\nlet used = Foo;"));
    assert!(
        facts.source_declarations.contains("Foo"),
        "import local `Foo` must be a declaration: {facts:?}"
    );
    assert!(
        facts.top_level_roots.contains("Foo"),
        "import local `Foo` must be a top-level root: {facts:?}"
    );
    // The import local is BOUND, so a reference to it is not free.
    assert!(!facts.free_references.contains("Foo"));
}

#[test]
fn distinguishes_a_store_base_from_its_synthesized_accessor() {
    // The authored-vs-synthesized distinction: a `const Foo = writable(0)` store
    // declares the base `Foo`; a source `$Foo` READ is a free reference; the
    // synthesized `$Foo` binding is NOT a source-form declaration.
    let facts = facts_for(None, Some("const Foo = writable(0);\nconst x = $Foo;"));
    assert!(facts.source_declarations.contains("Foo"));
    assert!(
        !facts.source_declarations.contains("$Foo"),
        "the synthesized `$Foo` accessor is not a source-form declaration: {facts:?}"
    );
    assert!(
        facts.free_references.contains("$Foo"),
        "a source `$Foo` read is a free reference: {facts:?}"
    );
    // When `$Foo` is NEVER referenced, it appears NOWHERE.
    let inert = facts_for(None, Some("const Bar = 1;\nconst y = Bar;"));
    assert!(!inert.source_declarations.contains("$Bar"));
    assert!(!inert.free_references.contains("$Bar"));
    assert!(!inert.name_conflicts().contains("$Bar"));
}

#[test]
fn captures_nested_declarations_but_excludes_them_from_roots() {
    let facts = facts_for(
        None,
        Some("function outer() { const nested = 1; return nested; }"),
    );
    assert!(facts.source_declarations.contains("outer"));
    assert!(
        facts.source_declarations.contains("nested"),
        "a nested declaration is a source-form declaration: {facts:?}"
    );
    assert!(facts.top_level_roots.contains("outer"));
    assert!(
        !facts.top_level_roots.contains("nested"),
        "a nested declaration is NOT a top-level root: {facts:?}"
    );
    // `nested` is bound inside `outer`, so it is not a free reference.
    assert!(!facts.free_references.contains("nested"));
}

#[test]
fn captures_free_references_excluding_bound_locals() {
    let facts = facts_for(
        None,
        Some("const y = String(x);\nfunction f(p) { return p + z; }"),
    );
    assert!(facts.free_references.contains("String"));
    assert!(facts.free_references.contains("x"));
    assert!(facts.free_references.contains("z"));
    // `p` is a bound parameter, `y`/`f` are declarations — none are free.
    assert!(!facts.free_references.contains("p"));
    assert!(!facts.free_references.contains("y"));
    assert!(!facts.free_references.contains("f"));
}

#[test]
fn captures_exported_declarations() {
    // Verter behavior: every EXPORTED top-level value declaration binds a name the
    // component-function name deconfliction reserves. The binder must unwrap `export`, so
    // `export let/const/var/function/class` land in BOTH the deconfliction set and the
    // declared roots; a module-scope export is a module-top declaration inherited by the
    // instance scope. (Cross-checked against svelte@5.56.3: `export let Foo` + name `Foo`
    // emits `Foo_1`; the `export_const` corpus axis is the authoritative svelte pin.)
    let facts = facts_for(
        Some("export const M = 1;"),
        Some(
            "export let A;\nexport var B;\nexport function C() {}\nexport class D {}\nexport const E = 1;\nexport const { p, q } = obj;",
        ),
    );
    for name in ["A", "B", "C", "D", "E", "M", "p", "q"] {
        assert!(
            facts.name_conflicts().contains(name),
            "exported declaration `{name}` missing from the deconfliction set: {facts:?}"
        );
        assert!(
            facts.declared_roots().contains(name),
            "exported top-level declaration `{name}` missing from the declared roots: {facts:?}"
        );
    }
    // `obj` (the destructure source) is undeclared → a free reference, never a root.
    assert!(facts.free_references.contains("obj"));
    assert!(!facts.declared_roots().contains("obj"));
}

#[test]
fn does_not_reserve_type_only_declarations() {
    // Verter behavior: a `type` alias / `interface` is erased by the projection, so it
    // does NOT reserve the component-function name; the binder must not admit a type-only
    // name as a value declaration — including the `export type` / `export interface`
    // forms. (Cross-checked against svelte@5.56.3: svelte COMPILES + erases these — name
    // `Foo` + `type Foo` stays a bare `Foo`.)
    let facts = facts_for(
        None,
        Some(
            "type Foo = number;\ninterface Bar { n: number }\nexport type Baz = string;\nexport interface Qux { m: number }",
        ),
    );
    for name in ["Foo", "Bar", "Baz", "Qux"] {
        assert!(
            !facts.name_conflicts().contains(name),
            "type-only declaration `{name}` must not reserve the component name: {facts:?}"
        );
        assert!(
            !facts.declared_roots().contains(name),
            "type-only declaration `{name}` is not a value root: {facts:?}"
        );
    }
}

#[test]
fn template_declarations_enter_conflicts_but_not_roots() {
    // The authored template locals (each / await / snippet name / slot `let:` /
    // `{@const}`) contribute to the source-form declaration set (so the component
    // name deconflicts against them) but are NOT top-level script roots.
    let alloc = Allocator::default();
    let imports = classify_script_imports_from_sources(None, None);
    let mut template_decls = FxHashSet::default();
    template_decls.insert("item".to_string());
    template_decls.insert("Snip".to_string());
    let exprs = ExprArena::new();
    let facts = build_component_scope_facts(&alloc, None, None, &imports, &template_decls, &exprs)
        .expect("clean scripts produce facts");
    for name in ["item", "Snip"] {
        assert!(
            facts.name_conflicts().contains(name),
            "authored template local `{name}` must reserve the component name: {facts:?}"
        );
        assert!(
            !facts.declared_roots().contains(name),
            "an authored template local `{name}` is not a top-level script root: {facts:?}"
        );
    }
}

#[test]
fn preserves_module_to_instance_topology() {
    // A module-declared name referenced from the instance script is BOUND by the
    // parent (module) frame — not a free reference — while remaining a declaration.
    let facts = facts_for(
        Some("const shared = 1;"),
        Some("const local = log(shared);"),
    );
    assert!(facts.source_declarations.contains("shared"));
    assert!(
        !facts.free_references.contains("shared"),
        "an instance reference to a module declaration is bound, not free: {facts:?}"
    );
    // `log` is a genuine free reference; `local` is a declaration.
    assert!(facts.free_references.contains("log"));
    assert!(facts.source_declarations.contains("local"));
}

#[test]
fn does_not_reserve_a_typescript_enum_declaration() {
    // BUCKET 2 (svelte HARD-ERRORS): svelte@5.56.3 REJECTS an `enum` inside a `<script>`
    // (a hard compile error — it emits NO component and NO name), so name-parity is
    // VACUOUS — NOT a svelte-parity claim. Verter behavior (DEFENSIVE): the scope-view
    // projection erases the `TSEnumDeclaration`, so the enum NAME never binds. This keeps
    // the projection-erased constructs uniformly NON-reserving (`type` / `interface`,
    // which svelte compiles + erases, and the enum, which svelte rejects).
    let facts = facts_for(None, Some("enum Foo { A, B }"));
    assert!(
        !facts.name_conflicts().contains("Foo"),
        "a TS enum must not reserve the component name: {facts:?}"
    );
    assert!(
        !facts.declared_roots().contains("Foo"),
        "a TS enum is not a value root: {facts:?}"
    );
}

#[test]
fn refuses_a_torn_script_rather_than_fabricating_facts() {
    // FAIL-CLOSED: a PRESENT script that does not parse cleanly yields a refusal
    // (`Err(slot)` naming the failing script), never partial facts — a fabricated,
    // un-deconflicted component name would emit broken JS.
    let alloc = Allocator::default();
    let template_decls = FxHashSet::default();
    let exprs = ExprArena::new();
    let torn = "const s = \"unterminated;";
    // A torn INSTANCE script refuses, naming the instance slot.
    let imports = classify_script_imports_from_sources(None, Some(torn));
    assert_eq!(
        build_component_scope_facts(&alloc, None, Some(torn), &imports, &template_decls, &exprs)
            .err(),
        Some(UserImportSlot::Instance),
        "a torn instance script must refuse with the instance slot, not fabricate facts"
    );
    // A torn MODULE script refuses, naming the module slot (span precision).
    let imports_mod = classify_script_imports_from_sources(Some(torn), None);
    assert_eq!(
        build_component_scope_facts(
            &alloc,
            Some(torn),
            None,
            &imports_mod,
            &template_decls,
            &exprs
        )
        .err(),
        Some(UserImportSlot::Module),
        "a torn module script must refuse with the module slot"
    );
    // A CLEAN present script still produces facts.
    let clean = "const a = 1;";
    let imports_ok = classify_script_imports_from_sources(None, Some(clean));
    assert!(
        build_component_scope_facts(
            &alloc,
            None,
            Some(clean),
            &imports_ok,
            &template_decls,
            &exprs
        )
        .is_ok(),
        "a clean script must produce facts"
    );
    // An ABSENT script is not a torn parse — no scripts still produces (empty) facts.
    let imports_none = classify_script_imports_from_sources(None, None);
    assert!(
        build_component_scope_facts(&alloc, None, None, &imports_none, &template_decls, &exprs)
            .is_ok(),
        "an absent script is not a torn parse"
    );
}

#[test]
fn reserves_a_class_expression_id() {
    // Verter behavior: a class-EXPRESSION binding `const x = class Foo {}` introduces
    // `Foo` in the class-expression's own scope, and the authoritative OXC scope tree
    // captures the class-expression id at every nesting level, so a component name `Foo`
    // deconflicts to `Foo_1`. (Cross-checked against svelte@5.56.3: svelte likewise
    // reserves the class-expression id — corpus axis `class_expression`.)
    let facts = facts_for(None, Some("const x = class Foo {};"));
    assert!(
        facts.name_conflicts().contains("Foo"),
        "a class-expression id must reserve the component name: {facts:?}"
    );
}

#[test]
fn reserves_a_static_block_binding() {
    // Verter behavior: a `static { const Foo = 1 }` initializer block binds `Foo` in the
    // static block's own scope, and the scope tree captures static-block value bindings,
    // so `Foo` reserves. (Cross-checked against svelte@5.56.3: svelte likewise reserves the
    // static-block binding — name `Foo` emits `Foo_1`.)
    let facts = facts_for(None, Some("class C { static { const Foo = 1; } }"));
    assert!(
        facts.name_conflicts().contains("Foo"),
        "a static-block binding must reserve the component name: {facts:?}"
    );
}

#[test]
fn reserves_a_switch_case_declaration() {
    // Verter behavior: a braceless `case` clause shares the switch block scope, so
    // `const Foo = 2` binds there and the scope tree captures it, so `Foo` reserves.
    // (Cross-checked against svelte@5.56.3: svelte likewise reserves the switch-case
    // binding — name `Foo` emits `Foo_1`.)
    let facts = facts_for(None, Some("switch (x) { case 1: const Foo = 2; break; }"));
    assert!(
        facts.name_conflicts().contains("Foo"),
        "a switch-case declaration must reserve the component name: {facts:?}"
    );
}

#[test]
fn reserves_a_module_script_class_expression_id() {
    // The MODULE script's scope tree is analyzed with the same authority: a
    // class-expression id declared in `<script module>` reserves the component
    // name (module-scope coverage, not only the instance scope).
    let facts = facts_for(Some("const w = class Shared {};"), None);
    assert!(
        facts.name_conflicts().contains("Shared"),
        "a module-script class-expression id must reserve the component name: {facts:?}"
    );
}

#[test]
fn does_not_reserve_type_only_references() {
    // A name referenced ONLY in TYPE position (`const y: OnlyType = realValue`) is
    // erased by svelte's TypeScript handling and must NOT reserve the component
    // name; a sibling VALUE reference (`realValue`) still does. The scope tree's
    // free references are filtered to value-space references only.
    let facts = facts_for(None, Some("const y: OnlyType = realValue;"));
    assert!(
        !facts.name_conflicts().contains("OnlyType"),
        "a type-only reference must not reserve the component name: {facts:?}"
    );
    assert!(
        facts.free_references.contains("realValue"),
        "a value-position reference is still a free reference: {facts:?}"
    );
}

#[test]
fn preserves_module_to_instance_topology_under_scope_tree() {
    // Per-script scope trees are joined by removing the module's top-level roots
    // from the instance's unresolved references: an instance reference to a
    // module-declared name is BOUND (removed from the free set) while still
    // reserving the component name; a genuine free reference is retained.
    let facts = facts_for(
        Some("const shared = 1;"),
        Some("const local = log(shared);"),
    );
    assert!(facts.name_conflicts().contains("shared"));
    assert!(
        !facts.free_references.contains("shared"),
        "an instance reference to a module top-level declaration is bound, not free: {facts:?}"
    );
    assert!(facts.free_references.contains("log"));
    assert!(facts.source_declarations.contains("local"));
}

// --- Ambient / runtime-erased exclusion: only runtime-surviving value bindings reserve
// the component name, so Verter's projection erases ambient / type-erased declarations and
// they do not reserve. (Cross-checked against svelte@5.56.3: an ambient
// `declare const/function/class` COMPILES to a bare name; an `enum` is a svelte HARD ERROR
// — Verter erases it DEFENSIVELY, name-parity VACUOUS. The corpus owns svelte parity.) ---

#[test]
fn does_not_reserve_an_ambient_declare_const() {
    // Verter behavior: only runtime-surviving value bindings reserve, so the projection
    // erases an ambient `declare const` and it never reserves the component name.
    // (Cross-checked against svelte@5.56.3: `declare const Foo` + name `Foo` COMPILES to a
    // bare `Foo` — svelte accepts and erases the ambient declaration.)
    let facts = facts_for(None, Some("declare const Foo: number;"));
    assert!(
        !facts.name_conflicts().contains("Foo"),
        "an ambient `declare const` must not reserve the component name: {facts:?}"
    );
    assert!(
        !facts.declared_roots().contains("Foo"),
        "an ambient `declare const` is not a runtime value root: {facts:?}"
    );
}

#[test]
fn does_not_reserve_an_ambient_declare_function() {
    let facts = facts_for(None, Some("declare function Foo(): void;"));
    assert!(
        !facts.name_conflicts().contains("Foo"),
        "an ambient `declare function` must not reserve the component name: {facts:?}"
    );
}

#[test]
fn does_not_reserve_an_ambient_declare_class() {
    let facts = facts_for(None, Some("declare class Foo {}"));
    assert!(
        !facts.name_conflicts().contains("Foo"),
        "an ambient `declare class` must not reserve the component name: {facts:?}"
    );
}

#[test]
fn does_not_reserve_an_ambient_by_context_declare_global_function() {
    // Verter behavior: the projection erases the whole `declare global { … }` statement (a
    // `TSGlobalDeclaration`), so its inner `function GF` never binds and `GF` does not
    // reserve the component name. (Cross-checked against svelte@5.56.3, re-probed via the
    // pinned compiler: this BODILESS `declare global { function GF(): void; }` COMPILES to
    // a bare `GF` — svelte drops the ambient block — so Verter's erased scope AGREES. The
    // svelte outcome is body-SENSITIVE: a BODIED `declare global { function GF() {} }`
    // instead REJECTS with `js_parse_error`. Either way Verter erases the `declare global`
    // block, so `GF` never reserves — a Verter-behavior lock, not a standalone svelte-parity
    // claim; the corpus owns svelte parity.)
    let facts = facts_for(None, Some("declare global { function GF(): void; }"));
    assert!(
        !facts.name_conflicts().contains("GF"),
        "an ambient-by-context `declare global` inner decl must not reserve: {facts:?}"
    );
}

#[test]
fn does_not_reserve_an_enum_member() {
    // BUCKET 2 (svelte HARD-ERRORS): svelte@5.56.3 REJECTS a plain `enum`
    // (`typescript_invalid_feature`) — it emits NO component and NO name, so name-parity is
    // VACUOUS (this is NOT a svelte-parity claim; svelte rejects before naming). This locks
    // Verter's DEFENSIVE scope behavior only: the projection erases the `TSEnumDeclaration`
    // (name AND members), so neither `Member` nor `E` reserves. DISCRIMINATING: a plain
    // (non-ambient) enum exercises the `TSEnumDeclaration` ERASE arm directly (not an
    // ambient-`declare` path); if the projection mis-KEEPS the enum, OXC would bind `Member`
    // as a value enum member and `E` as a value enum — both would (wrongly) reserve.
    let facts = facts_for(None, Some("enum E { Member }"));
    assert!(
        !facts.name_conflicts().contains("Member"),
        "an enum member must not reserve the component name: {facts:?}"
    );
    assert!(
        !facts.name_conflicts().contains("E"),
        "a plain enum declaration must not reserve the component name: {facts:?}"
    );
}

#[test]
fn does_not_reserve_an_ambient_value_merged_with_an_interface() {
    // Verter behavior: a symbol whose only VALUE form is ambient (`declare const X`) merged
    // with a type-only `interface X` is fully erased by the projection, so `X` never
    // reserves; a merged symbol survives ONLY with a concrete non-ambient value
    // declaration. (Cross-checked against svelte@5.56.3: this COMPILES to a bare `X`.)
    let facts = facts_for(
        None,
        Some("declare const X: number;\ninterface X { p: number }"),
    );
    assert!(
        !facts.name_conflicts().contains("X"),
        "an ambient-value + interface merge must not reserve the component name: {facts:?}"
    );
}

#[test]
fn reserves_a_non_ambient_const() {
    // Positive control: a concrete non-ambient value binding still reserves — guards
    // against a blanket ambient/erasure over-exclusion.
    let facts = facts_for(None, Some("const Foo = 1;"));
    assert!(
        facts.name_conflicts().contains("Foo"),
        "a non-ambient `const` must reserve the component name: {facts:?}"
    );
    assert!(
        facts.declared_roots().contains("Foo"),
        "a non-ambient `const` is a runtime value root: {facts:?}"
    );
}

#[test]
fn reserves_a_function_overload_group_once() {
    // Positive control (Verter behavior): bodiless overload signatures + the
    // implementation share ONE runtime binding, so the single merged symbol reserves `f`.
    // (Cross-checked against svelte@5.56.3: `function f(a: number): void; function f(a) {}`
    // + name `f` emits `f_1`.)
    let facts = facts_for(
        None,
        Some("function f(a: number): void;\nfunction f(a: any) { return a; }"),
    );
    assert!(
        facts.name_conflicts().contains("f"),
        "a function overload group must reserve the component name once: {facts:?}"
    );
}

#[test]
fn reserves_a_value_declaration_merged_with_an_interface() {
    // Positive control (Verter behavior): a runtime `const Y` merged with a type-only
    // `interface Y` survives (the non-ambient value declaration wins), so `Y` reserves.
    // (Cross-checked against svelte@5.56.3: `interface Y {…}; const Y = 1` + name `Y` emits
    // `Y_1`.)
    let facts = facts_for(None, Some("interface Y { p: number }\nconst Y = 1;"));
    assert!(
        facts.name_conflicts().contains("Y"),
        "a value declaration merged with an interface must reserve: {facts:?}"
    );
}

// --- Svelte scope-view projection: constructs OXC binds as VALUE but svelte's
// `remove_typescript_nodes ∘ create_scopes` scope view ERASES / treats as scope-inert, so
// they contribute no runtime binding and must NOT reserve. These are the cases the old
// exclusion blocklist diverged on; the positive projection makes them non-reserving.
// (Cross-checked against svelte@5.56.3: each COMPILES to a bare name; the corpus owns the
// svelte pins.) ---

#[test]
fn does_not_reserve_a_lone_bodiless_overload_signature() {
    // A LONE bodiless function-overload signature (`function f(a): void;` with no
    // following implementation) is a type-only declaration (OXC `Function { body: None }`,
    // svelte's `TSDeclareFunction` → `b.empty`); Verter's projection erases it, so it must
    // NOT reserve. (Cross-checked against svelte@5.56.3: it COMPILES to a bare name — corpus
    // axis `function_lone_overload`.) Contrast `reserves_a_function_overload_group_once`,
    // where a bodied implementation follows and the merged symbol DOES reserve.
    let facts = facts_for(None, Some("function f(a: number): void;"));
    assert!(
        !facts.name_conflicts().contains("f"),
        "a lone bodiless overload signature must not reserve the component name: {facts:?}"
    );
    assert!(
        !facts.declared_roots().contains("f"),
        "a lone bodiless overload signature is not a runtime value root: {facts:?}"
    );
}

#[test]
fn does_not_reserve_an_import_equals_binding() {
    // `import X = require("y")` (`TSImportEqualsDeclaration`) is SCOPE-INERT in svelte's
    // scope pass — `create_scopes` declares nothing for it — so Verter's projection erases
    // the statement and `X` does NOT reserve. (Cross-checked against svelte@5.56.3: it
    // COMPILES to a bare `X` — svelte does not reserve the import-equals local.) Although
    // OXC binds `X` as a value import, the projection erases the statement.
    let facts = facts_for(None, Some("import X = require(\"y\");"));
    assert!(
        !facts.name_conflicts().contains("X"),
        "an `import X = require(...)` binding must not reserve the component name: {facts:?}"
    );
    assert!(
        !facts.declared_roots().contains("X"),
        "an `import =` binding is not a runtime value root: {facts:?}"
    );
}

#[test]
fn does_not_reserve_an_export_assignment_reference() {
    // `export = X` (`TSExportAssignment`) is SCOPE-INERT in svelte's scope pass; its `X`
    // operand is neither a declaration nor a counted free reference, so an UNBOUND
    // `export = X` does NOT reserve. (Cross-checked against svelte@5.56.3: it COMPILES to a
    // bare `X`.) Verter's projection erases the whole statement, dropping the phantom `X`
    // value reference OXC records.
    let facts = facts_for(None, Some("export = X;"));
    assert!(
        !facts.name_conflicts().contains("X"),
        "an unbound `export = X` reference must not reserve the component name: {facts:?}"
    );
}

#[test]
fn unwraps_ts_expression_wrappers_to_inner_value_refs() {
    // Verter behavior: the projection UNWRAPS the TS type carrier of an `x as T` /
    // `x satisfies T` / `x!` wrapper to its inner RUNTIME expression, so the inner value
    // reference survives while the type operand does not. (Cross-checked against
    // svelte@5.56.3: svelte likewise erases the type carrier and keeps the inner expression
    // — these COMPILE.) (The `<T>x` angle-bracket `TSTypeAssertion` form is classified
    // UNWRAP for faithfulness but is unparseable under the `SourceType::tsx()` reparse —
    // JSX ambiguity — so it never reaches the projection in practice.)
    let facts = facts_for(
        None,
        Some("const a = outer as Foo;\nconst b = other satisfies Bar;\nconst c = thing!;"),
    );
    for name in ["outer", "other", "thing"] {
        assert!(
            facts.free_references.contains(name),
            "the inner value reference `{name}` of a TS wrapper must survive: {facts:?}"
        );
    }
    for ty in ["Foo", "Bar"] {
        assert!(
            !facts.name_conflicts().contains(ty),
            "the type operand `{ty}` of a TS wrapper must not reserve: {facts:?}"
        );
    }
}

// --- Class-member scope-view projection (svelte's ClassBody / MethodDefinition /
// PropertyDefinition / TSParameterProperty handlers). The projection recurses THROUGH
// kept class bodies, so it must ERASE svelte's erased TS members BEFORE binding — else
// OXC binds abstract-method params, visits computed keys of `declare` fields, and binds
// ctor param-properties. (svelte parity for these is owned by the name-parity corpus +
// conformance module; the comments below characterize Verter behavior.) ---

#[test]
fn does_not_reserve_an_abstract_method_parameter() {
    // Verter behavior: the projection erases an abstract method (svelte's `MethodDefinition`
    // handler → `b.empty`), so its parameters are never bound and do NOT reserve.
    // (Cross-checked against svelte@5.56.3: `abstract class A { abstract m(X): void }` +
    // name `X` COMPILES to a bare `X`.) OXC keeps the abstract method
    // (`MethodDefinitionType::TSAbstractMethodDefinition`) and binds its param `X` in the
    // method scope unless the projection erases the member.
    let facts = facts_for(
        None,
        Some("abstract class A { abstract m(X: number): void; }"),
    );
    assert!(
        !facts.name_conflicts().contains("X"),
        "an abstract-method parameter must not reserve the component name: {facts:?}"
    );
    // Control: the class NAME is a real binding and still reserves.
    assert!(
        facts.name_conflicts().contains("A"),
        "the class name must still reserve: {facts:?}"
    );
}

#[test]
fn does_not_reserve_a_declare_field_computed_key() {
    // Verter behavior: the projection drops `declare` property definitions (svelte's
    // `ClassBody` handler), so a computed key `[X]` of a `declare` field is never visited
    // and does NOT reserve. (Cross-checked against svelte@5.56.3: `class A { declare [X]:
    // number }` with `X` undeclared + name `X` COMPILES to a bare `X`.) OXC keeps the
    // `declare` field and visits its computed key `X` as a value reference unless the
    // projection erases the member.
    let facts = facts_for(None, Some("class A { declare [X]: number; }"));
    assert!(
        !facts.name_conflicts().contains("X"),
        "a `declare` field computed key must not reserve the component name: {facts:?}"
    );
}

#[test]
fn defensively_erases_a_constructor_parameter_property() {
    // BUCKET 2 (svelte HARD-ERRORS): a ctor param-property (`constructor(public X)`) is a
    // svelte `typescript_invalid_feature` reject — svelte emits NO component (no name), so
    // name-parity is VACUOUS. This locks Verter's DEFENSIVE scope behavior only: the
    // projection drops the param-property name (via `formal_parameter_is_scope_erased`), so
    // if the reject is ever bypassed the name is not fabricated. Verter's own reject-parity
    // (rejecting like svelte) is the PRE-EXISTING cat-4 debt — NOT asserted here. OXC models
    // the param-property as a `FormalParameter` with an `accessibility` modifier.
    let facts = facts_for(None, Some("class A { constructor(public X: number) {} }"));
    assert!(
        !facts.name_conflicts().contains("X"),
        "the projection must defensively drop a ctor param-property name: {facts:?}"
    );
}

#[test]
fn defensively_erases_an_accessor_field() {
    // BUCKET 2 (svelte HARD-ERRORS): an `accessor` field is a svelte
    // `typescript_invalid_feature` reject — no component, no name, so name-parity is
    // VACUOUS. This locks Verter's DEFENSIVE behavior: the projection erases the
    // `AccessorProperty` (both subtypes), so a computed key `[X]` of an accessor field is
    // never visited and does not reserve. If NOT erased, `[X]` would bind `X`.
    let facts = facts_for(None, Some("class A { accessor [X] = 1; }"));
    assert!(
        !facts.name_conflicts().contains("X"),
        "the projection must defensively erase an accessor field (computed key unvisited): {facts:?}"
    );
    // Control: the class NAME still reserves (only the accessor member is erased).
    assert!(
        facts.name_conflicts().contains("A"),
        "the class name must still reserve: {facts:?}"
    );
}

#[test]
fn fail_closes_on_a_tsx_ambiguous_angle_bracket_type_assertion() {
    // BUCKET 3 (svelte COMPILES, Verter CANNOT PARSE): the angle-bracket assertion `<T>x`
    // is a real svelte-accepted `TSTypeAssertion` that RESERVES its inner value ref — but
    // the sanctioned shared `reparse_module` uses `SourceType::tsx()`, under which `<T>x`
    // is JSX and fails to parse. Verter therefore FAIL-CLOSES the whole component (a
    // pre-existing tsx-ambiguity limitation of the shared IDE parser — dialect-aware
    // reparse is out of scope; the classification's `TSTypeAssertion → unwrap` arm is
    // kept for exhaustiveness but is UNREACHABLE here). What is asserted here is
    // Verter's fail-closed refusal — NOT name-parity with svelte's reserved `X`. Verter
    // never mis-emits: it refuses.
    let alloc = Allocator::default();
    let src = "const x = <Foo>realX;";
    let imports = classify_script_imports_from_sources(None, Some(src));
    let template_decls = FxHashSet::default();
    let exprs = ExprArena::new();
    assert!(
        build_component_scope_facts(&alloc, None, Some(src), &imports, &template_decls, &exprs)
            .is_err(),
        "an angle-bracket type assertion must fail closed under the tsx reparse, not fabricate facts"
    );
}

#[test]
fn reserves_a_normal_class_member_binding() {
    // Controls: NORMAL (non-abstract, non-declare) members are KEPT and recursed, so a
    // normal method's params/locals, a computed key of a KEPT member, a plain ctor
    // param, and a `static { … }` block binding all still reserve.
    let normal_method = facts_for(
        None,
        Some("class A { m(X) { const L = 1; return X + L; } }"),
    );
    assert!(
        normal_method.name_conflicts().contains("X"),
        "a normal method param must reserve: {normal_method:?}"
    );
    assert!(
        normal_method.name_conflicts().contains("L"),
        "a normal method local must reserve: {normal_method:?}"
    );

    let computed_key = facts_for(None, Some("class A { [X]() {} }"));
    assert!(
        computed_key.name_conflicts().contains("X"),
        "a computed key of a KEPT member is a real reference and must reserve: {computed_key:?}"
    );

    let ctor_param = facts_for(None, Some("class A { constructor(X) { this.x = X; } }"));
    assert!(
        ctor_param.name_conflicts().contains("X"),
        "a plain (unmodified) ctor param must reserve: {ctor_param:?}"
    );
}

#[test]
fn does_not_leak_type_position_names_at_any_level() {
    // svelte's universal `_` handler deletes every node's `typeAnnotation` /
    // `typeParameters` / `typeArguments` / `returnType`, so NO type-position name enters
    // its scope. Verter achieves the SAME net scope via the VALUE-POSITION reference/
    // symbol filter: a plain type reference, a `typeof X` value-as-type reference, a type
    // parameter binding, and a return-type reference are all excluded — at statement,
    // class-member, and parameter levels. (If the value-position filter regressed, these
    // type-position names would leak and reserve.)
    let cases: &[(&str, &str)] = &[
        ("plain type ref", "const y: OnlyType = realX;"),
        ("typeof in var type", "const y: typeof TX = realX;"),
        (
            "typeof in kept method param type",
            "class A { m(a: typeof TX) { return a; } }",
        ),
        (
            "typeof in abstract prop type",
            "abstract class A { abstract p: typeof TX; }",
        ),
        (
            "type parameter binding",
            "function f<TParam>(a) { return a; }",
        ),
        ("return type ref", "function g(): RetType { return realX; }"),
        (
            "this-param type",
            "function h(this: ThisT, a) { return a; }",
        ),
    ];
    for (label, src) in cases {
        let facts = facts_for(None, Some(src));
        for leaked in ["OnlyType", "TX", "TParam", "RetType", "ThisT"] {
            assert!(
                !facts.name_conflicts().contains(leaked),
                "type-position name `{leaked}` leaked in `{label}` (`{src}`): {facts:?}"
            );
        }
    }
    // Positive control: the VALUE-position siblings in the same sources DO reserve.
    assert!(facts_for(None, Some("const y: typeof TX = realX;"))
        .name_conflicts()
        .contains("realX"));
    assert!(
        facts_for(None, Some("function g(): RetType { return realX; }"))
            .name_conflicts()
            .contains("realX")
    );
}

#[test]
fn does_not_bind_a_this_parameter() {
    // Verter behavior: OXC never binds `this` as a value symbol and the value-position
    // filter drops its type, so a `this` param is a no-op for scope and does NOT reserve.
    // (svelte's `remove_this_param` likewise drops a leading `this` parameter.)
    // (Cross-checked against svelte@5.56.3: `function f(this: X, a)` + name `X` COMPILES to
    // a bare `X`; the real param `a` still reserves.)
    let facts = facts_for(None, Some("function f(this: ThisType, a) { return a; }"));
    assert!(
        !facts.name_conflicts().contains("ThisType"),
        "a `this`-param type must not reserve: {facts:?}"
    );
    assert!(
        !facts.name_conflicts().contains("this"),
        "a `this` param is not a value binding: {facts:?}"
    );
    assert!(
        facts.name_conflicts().contains("a"),
        "the real param `a` must reserve: {facts:?}"
    );
}

#[test]
fn does_reserve_a_referenced_ambient_declare() {
    // Verter behavior: an ambient `declare const Foo` is erased (emits no binding), so a
    // later VALUE reference to `Foo` is UNBOUND → a free reference → it reserves the
    // component name. (Cross-checked against svelte@5.56.3: `declare const Foo;
    // console.log(Foo)` + name `Foo` emits `Foo_1`.) This is the dual of
    // `does_not_reserve_an_ambient_declare_const`: the DECLARATION is erased but a real
    // value REFERENCE to the same name still counts.
    let facts = facts_for(None, Some("declare const Foo: number;\nconsole.log(Foo);"));
    assert!(
        facts.free_references.contains("Foo"),
        "a value reference to an erased ambient declare is a free reference: {facts:?}"
    );
    assert!(
        facts.name_conflicts().contains("Foo"),
        "a referenced ambient declare must reserve via its free reference: {facts:?}"
    );
    // And it is NOT a declared root (the declaration was erased).
    assert!(
        !facts.declared_roots().contains("Foo"),
        "an erased ambient declare is not a runtime value root even when referenced: {facts:?}"
    );
}

#[test]
fn projection_unwraps_ts_wrappers_to_bare_inner_ast() {
    // DISCRIMINATING AST-shape assertion (not via bound symbol names): after projection
    // the `as` / `satisfies` / non-null / instantiation wrapper NODES are GONE and the
    // inner runtime expression remains. If the unwrap regressed, a wrapper node survives.
    use oxc_ast::ast::Expression;
    use oxc_ast_visit::{walk, Visit};

    #[derive(Default)]
    struct WrapperScan {
        wrappers: usize,
        idents: Vec<String>,
    }
    impl<'a> Visit<'a> for WrapperScan {
        fn visit_expression(&mut self, expr: &Expression<'a>) {
            match expr {
                Expression::TSAsExpression(_)
                | Expression::TSSatisfiesExpression(_)
                | Expression::TSTypeAssertion(_)
                | Expression::TSNonNullExpression(_)
                | Expression::TSInstantiationExpression(_) => self.wrappers += 1,
                Expression::Identifier(id) => self.idents.push(id.name.to_string()),
                _ => {}
            }
            walk::walk_expression(self, expr);
        }
    }

    let alloc = Allocator::default();
    let program = super::project_source_for_test(
        &alloc,
        "const a = inner as Foo;\nconst b = other satisfies Bar;\nconst c = thing!;\nconst d = generic<Ty>;",
    )
    .expect("projection succeeds on a clean script");
    let mut scan = WrapperScan::default();
    scan.visit_program(&program);
    assert_eq!(
        scan.wrappers, 0,
        "every TS expression wrapper must be unwrapped out of the projected AST (found {})",
        scan.wrappers
    );
    for inner in ["inner", "other", "thing", "generic"] {
        assert!(
            scan.idents.iter().any(|n| n == inner),
            "the inner runtime identifier `{inner}` must survive unwrapping: {:?}",
            scan.idents
        );
    }
}
