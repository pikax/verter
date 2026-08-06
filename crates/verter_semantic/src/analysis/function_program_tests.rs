//! @ai-generated - `FunctionProgramIndex` discovery, inventory, and
//! `flow_body_stable_hash` discrimination tests.

use super::*;
use crate::analysis::top_level_owners::TopLevelOwnerTable;
use crate::facts::SymbolSpace;
use verter_type_expr::facts::FunctionPartIdentity;

fn index_of(source: &str) -> FunctionProgramIndex {
    let allocator = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::ts();
    let ret = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    assert!(
        ret.errors.is_empty(),
        "fixture must parse: {:?}",
        ret.errors
    );
    let owners = TopLevelOwnerTable::ordinary_file(ret.program.body.len());
    build_function_program_index(&ret.program, source, &owners)
}

fn hash_of(source: &str, name: &str) -> crate::analysis::types::Hash16 {
    let index = index_of(source);
    let entry = index
        .entries
        .iter()
        .find(|entry| entry.key.declaration.name.as_ref() == name)
        .unwrap_or_else(|| panic!("{name} must be indexed"));
    entry.flow_body_stable_hash
}

fn entry_of<'a>(index: &'a FunctionProgramIndex, name: &str) -> &'a FunctionProgramEntry {
    index
        .entries
        .iter()
        .find(|entry| entry.key.declaration.name.as_ref() == name)
        .unwrap_or_else(|| panic!("{name} must be indexed"))
}

fn member_entry_of<'a>(
    index: &'a FunctionProgramIndex,
    class_name: &str,
    ordinal: u32,
) -> &'a FunctionProgramEntry {
    index
        .entries
        .iter()
        .find(|entry| {
            entry.key.declaration.name.as_ref() == class_name
                && matches!(&entry.key.part, FunctionPartIdentity::Member { member_path } if member_path.contains(&ordinal))
        })
        .unwrap_or_else(|| panic!("{class_name} member {ordinal} must be indexed"))
}

#[test]
fn function_program_indexes_declarations_initializers_members_and_namespaces() {
    let index = index_of(
        r#"
export function decl(flag: boolean): number { return 1; }
export const arrow = (x: string) => x.length;
export const fnExpr = function () { return "e"; };
export class Klass {
  inst() { return 1; }
  static stat() { return "s"; }
  overloaded(a: string): void;
  overloaded(a: number) { return a; }
}
export const obj = {
  method() { return 1; },
  get accessor() { return 2; },
};
namespace Ns {
  export function inner() { return 1; }
}
"#,
    );

    let decl = entry_of(&index, "decl");
    assert!(matches!(
        decl.key.part,
        FunctionPartIdentity::DeclarationBody
    ));
    assert_eq!(decl.key.overload_ordinal, 0);
    assert_eq!(decl.params.len(), 1);
    assert!(decl.params[0].has_ts_annotation);
    assert_eq!(decl.return_sites.len(), 1);

    let arrow = entry_of(&index, "arrow");
    assert!(matches!(arrow.key.part, FunctionPartIdentity::Initializer));
    assert_eq!(arrow.key.declaration.space, SymbolSpace::Value);

    let fn_expr = entry_of(&index, "fnExpr");
    assert!(matches!(
        fn_expr.key.part,
        FunctionPartIdentity::Initializer
    ));

    let inst = member_entry_of(&index, "Klass", 0);
    assert!(matches!(inst.key.part, FunctionPartIdentity::Member { .. }));
    let stat = member_entry_of(&index, "Klass", 1);
    assert!(matches!(stat.key.part, FunctionPartIdentity::Member { .. }));

    // The bodiless overload consumes ordinal 0; the implementation is ordinal 1.
    let overloaded_impl = member_entry_of(&index, "Klass", 3);
    assert_eq!(overloaded_impl.key.overload_ordinal, 1);

    let object_members: Vec<_> = index
        .entries
        .iter()
        .filter(|entry| entry.key.declaration.name.as_ref() == "obj")
        .collect();
    assert_eq!(object_members.len(), 2, "method + accessor both index");

    let inner = entry_of(&index, "Ns.inner");
    assert!(matches!(
        inner.key.part,
        FunctionPartIdentity::DeclarationBody
    ));
}

