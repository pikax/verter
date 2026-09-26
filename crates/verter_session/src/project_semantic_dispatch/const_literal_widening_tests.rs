//! A `const` without an annotation declares the FRESH literal type of its
//! literal initializer, and a read of it widens wherever a bare literal
//! would: `const c = 1; function f() { return c; }` returns `number`, as
//! `return 1` does. An annotation, an assertion or a mutable declaration
//! declares a regular literal type, which never widens.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --declaration
//! --emitDeclarationOnly --strict`, read off the emitted `.d.ts`.

use std::sync::Arc;

use super::checker_probe_lane_tests::{mismatches, mismatches_in, ProbeProject};
use super::*;
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{PrimitiveName, TopLevelOwnerId, TypeExpr};

const SAME_FILE: &str = "\
const mc = 1;
const mcAnn: 1 = 1;
const mcAs = 1 as const;
const mcStr = \"s\";
const mcTrue = true;
const mcObj = { a: 1 };
const mcNeg = -1;
const mcTpl = `t`;
const mcBig = 1n;
declare const mcDecl: 1;
let ml = 1;
const mcCopy = mc;
const mcCopyAnn = mcAnn;
const mcUnion = Math.random() > 0.5 ? 1 : 2;
export function readMc() { return mc; }
export function readMcAnn() { return mcAnn; }
export function readMcAs() { return mcAs; }
export function readMcStr() { return mcStr; }
export function readMcTrue() { return mcTrue; }
export function readMcObjA() { return mcObj.a; }
export function readMcNeg() { return mcNeg; }
export function readMcTpl() { return mcTpl; }
export function readMcBig() { return mcBig; }
export function readMcDecl() { return mcDecl; }
export function readMl() { return ml; }
export function readMcCopy() { return mcCopy; }
export function readMcCopyAnn() { return mcCopyAnn; }
export function readMcUnion() { return mcUnion; }
export function readMcInObj() { return { v: mc }; }
export function readMcInArr() { return [mc]; }
export function readMcCond(b: boolean) { return b ? mc : mcStr; }
export function readMcAnnotated(): 1 { return mc; }
export const arrowMc = () => mc;
export function readLocalConst() { const lc = 1; return lc; }
export function readMcViaLet() { let x = mc; return x; }
export function readMcViaConst() { const x = mc; return x; }
";

/// A read of a module `const` widens as its literal does, in every
/// position a bare literal widens in; a regular literal never widens.
///
/// Measured on TypeScript 7.0.2: `readMc` is `number`, `readMcStr`
/// `string`, `readMcTrue` `boolean`, `readMcNeg` `number`, `readMcTpl`
/// `string`, `readMcBig` `bigint`, `readMcCopy` (a `const` copying `mc`)
/// `number`, `readMcInObj` `{ v: number; }`, `readMcInArr` `number[]`,
/// `arrowMc` `() => number`, `readMcViaLet` and `readMcViaConst` `number`;
/// `readMcUnion` keeps `1 | 2` and `readMcCond` `"s" | 1` (a return widens
/// only a lone literal); `readMcAnn`, `readMcAs`, `readMcDecl`,
/// `readMcCopyAnn` and `readMcAnnotated` stay `1`; `typeof mc` is `1`.
///
/// Mutation: declaring every `const` regular answers `readMc` `1`; dropping
/// the copied `const`'s freshness answers `readMcCopy` `1`; reading no
/// declaration freshness at a member position answers `readMcInObj`
/// `{ v: 1 }`, and at a `const` initializer answers `readMcViaConst` `1`;
/// probing a conditional's test for calls leaves `readMcUnion` a typed miss.
#[test]
fn a_const_read_widens_as_its_literal_does() {
    let failures = mismatches(
        SAME_FILE,
        &[
            ("ReturnType<typeof readMc>", "number"),
            ("ReturnType<typeof readMcAnn>", "1"),
            ("ReturnType<typeof readMcAs>", "1"),
            ("ReturnType<typeof readMcStr>", "string"),
            ("ReturnType<typeof readMcTrue>", "boolean"),
            ("ReturnType<typeof readMcObjA>", "number"),
            ("ReturnType<typeof readMcNeg>", "number"),
            ("ReturnType<typeof readMcTpl>", "string"),
            ("ReturnType<typeof readMcBig>", "bigint"),
            ("ReturnType<typeof readMcDecl>", "1"),
            ("ReturnType<typeof readMl>", "number"),
            ("ReturnType<typeof readMcCopy>", "number"),
            ("ReturnType<typeof readMcCopyAnn>", "1"),
            ("ReturnType<typeof readMcUnion>", "1 | 2"),
            ("ReturnType<typeof readMcInObj>", "{ v: number; }"),
            ("ReturnType<typeof readMcInArr>", "number[]"),
            ("ReturnType<typeof readMcCond>", "\"s\" | 1"),
            ("ReturnType<typeof readMcAnnotated>", "1"),
            ("ReturnType<typeof arrowMc>", "number"),
            ("ReturnType<typeof readLocalConst>", "number"),
            ("ReturnType<typeof readMcViaLet>", "number"),
            ("ReturnType<typeof readMcViaConst>", "number"),
            ("typeof mc", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const DEP: &str = "\
export const dc = 1;
export const dcAnn: 1 = 1;
export const dcUnion = Math.random() > 0.5 ? \"a\" : \"b\";
export namespace NsW { export const nc = 1; }
export const dcParen = (1);
export const dcSat = 1 satisfies number;
export const dcNull = null;
";
const DEP_USE: &str = "\
import { dc, dcAnn, dcUnion, NsW, dcParen, dcSat, dcNull } from './dep';
import * as D from './dep';
export function readDc() { return dc; }
export function readDcAnn() { return dcAnn; }
export function readDcUnion() { return dcUnion; }
export function readDcUnionObj() { return { u: dcUnion }; }
export function readDcUnionLet() { let u = dcUnion; return u; }
export function readNsc() { return NsW.nc; }
export function readDNs() { return D.dc; }
export function readDcParen() { return dcParen; }
export function readDcSat() { return dcSat; }
export function readDcNull() { return dcNull; }
export function readDcArrow() { return () => dc; }
export function readDcCall() { return id(dc); }
function id<T>(x: T): T { return x; }
";

/// The `compilerOptions` of a project with `strictNullChecks` off.
const LOOSE: &str = r#"{ "strict": true, "strictNullChecks": false }"#;

/// An imported `const` widens by its declaration in the module that
/// declares it — read by name, through a namespace it belongs to, or
/// through a namespace import.
///
/// Measured on TypeScript 7.0.2: `readDc`, `readNsc`, `readDNs`,
/// `readDcParen`, `readDcSat` and `readDcCall` (a generic call's argument)
/// are `number`, `readDcArrow` `() => number`; `readDcUnion` keeps
/// `"a" | "b"` while `readDcUnionObj` is `{ u: string; }` and
/// `readDcUnionLet` `string`; `readDcAnn` stays `1`; every row answers
/// alike under both `strictNullChecks` settings, and under `noImplicitAny`
/// on and off. `readDcNull` is `null` with `strictNullChecks`.
///
/// Mutation: resolving a namespace import's member only through a longer
/// qualified name answers `readDNs` `1`; reading no declaration freshness
/// for a call argument answers `readDcCall` `1`.
#[test]
fn an_imported_const_widens_by_its_declaration() {
    let files = [("dep.ts", DEP)];
    for compiler_options in [None, Some(LOOSE)] {
        let mut rows = vec![
            ("ReturnType<typeof readDc>", "number"),
            ("ReturnType<typeof readDcAnn>", "1"),
            ("ReturnType<typeof readDcUnion>", "\"a\" | \"b\""),
            ("ReturnType<typeof readDcUnionObj>", "{ u: string; }"),
            ("ReturnType<typeof readDcUnionLet>", "string"),
            ("ReturnType<typeof readNsc>", "number"),
            ("ReturnType<typeof readDNs>", "number"),
            ("ReturnType<typeof readDcParen>", "number"),
            ("ReturnType<typeof readDcSat>", "number"),
            ("ReturnType<typeof readDcArrow>", "() => number"),
            ("ReturnType<typeof readDcCall>", "number"),
        ];
        if compiler_options.is_none() {
            rows.push(("ReturnType<typeof readDcNull>", "null"));
        }
        let failures = mismatches_in(
            ProbeProject {
                files: &files,
                compiler_options,
                ambient_lib: None,
            },
            DEP_USE,
            &rows,
        );
        assert!(
            failures.is_empty(),
            "{compiler_options:?}:\n{}",
            failures.join("\n")
        );
    }
}

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// One evaluated function's return type and degradation.
fn eval(
    host: &Arc<VerterHost>,
    canonical: &str,
    name: &str,
) -> (TypeExpr, Option<FlowReturnDegradation>) {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(canonical),
            TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(canonical),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    };
    match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key))) {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) => (
            host.project_node_to_type_expr_for_test(result.return_type())
                .unwrap_or_else(|| panic!("{name}: the value did not project")),
            result.degradation(),
        ),
        other => panic!("{name} must produce a value, got {other:?}"),
    }
}