#[test]
fn function_program_inventory_records_returns_loops_and_direct_calls() {
    let index = index_of(
        r#"
export function selfRec(n: number) {
  if (n <= 0) return 0;
  return selfRec(n - 1);
}
export function callsOther() {
  return selfRec(1);
}
export function loopTransparent() {
  for (let i = 0; i < 3; i++) console.log(i);
  while (false) {}
  return 1;
}
export function loopUnsupported(n: number) {
  while (n > 0) { return n; }
  return 0;
}
export function switchUnsupported(x: number) {
  switch (x) { case 1: return "a"; default: return "b"; }
}
export function bareReturn(flag: boolean) {
  if (flag) return;
}
export function memberCall() {
  return Math.random();
}
"#,
    );

    let self_rec = entry_of(&index, "selfRec");
    assert_eq!(self_rec.return_sites.len(), 2);
    assert!(
        self_rec
            .direct_calls
            .iter()
            .any(|call| call.target.declaration.name.as_ref() == "selfRec"),
        "the self-call is an exact direct call: {:?}",
        self_rec.direct_calls
    );

    let calls_other = entry_of(&index, "callsOther");
    assert!(
        calls_other
            .direct_calls
            .iter()
            .any(|call| call.target.declaration.name.as_ref() == "selfRec"),
        "the local call resolves to the exact same-file target"
    );

    let transparent = entry_of(&index, "loopTransparent");
    let loops: Vec<_> = transparent
        .control
        .iter()
        .filter(|region| region.kind == FunctionControlKind::Loop)
        .collect();
    assert_eq!(loops.len(), 2);
    assert!(
        loops.iter().all(|region| !region.has_return),
        "return-free loops stay fall-through transparent"
    );

    let unsupported = entry_of(&index, "loopUnsupported");
    let loop_region = unsupported
        .control
        .iter()
        .find(|region| region.kind == FunctionControlKind::Loop)
        .expect("loop region");
    assert!(loop_region.has_return, "a return-bearing loop is marked");

    let switch = entry_of(&index, "switchUnsupported");
    assert!(switch
        .control
        .iter()
        .any(|region| region.kind == FunctionControlKind::Switch && region.has_return));

    let bare = entry_of(&index, "bareReturn");
    assert_eq!(bare.return_sites.len(), 1);
    assert!(!bare.return_sites[0].has_argument);

    let member_call = entry_of(&index, "memberCall");
    assert!(
        member_call.direct_calls.is_empty(),
        "a static member call is never an exact direct call"
    );
    assert!(
        member_call
            .effects
            .iter()
            .any(|effect| matches!(&effect.callee, FunctionEffectCallee::StaticMember(path) if path.len() == 2))
    );
}

#[test]
fn flow_body_hash_ignores_binding_renames_and_cosmetic_edits() {
    let base = r#"
export function flow(input: number) {
  const local = input + 1;
  const doubled = local * 2;
  return doubled;
}
"#;
    let renamed = r#"
export function flow(source: number) {
  const temp = source + 1;
  const twice = temp * 2;
  return twice;
}
"#;
    let cosmetic = r#"
export   function   flow(input: number) {
  // an ordinary comment
  const   local   =   input + 1;
  const doubled = local * 2;   /* trailing */
  return doubled;
}
"#;
    assert_eq!(
        hash_of(base, "flow"),
        hash_of(renamed, "flow"),
        "alpha-normalization: binding/reference renames keep the hash"
    );
    assert_eq!(
        hash_of(base, "flow"),
        hash_of(cosmetic, "flow"),
        "whitespace and ordinary comments are cosmetic"
    );
}

#[test]
fn flow_body_hash_discriminates_observable_edits() {
    let base = r#"
export function flow(flag: boolean) {
  const out = { kind: "a", count: 1 };
  if (flag) return out;
  return { kind: "b", count: 2 };
}
"#;
    let cases: &[(&str, &str)] = &[
        (
            "literal edit",
            r#"
export function flow(flag: boolean) {
  const out = { kind: "a", count: 1 };
  if (flag) return out;
  return { kind: "b", count: 3 };
}
"#,
        ),
        (
            "property key rename",
            r#"
export function flow(flag: boolean) {
  const out = { kind: "a", count: 1 };
  if (flag) return out;
  return { kind: "b", total: 2 };
}
"#,
        ),
        (
            "operator edit",
            r#"
export function flow(flag: boolean) {
  const out = { kind: "a", count: 1 };
  if (!flag) return out;
  return { kind: "b", count: 2 };
}
"#,
        ),
        (
            "control edit",
            r#"
export function flow(flag: boolean) {
  const out = { kind: "a", count: 1 };
  if (flag) { if (out.count > 0) return out; }
  return { kind: "b", count: 2 };
}
"#,
        ),
        (
            "statement reorder",
            r#"
export function flow(flag: boolean) {
  if (flag) return { kind: "a", count: 1 };
  const out = { kind: "a", count: 1 };
  return { kind: "b", count: 2 };
}
"#,
        ),
    ];
    for (name, edited) in cases {
        assert_ne!(
            hash_of(base, "flow"),
            hash_of(edited, "flow"),
            "{name} must change the hash"
        );
    }
}

#[test]
fn flow_body_hash_discriminates_parameter_annotation_and_default_edits() {
    // A parameter annotation or default-initializer edit is whole-body
    // identity (the served parameter type lowers from it).
    let annotated = "export function flow(x: string) { return x; }";
    let reannotated = "export function flow(x: number) { return x; }";
    assert_ne!(
        hash_of(annotated, "flow"),
        hash_of(reannotated, "flow"),
        "a parameter annotation edit must change the hash"
    );
    let defaulted = "export function flow(x = 1) { return x; }";
    let redefaulted = "export function flow(x = 2) { return x; }";
    assert_ne!(
        hash_of(defaulted, "flow"),
        hash_of(redefaulted, "flow"),
        "a default-initializer edit must change the hash"
    );
    let defaulted_vs_not = "export function flow(x: number) { return x; }";
    assert_ne!(
        hash_of(defaulted_vs_not, "flow"),
        hash_of(defaulted, "flow"),
        "adding a default initializer changes the hash"
    );
}

#[test]
fn flow_body_hash_discriminates_shorthand_and_computed_keys() {
    let shorthand = "export function flow() { const k = 1; return { k }; }";
    let explicit = "export function flow() { const k = 1; return { k: k }; }";
    let computed = "export function flow() { const k = 1; return { [\"k\"]: k }; }";
    assert_ne!(hash_of(shorthand, "flow"), hash_of(explicit, "flow"));
    assert_ne!(hash_of(explicit, "flow"), hash_of(computed, "flow"));
}

#[test]
fn flow_body_hash_ignores_cosmetic_tag_descriptions() {
    // Only the typed `{T}` payload is type-affecting: the parameter name
    // and trailing description text never invalidate.
    let base = "/** Doubles.
 * @param {number} x the value
 * @returns {number} doubled
 */
export function flow(x) { return x * 2; }";
    let renamed = "/** Doubles.
 * @param {number} value the value
 * @returns {number} the doubled value
 */
export function flow(x) { return x * 2; }";
    let retyped = "/** Doubles.
 * @param {string} x the value
 * @returns {number} doubled
 */
export function flow(x) { return x * 2; }";
    assert_eq!(
        hash_of(base, "flow"),
        hash_of(renamed, "flow"),
        "parameter-name and description edits are cosmetic"
    );
    assert_ne!(
        hash_of(base, "flow"),
        hash_of(retyped, "flow"),
        "a typed payload edit changes the hash"
    );
}