/// An edit that annotates the imported `const` misses the warm read: its
/// declared type is regular from then on.
///
/// Measured on TypeScript 7.0.2: `readDc` is `number` over
/// `export const dc = 1`, and `1` over `export const dc: 1 = 1`.
#[test]
fn an_edit_to_the_const_misses_the_warm_read() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, "/w/dep.ts", "export const dc = 1;\n");
    upsert(
        &host,
        "/w/use.ts",
        "import { dc } from './dep';\nexport function readDc() { return dc; }\n",
    );
    assert_eq!(
        eval(&host, "/w/use.ts", "readDc"),
        (TypeExpr::Primitive(PrimitiveName::Number), None)
    );
    upsert(&host, "/w/dep.ts", "export const dc: 1 = 1;\n");
    assert_eq!(
        eval(&host, "/w/use.ts", "readDc"),
        (
            TypeExpr::Literal(verter_type_expr::LiteralValue::Number(1.0)),
            None
        )
    );
}

const SHADOWING_USE: &str = "\
import { imported } from './dep';
import { dd, ddAnn } from './decl';
declare const cond: boolean;
const c = 1;
const gShadow = \"mod\";
{ const c = \"block\"; }
export function readAfterBlock() { if (cond) { const c = \"inner\"; } return c; }
export function readShadowGlobal() { return gShadow; }
export function readParamShadow(c: string) { return c; }
export function readInnerArrow() { const c = \"local\"; return () => c; }
export function readOuterFromArrow() { return () => c; }
export function readImported() { return imported; }
export function readShadowImport() { const imported = \"x\"; return imported; }
export function readLetCopy() { let x = c; x = 2; return x; }
export function readTernaryConst() { return cond ? c : c; }
export function readAsConst() { return c as 1; }
export function readSpread() { return [...[c]]; }
export function readObjShorthand() { return { c }; }
export function readTpl() { return `${c}`; }
export function readCallArg() { return id(c); }
export function readCallArgPinned() { return id<1>(c); }
function id<T>(x: T): T { return x; }
export const readArrowObj = () => ({ c });
export function readMaybe(b: boolean) { if (b) return c; return undefined; }
export function readDd() { return dd; }
export function readDdAnn() { return ddAnn; }
export function readGd() { return gd; }
export function readGdAnn() { return gdAnn; }
export function readSc() { return sc; }
export function readGdLet() { let x = gd; return x; }
";

/// A module `const` read through a bare reference widens by the
/// declaration the reference names — past a block-scoped or local
/// same-name declaration that does not enclose it, over a global it
/// shadows — and not by one it does not name (a parameter, a local, a
/// copy the frame retypes). A declaration file's `declare const` with a
/// literal initializer, a global `declare const` and a script's `const`
/// widen the same way; an annotated one never does.
///
/// Measured on TypeScript 7.0.2: `readAfterBlock` is `number`,
/// `readShadowGlobal` (a module `const gShadow = "mod"` over a global
/// `var gShadow: boolean`) `string`, `readParamShadow` `string`,
/// `readInnerArrow` `() => string`, `readOuterFromArrow` `() => number`,
/// `readImported` `number`, `readShadowImport` `string`, `readLetCopy`
/// `number`, `readTernaryConst` `number`, `readAsConst` `1`, `readSpread`
/// `number[]`, `readObjShorthand` `{ c: number; }`, `readTpl` `string`,
/// `readCallArg` `number`, `readCallArgPinned` `1`, `readArrowObj`
/// `() => { c: number; }`, `readDd` `number`, `readDdAnn` `1`, `readGd`
/// `number`, `readGdAnn` `2`, `readSc` `number`, `readGdLet` `number`;
/// `readMaybe` is `1 | undefined` with `strictNullChecks` (a return widens
/// only a lone literal) and `number` without it. Every row answers alike
/// under `noImplicitAny` on and off.
///
/// Mutation: reading no declaration freshness for a free read answers
/// `readAfterBlock`, `readShadowGlobal`, `readImported`, `readDd`, `readGd`
/// and `readSc` with their literal, and `readMaybe` without
/// `strictNullChecks` `1`.
#[test]
fn a_bare_const_read_widens_by_the_declaration_it_names() {
    let files = [
        (
            "glob.d.ts",
            "declare var gShadow: boolean;\ndeclare const gd = 2;\ndeclare const gdAnn: 2;\n",
        ),
        ("dep.ts", "export const imported = 5;\n"),
        (
            "decl.d.ts",
            "export declare const dd = 1;\nexport declare const ddAnn: 1;\n",
        ),
        ("script.ts", "const sc = 3;\n"),
    ];
    for (compiler_options, maybe) in [(None, "1 | undefined"), (Some(LOOSE), "number")] {
        let failures = mismatches_in(
            ProbeProject {
                files: &files,
                compiler_options,
                ambient_lib: None,
            },
            SHADOWING_USE,
            &[
                ("ReturnType<typeof readAfterBlock>", "number"),
                ("ReturnType<typeof readShadowGlobal>", "string"),
                ("ReturnType<typeof readParamShadow>", "string"),
                ("ReturnType<typeof readInnerArrow>", "() => string"),
                ("ReturnType<typeof readOuterFromArrow>", "() => number"),
                ("ReturnType<typeof readImported>", "number"),
                ("ReturnType<typeof readShadowImport>", "string"),
                ("ReturnType<typeof readLetCopy>", "number"),
                ("ReturnType<typeof readTernaryConst>", "number"),
                ("ReturnType<typeof readAsConst>", "1"),
                ("ReturnType<typeof readSpread>", "number[]"),
                ("ReturnType<typeof readObjShorthand>", "{ c: number; }"),
                ("ReturnType<typeof readTpl>", "string"),
                ("ReturnType<typeof readCallArg>", "number"),
                ("ReturnType<typeof readCallArgPinned>", "1"),
                ("ReturnType<typeof readArrowObj>", "{ c: number; }"),
                ("ReturnType<typeof readMaybe>", maybe),
                ("ReturnType<typeof readDd>", "number"),
                ("ReturnType<typeof readDdAnn>", "1"),
                ("ReturnType<typeof readGd>", "number"),
                ("ReturnType<typeof readGdAnn>", "2"),
                ("ReturnType<typeof readSc>", "number"),
                ("ReturnType<typeof readGdLet>", "number"),
            ],
        );
        assert!(
            failures.is_empty(),
            "{compiler_options:?}:\n{}",
            failures.join("\n")
        );
    }
}

/// A read of a module `const` decides its widening from the declaration it
/// names alone: the file's other declarations stay unlowered however many
/// it holds.
///
/// Measured on TypeScript 7.0.2: `readK7` is `number`.
///
/// Mutation: finding the declaration by reading every value declaration
/// of the file in turn materializes every `k*` before `k7`.
#[test]
fn a_const_read_materializes_only_the_declaration_it_names() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let mut source = String::new();
    for index in 0..24 {
        source.push_str(&format!("const k{index} = {index};\n"));
    }
    source.push_str("export function readK7() { return k7; }\n");
    upsert(&host, "/w/consts.ts", &source);
    assert_eq!(
        eval(&host, "/w/consts.ts", "readK7"),
        (TypeExpr::Primitive(PrimitiveName::Number), None)
    );
    let indexed = host
        .ensure_indexed_ready_serve("/w/consts.ts")
        .expect("the module is served")
        .indexed;
    let memo = indexed.shallow_state.decl_bodies();
    assert!(memo.value_entry_materialized("k7"), "the read names `k7`");
    for index in (0..24).filter(|index| *index != 7) {
        assert!(
            !memo.value_entry_materialized(&format!("k{index}")),
            "`k{index}` is not read"
        );
    }
    assert!(!memo.whole_env_materialized(), "no whole-file environment");
}

/// A cycle of copies (`const a = b; const b = a`, the checker's TS7022)
/// reads no widening literal, and the widening read terminates.
#[test]
fn a_copy_cycle_reads_no_widening_literal() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(
        &host,
        "/w/cycle.ts",
        "const a = b;\nconst b = a;\nexport function readA() { return a; }\n",
    );
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    assert!(!dispatch.value_read_widens(
        "/w/cycle.ts",
        TopLevelOwnerId::ordinary_file(),
        &["a".to_string()],
    ));
}

const STATIC_DEP: &str = "\
export class KW { static readonly s = 1; static readonly sAnn: 1 = 1; static t = 1; static readonly str = \"s\"; static readonly neg = -1; }
export const dn = null;
export const du = undefined;
export const dnAnn: null = null;
";

const STATIC_USE: &str = "\
import { KW, dn, du, dnAnn } from './dep';
class Local { static readonly ls = 2; static readonly lAs = 2 as const; }
const mn = null;
const mu = undefined;
const mv = void 0;
export function ks() { return KW.s; }
export function ksAnn() { return KW.sAnn; }
export function kt() { return KW.t; }
export function kstr() { return KW.str; }
export function kneg() { return KW.neg; }
export function ksObj() { return { v: KW.s }; }
export function ksLet() { let v = KW.s; return v; }
export function lls() { return Local.ls; }
export function llAs() { return Local.lAs; }
export function rmn() { return mn; }
export function rmu() { return mu; }
export function rmv() { return mv; }
export function rdn() { return dn; }
export function rdu() { return du; }
export function rdnAnn() { return dnAnn; }
export function rmnObj() { return { v: mn }; }
export function rmnLet() { let v = mn; return v; }
export function rmnArr() { return [mn]; }
";

/// A `static readonly` property without an annotation declares the fresh
/// literal type of its literal initializer, as a `const` does: a read of
/// it widens wherever a bare literal widens. With `strictNullChecks` off, a
/// declaration whose initializer is `null`, `undefined` or a `void`
/// expression and that has no annotation declares `any`.
///
/// Measured on TypeScript 7.0.2 (every row alike under `noImplicitAny` on
/// and off): `ks`, `kt`, `kneg`, `ksLet` and `lls` are `number`, `kstr`
/// `string`, `ksObj` `{ v: number; }`, while `ksAnn` stays `1`, `llAs` `2`
/// and `typeof KW.s` is `1`. With `strictNullChecks`, `rmn`, `rdn`,
/// `rdnAnn` and `rmnLet` are `null`, `rmu`, `rmv` and `rdu` `undefined`,
/// `rmnObj` `{ v: null; }` and `rmnArr` `null[]`, `typeof mn` `null`;
/// without it every one of them is `any` (`{ v: any; }`, `any[]`) except
/// `rdnAnn`, which stays `null`.
///
/// Mutation: recording no widening static member answers `ks` `1`;
/// declaring a `null` initializer's type regardless of `strictNullChecks`
/// answers `rmn` `null` without it.
#[test]
fn a_readonly_static_and_a_nullish_declaration_widen_as_the_checker_declares() {
    let files = [("dep.ts", STATIC_DEP)];
    for (compiler_options, strict) in [(None, true), (Some(LOOSE), false)] {
        let pick = |strict_answer: &'static str, loose_answer: &'static str| {
            if strict {
                strict_answer
            } else {
                loose_answer
            }
        };
        let failures = mismatches_in(
            ProbeProject {
                files: &files,
                compiler_options,
                ambient_lib: None,
            },
            STATIC_USE,
            &[
                ("ReturnType<typeof ks>", "number"),
                ("ReturnType<typeof ksAnn>", "1"),
                ("ReturnType<typeof kt>", "number"),
                ("ReturnType<typeof kstr>", "string"),
                ("ReturnType<typeof kneg>", "number"),
                ("ReturnType<typeof ksObj>", "{ v: number; }"),
                ("ReturnType<typeof ksLet>", "number"),
                ("ReturnType<typeof lls>", "number"),
                ("ReturnType<typeof llAs>", "2"),
                ("typeof KW.s", "1"),
                ("ReturnType<typeof rmn>", pick("null", "any")),
                ("ReturnType<typeof rmu>", pick("undefined", "any")),
                ("ReturnType<typeof rmv>", pick("undefined", "any")),
                ("ReturnType<typeof rdn>", pick("null", "any")),
                ("ReturnType<typeof rdu>", pick("undefined", "any")),
                ("ReturnType<typeof rdnAnn>", "null"),
                (
                    "ReturnType<typeof rmnObj>",
                    pick("{ v: null; }", "{ v: any; }"),
                ),
                ("ReturnType<typeof rmnLet>", pick("null", "any")),
                ("ReturnType<typeof rmnArr>", pick("null[]", "any[]")),
                ("typeof mn", pick("null", "any")),
            ],
        );
        assert!(
            failures.is_empty(),
            "{compiler_options:?}:\n{}",
            failures.join("\n")
        );
    }
}

const READONLY_INSTANCE: &str = "\
class K { readonly r = 1; readonly rAnn: 1 = 1; readonly rAs = 1 as const; readonly rs = \"s\"; readonly rn = -1; readonly rc: number; readonly rCtor; constructor() { this.rc = 2; this.rCtor = 3; } }
export function kr(k: K) { return k.r; }
export function krAnn(k: K) { return k.rAnn; }
export function krAs(k: K) { return k.rAs; }
export function krs(k: K) { return k.rs; }
export function krn(k: K) { return k.rn; }
export function krc(k: K) { return k.rc; }
export function krObj(k: K) { return { v: k.r }; }
export function krLet(k: K) { let v = k.r; return v; }
export function tkr() { const x: K['r'] = null as any; return x; }
export function krCtor(k: K) { return k.rCtor; }
";

/// An unannotated `readonly` instance property with a literal initializer
/// declares the fresh literal type, so a read of it widens wherever a bare
/// literal widens; an annotation or a `const` assertion declares a regular
/// literal.
///
/// Measured on TypeScript 7.0.2 (alike on the four `strictNullChecks` ×
/// `noImplicitAny` settings): `kr`, `krn`, `krLet` and `tkr` are `number`,
/// `krs` `string`, `krObj` `{ v: number; }`, while `krAnn` and `krAs` stay
/// `1` and `krc` is `number`.
#[test]
#[ignore = "a read of an unannotated readonly instance property with a literal initializer widens"]
fn a_readonly_instance_literal_widens_as_the_checker_declares() {
    let failures = mismatches(
        READONLY_INSTANCE,
        &[
            ("ReturnType<typeof kr>", "number"),
            ("ReturnType<typeof krAnn>", "1"),
            ("ReturnType<typeof krAs>", "1"),
            ("ReturnType<typeof krs>", "string"),
            ("ReturnType<typeof krn>", "number"),
            ("ReturnType<typeof krc>", "number"),
            ("ReturnType<typeof krObj>", "{ v: number; }"),
            ("ReturnType<typeof krLet>", "number"),
            ("ReturnType<typeof tkr>", "number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A property declared without a type or an initializer takes the type
/// its constructor assigns under `noImplicitAny`, and is the implicit
/// `any` without it.
///
/// Measured on TypeScript 7.0.2: `krCtor` (`readonly rCtor;` assigned `3`
/// in the constructor) is `number` with `noImplicitAny`, `any` without it,
/// under both `strictNullChecks` settings. The lane answers the typed
/// missing-member marker ([`a_flow_typed_property_is_the_missing_member_marker`]).
#[test]
#[ignore = "a property without a type or initializer takes the type its constructor assigns"]
fn a_constructor_assigned_property_takes_the_assigned_type() {
    for (compiler_options, answer) in [
        (None, "number"),
        (Some(r#"{ "strict": true, "noImplicitAny": false }"#), "any"),
    ] {
        let failures = mismatches_in(
            ProbeProject {
                files: &[],
                compiler_options,
                ambient_lib: None,
            },
            READONLY_INSTANCE,
            &[("ReturnType<typeof krCtor>", answer)],
        );
        assert!(
            failures.is_empty(),
            "{compiler_options:?}:\n{}",
            failures.join("\n")
        );
    }
}

const IMPLICIT_PROPERTIES: &str = "\
class N { p; readonly q; static s; }
export function np(n: N) { return n.p; }
export function nq(n: N) { return n.q; }
export function ns() { return N.s; }
declare class D { p; }
export function dp(d: D) { return d.p; }
class C { p; constructor() { } }
export function cp(c: C) { return c.p; }
class C3 { p; constructor() { const f = () => { this.p = 1; }; f(); } }
export function c3(c: C3) { return c.p; }
class B { p = 1 }
class Dd extends B { q; }
export function ddq(d: Dd) { return d.q; }
";

/// The four `strictNullChecks` × `noImplicitAny` settings.
const FOUR_SETTINGS: [Option<&str>; 4] = [
    None,
    Some(r#"{ "strict": true, "noImplicitAny": false }"#),
    Some(r#"{ "strict": true, "strictNullChecks": false }"#),
    Some(r#"{ "strict": true, "strictNullChecks": false, "noImplicitAny": false }"#),
];

/// A property written with neither a type nor an initializer is the
/// implicit `any` wherever the checker reads no control flow for it: a
/// class without a constructor, a constructor that never assigns it (an
/// assignment in an arrow it creates is not on its flow), a static of a
/// class without a static block, and an ambient class's property.
///
/// Measured on TypeScript 7.0.2 (all four `strictNullChecks` ×
/// `noImplicitAny` settings alike): `np`, `nq`, `ns`, `dp`, `cp`, `c3` and
/// `ddq` are `any`.
#[test]
fn a_property_without_type_or_initializer_is_the_implicit_any() {
    for compiler_options in FOUR_SETTINGS {
        let failures = mismatches_in(
            ProbeProject {
                files: &[],
                compiler_options,
                ambient_lib: None,
            },
            IMPLICIT_PROPERTIES,
            &[
                ("ReturnType<typeof np>", "any"),
                ("ReturnType<typeof nq>", "any"),
                ("ReturnType<typeof ns>", "any"),
                ("ReturnType<typeof dp>", "any"),
                ("ReturnType<typeof cp>", "any"),
                ("ReturnType<typeof c3>", "any"),
                ("ReturnType<typeof ddq>", "any"),
            ],
        );
        assert!(
            failures.is_empty(),
            "{compiler_options:?}:\n{}",
            failures.join("\n")
        );
    }
}

/// A property the constructor assigns takes its type from the
/// constructor's control flow under `noImplicitAny`, which the class
/// lowering does not model: the property is the typed missing-member
/// marker under every setting, never a guessed type
/// ([`a_constructor_assigned_property_takes_the_assigned_type`] holds the
/// measured answers). So is a static property of a class with a static
/// block (TypeScript 7.0.2: `S1.s` with `static { this.s = 1; }` is
/// `number` under `noImplicitAny`, `any` without it).
#[test]
fn a_flow_typed_property_is_the_missing_member_marker() {
    let source = "\
class K { readonly rCtor; constructor() { this.rCtor = 3; } }
export function krCtor(k: K) { return k.rCtor; }
class S1 { static s; static { this.s = 1; } }
export function s1() { return S1.s; }
";
    for compiler_options in FOUR_SETTINGS {
        for probe in [
            "ReturnType<typeof krCtor>",
            "K['rCtor']",
            "ReturnType<typeof s1>",
        ] {
            super::checker_probe_lane_tests::with_probe_in(
                ProbeProject {
                    files: &[],
                    compiler_options,
                    ambient_lib: None,
                },
                source,
                probe,
                |dispatch, node| {
                    assert!(
                        matches!(
                            dispatch.graph().node_data(node).as_deref(),
                            Some(crate::semantic_query::SemanticNodeData::Opaque(_))
                        ),
                        "{compiler_options:?} `{probe}`: the lane measured `{}`",
                        crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                            dispatch, node, 0
                        )
                    );
                },
            );
        }
    }
}