#[test]
fn flow_body_hash_folds_type_affecting_jsdoc_only() {
    let base = "/** Identity helper.\n * @param {number} x the value\n * @returns {number} doubled\n */\nexport function flow(x) { return x * 2; }";
    let description_edit = "/** Changed description.\n * @param {number} x the value\n * @returns {number} doubled\n */\nexport function flow(x) { return x * 2; }";
    let returns_edit = "/** Identity helper.\n * @param {number} x the value\n * @returns {string} doubled\n */\nexport function flow(x) { return x * 2; }";
    let param_edit = "/** Identity helper.\n * @param {string} x the value\n * @returns {number} doubled\n */\nexport function flow(x) { return x * 2; }";
    assert_eq!(
        hash_of(base, "flow"),
        hash_of(description_edit, "flow"),
        "description text is cosmetic"
    );
    assert_ne!(
        hash_of(base, "flow"),
        hash_of(returns_edit, "flow"),
        "a @returns payload edit is type-affecting"
    );
    assert_ne!(
        hash_of(base, "flow"),
        hash_of(param_edit, "flow"),
        "a @param payload edit is type-affecting"
    );
}

#[test]
fn flow_body_hash_discriminates_free_names_and_annotations() {
    let base =
        "export declare function ext(): number;\nexport function flow(): number { return ext(); }";
    let renamed_callee = "export declare function ext2(): number;\nexport function flow(): number { return ext2(); }";
    assert_ne!(
        hash_of(base, "flow"),
        hash_of(renamed_callee, "flow"),
        "a free callee name is observable"
    );

    let annotated = "export function flow() { const x: number = 1; return x; }";
    let reannotated = "export function flow() { const x: string = 1; return x; }";
    assert_ne!(
        hash_of(annotated, "flow"),
        hash_of(reannotated, "flow"),
        "an authored annotation edit is observable"
    );
}

#[test]
fn flow_body_hash_walks_nested_function_structure() {
    let base = "export function flow() { const g = () => 1; return g(); }";
    let edited = "export function flow() { const g = () => 2; return g(); }";
    assert_ne!(
        hash_of(base, "flow"),
        hash_of(edited, "flow"),
        "a nested function body edit is observable"
    );
    let renamed_nested = "export function flow() { const h = () => 1; return h(); }";
    assert_eq!(
        hash_of(base, "flow"),
        hash_of(renamed_nested, "flow"),
        "a nested binding rename stays alpha-normalized"
    );
}

/// A clause parameter's `first_parameter_occurrence` is the caller's
/// inference oracle: TypeScript takes a declared default ONLY when
/// inference produced no candidate, and inference can produce one only
/// from an argument supplied at a parameter position whose type names
/// the parameter.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`), read through a TWO-STEP assignment error —
/// `const v = <call>; const p: null = v;` — over calls whose default is
/// DIFFERENT from what inference produces.
///
/// The two-step form is load-bearing. A one-step `const p: null = <call>`
/// lets the contextual `null` FEED return-type inference, and reports NO
/// ERROR AT ALL for `occursSecond` / `occursNowhere` / `occursRest()` /
/// `occursShadowed` — five of the seven rows below are unreadable through
/// it. The values were re-derived through the two-step form and all seven
/// are correct.
///
/// ```text
/// occursFirst<T = string>(x: T)                  called (1)   : 1       ← INFERRED, not the `string` default
/// occursSecond<T = number>(a: string, b?: T)     called ("a") : number  ← DEFAULT (ordinal 1 unsupplied)
/// occursNowhere<T = number>(x: string)           called ("a") : number  ← DEFAULT (no occurrence at all)
/// occursRest<T = number>(...xs: T[])             called ()    : number  ← DEFAULT (ordinal 0 unsupplied)
/// occursRest<T = number>(...xs: T[])             called ("a") : "a"     ← INFERRED
/// occursShadowed<T = number>(cb: <T>(y: T) => T) called (…)   : number  ← DEFAULT (the inner clause shadows)
/// occursShadowedCall<T = number>(cb: { <T>(y: T): T })         : number  ← DEFAULT (same, as a call signature)
/// occursShadowedNew<T = number>(cb: { new <T>(y: T): T })      : number  ← DEFAULT (same, as a construct signature)
/// ```
///
/// (The literal rows read `1` / `"a"` rather than `number` / `string`
/// because the probe binds them to a `const`; the rule under test is
/// which SOURCE resolved the parameter, not how the result widened.)
///
/// Mutation recipes: dropping the shadow stack makes `occursShadowed`
/// record ordinal 0 (the substrate would then answer the interim
/// `unknown` where the checker answers the default); dropping either the
/// `TSCallSignatureDeclaration` or the `TSConstructSignatureDeclaration`
/// override flips exactly its own row — a clause declared on a signature
/// inside a type literal masked nothing, so the outer parameter looked
/// like it occurred; recording only the
/// LAST occurrence rather than the smallest flips `occursTwice`;
/// skipping the rest parameter makes `occursRest` record `None`, which
/// would take the default at an argument-BEARING call the checker
/// infers from.
#[test]
fn type_param_occurrence_records_the_smallest_supplying_parameter_ordinal() {
    let index = index_of(
        r#"
export function occursFirst<T = string>(x: T): T { return x; }
export function occursSecond<T = number>(a: string, b?: T): T { return b!; }
export function occursNowhere<T = number>(x: string): T { return null as any; }
export function occursRest<T = number>(...xs: T[]): T { return xs[0]; }
export function occursShadowed<T = number>(cb: <T>(y: T) => T): T { return null as any; }
export function occursShadowedCall<T = number>(cb: { <T>(y: T): T }): T { return null as any; }
export function occursShadowedNew<T = number>(cb: { new <T>(y: T): T }): T { return null as any; }
export function occursTwice<T = number>(a: string, b: T, c: T): T { return b; }
export function occursNested<T = number>(a: string, b: { k: T }): T { return b.k; }
"#,
    );
    let occurrence = |name: &str| {
        entry_of(&index, name)
            .type_parameters
            .iter()
            .find(|param| param.name.as_ref() == "T")
            .unwrap_or_else(|| panic!("{name} declares T"))
            .first_parameter_occurrence
    };

    assert_eq!(occurrence("occursFirst"), Some(0));
    assert_eq!(occurrence("occursSecond"), Some(1));
    assert_eq!(
        occurrence("occursNowhere"),
        None,
        "a parameter naming no formal parameter can never get an inference candidate, \
         so its declared default applies even at an argument-BEARING call"
    );
    assert_eq!(
        occurrence("occursRest"),
        Some(0),
        "a rest parameter occupies its own ordinal and covers every later one"
    );
    assert_eq!(
        occurrence("occursShadowed"),
        None,
        "a nested clause re-declaring the name owns its own subtree — the OUTER T \
         occurs in no parameter type, which is what the checker answers too"
    );
    for name in ["occursShadowedCall", "occursShadowedNew"] {
        assert_eq!(
            occurrence(name),
            None,
            "{name}: a CALL / CONSTRUCT signature inside a type literal declares its \
             own clause exactly as a bare function type does — the outer `T` occurs \
             in no parameter type, and the checker answers the declared `number`"
        );
    }
    assert_eq!(
        occurrence("occursTwice"),
        Some(1),
        "the SMALLEST occurrence is the oracle: a call supplying only `a` reaches \
         neither, a call supplying `a, b` reaches the first"
    );
    assert_eq!(
        occurrence("occursNested"),
        Some(1),
        "occurrence is structural, not top-level: `{{ k: T }}` names T"
    );
}
