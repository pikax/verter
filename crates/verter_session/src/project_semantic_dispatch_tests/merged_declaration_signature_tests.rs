//! Overload resolution over MERGED declarations and specialized signatures,
//! in TypeScript's candidate order (`reorderCandidates`): the signatures of
//! a later declaration of the same symbol are tried before an earlier
//! declaration's, a signature with a literal-typed parameter (a
//! "specialized" signature) is tried before the rest, and a merged
//! method's identical overloads all stay. Call resolution and the
//! signature utilities read that one order from `SignaturesOfType`.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --noEmit` (strict by default):
//! `declare const v: <probe>; export const s: null = v;` read off the
//! TS2322 message.

use super::checker_probe_lane_tests::{mismatches, mismatches_in, tuple_labels, ProbeProject};

const MERGED: &str = "\
interface M { (): 'first' }
interface M { (): 'second' }
declare const m: M;
export function callM() { return m(); }
interface MM { f(): 'first' }
interface MM { f(): 'second' }
declare const mm: MM;
export function callMM() { return mm.f(); }
interface MO { f(x: string): 'o1'; f(x: string): 'o2' }
interface MO { f(x: string): 'o3' }
declare const mo: MO;
export function callMO() { return mo.f('s'); }
interface T3 { (): 1 }
interface T3 { (): 2 }
interface T3 { (): 3 }
declare const t3: T3;
export function callT3() { return t3(); }
declare class CI { f(): 'class' }
interface CI { f(): 'iface' }
declare const ci: CI;
export function callCi() { return ci.f(); }
declare function nsf(x: string): 'fn';
declare namespace nsf { const v: 1 }
export function callNsf() { return nsf('x'); }
interface HA { (): 'base' }
interface M2 extends HA { (): 'first' }
interface M2 { (): 'second' }
declare const m2: M2;
export function callM2() { return m2(); }
interface HB { (x: string): 'base-str' }
interface M3 extends HB { (x: number): 'm3-num' }
interface M3 { (x: 'a'): 'm3-lit' }
declare const m3: M3;
export function callM3() { return m3('a'); }
export function callM3s() { return m3('s'); }
";

/// A later declaration's signatures are tried first: over two, three or a
/// class and an interface declaration, for call signatures and a merged
/// method, with a declaration carrying several overloads of its own kept
/// in its own order; a namespace merged into a function adds no signature.
///
/// Measured on TypeScript 7.0.2: `m()`, `mm.f()`, `ReturnType<M>` and
/// `ReturnType<MM['f']>` are `"second"`, `mo.f('s')` is `"o3"`, `t3()` is
/// `3`, `ci.f()` and `ReturnType<CI['f']>` are `"iface"`, and `nsf('x')`
/// is `"fn"`. Over `interface M2 extends HA { (): 'first' }` and `interface
/// M2 { (): 'second' }`, `m2()` is `"second"` while `ReturnType<M2>` is
/// `"base"` (the base's signature is listed last); over `M3 extends HB` with
/// `(x: number)` and a later `(x: 'a')`, `m3('a')` is `"m3-lit"` and
/// `m3('s')` reaches the base's `"base-str"`.
#[test]
fn a_later_merged_declaration_is_tried_first() {
    let failures = mismatches(
        MERGED,
        &[
            ("ReturnType<typeof callM>", "\"second\""),
            ("ReturnType<typeof callMM>", "\"second\""),
            ("ReturnType<typeof callMO>", "\"o3\""),
            ("ReturnType<typeof callT3>", "3"),
            ("ReturnType<typeof callCi>", "\"iface\""),
            ("ReturnType<typeof callNsf>", "\"fn\""),
            ("ReturnType<M>", "\"second\""),
            ("ReturnType<MM['f']>", "\"second\""),
            ("ReturnType<CI['f']>", "\"iface\""),
            ("ReturnType<typeof callM2>", "\"second\""),
            ("ReturnType<M2>", "\"base\""),
            ("ReturnType<typeof callM3>", "\"m3-lit\""),
            ("ReturnType<typeof callM3s>", "\"base-str\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const AUGMENTED: &str = "export interface X { (): 'a-file'; f(): 'a-f' }\n";
const AUGMENTING: &str = "import './a';\n\
declare module './a' {\n  interface X { (): 'b-file'; f(): 'b-f' }\n}\nexport {};\n";
const AUGMENTED_USE: &str = "import './b';
import type { X } from './a';
declare const x: X;
export function callX() { return x(); }
export function callXf() { return x.f(); }
";

/// A module augmentation in another file is a later declaration too.
///
/// Measured on TypeScript 7.0.2 with `a.ts` declaring `X` and `b.ts`
/// augmenting it: `x()` and `ReturnType<X>` are `"b-file"`, `x.f()` and
/// `ReturnType<X['f']>` are `"b-f"`.
#[test]
fn a_module_augmentation_is_a_later_declaration() {
    let files = [("a.ts", AUGMENTED), ("b.ts", AUGMENTING)];
    let project = ProbeProject {
        files: &files,
        compiler_options: None,
        ambient_lib: None,
    };
    let failures = mismatches_in(
        project,
        AUGMENTED_USE,
        &[
            ("ReturnType<typeof callX>", "\"b-file\""),
            ("ReturnType<typeof callXf>", "\"b-f\""),
            ("ReturnType<X>", "\"b-file\""),
            ("ReturnType<X['f']>", "\"b-f\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const SPECIALIZED: &str = "\
interface S { (x: string): 'str'; (x: 'a'): 'lit' }
declare const s: S;
export function callS() { return s('a'); }
export function callSb() { return s('b'); }
declare function sf(x: string): 'str';
declare function sf(x: 'a'): 'lit';
export function callSf() { return sf('a'); }
declare function uf(x: string): 'str';
declare function uf(x: 'a' | 'b'): 'union';
export function callUf() { return uf('a'); }
declare function tf(x: boolean): 'bool';
declare function tf(x: true): 'true';
export function callTf() { return tf(true); }
declare function nf(x: number): 'num';
declare function nf(x: 1): 'one';
export function callNf() { return nf(1); }
declare function nlf(x: string | null): 'any';
declare function nlf(x: null): 'null';
export function callNlf() { return nlf(null); }
declare function udf(x: string | undefined): 'any';
declare function udf(x: undefined): 'undef';
export function callUdf() { return udf(undefined); }
type LitA = 'a';
declare function af(x: string): 'str';
declare function af(x: LitA): 'alias';
export function callAf() { return af('a'); }
declare function tl(x: string): 'str';
declare function tl(x: `a${string}`): 'tpl';
export function callTl() { return tl('ab'); }
interface SB { (x: 'a'): 'base-lit' }
interface SD extends SB { (x: string): 'own-str' }
declare const sd: SD;
export function callSd() { return sd('a'); }
interface GB<T> { (x: string): 1; (x: T): 2 }
declare const gb: GB<'a'>;
export function callGB() { return gb('a'); }
declare function gf<T>(x: T): 'gen';
declare function gf(x: 'a'): 'lit';
export function callGf() { return gf('a'); }
";

/// A signature whose parameter is WRITTEN as a literal type (`null`
/// included) is tried before the others; a union, an alias, a template
/// literal, `undefined` or an instantiated type parameter is not such a
/// parameter. A specialized base signature stays ahead of the derived
/// declaration's own.
///
/// Measured on TypeScript 7.0.2: `s('a')`, `ReturnType<S>`, `sf('a')` and
/// `gf('a')` are `"lit"`, `s('b')` is `"str"`, `uf('a')` is `"str"`,
/// `tf(true)` is `"true"`, `nf(1)` is `"one"`, `nlf(null)` is `"null"`,
/// `udf(undefined)` is `"any"`, `af('a')` and `tl('ab')` are `"str"`,
/// `sd('a')` is `"base-lit"` and `gb('a')` over `GB<'a'>` is `1`.
#[test]
fn a_specialized_signature_is_tried_first() {
    let failures = mismatches(
        SPECIALIZED,
        &[
            ("ReturnType<typeof callS>", "\"lit\""),
            ("ReturnType<typeof callSb>", "\"str\""),
            ("ReturnType<S>", "\"lit\""),
            ("ReturnType<typeof callSf>", "\"lit\""),
            ("ReturnType<typeof callUf>", "\"str\""),
            ("ReturnType<typeof callTf>", "\"true\""),
            ("ReturnType<typeof callNf>", "\"one\""),
            ("ReturnType<typeof callNlf>", "\"null\""),
            ("ReturnType<typeof callUdf>", "\"any\""),
            ("ReturnType<typeof callAf>", "\"str\""),
            ("ReturnType<typeof callTl>", "\"str\""),
            ("ReturnType<typeof callSd>", "\"base-lit\""),
            ("ReturnType<typeof callGB>", "1"),
            ("ReturnType<typeof callGf>", "\"lit\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const IDENTICAL: &str = "\
interface MDX { f(x: string): string }
interface MDX { f(y: string): string }
interface G<T> { f(x: T): T }
interface G<T> { f(y: T): T }
interface OD { f(x: string): string; f(y: string): string }
declare function fd(x: string): string;
declare function fd(y: string): string;
";

/// Identical overloads of a merged method all stay, so the utilities read
/// the LAST one, as they do for one declaration's identical overloads and
/// for function overloads.
///
/// Measured on TypeScript 7.0.2: `Parameters<MDX['f']>`,
/// `Parameters<OD['f']>` and `Parameters<typeof fd>` are `[y: string]`,
/// and `Parameters<G<number>['f']>` is `[y: number]`.
#[test]
fn identical_overloads_of_a_merged_method_all_stay() {
    for (probe, label) in [
        ("Parameters<MDX['f']>", "y"),
        ("Parameters<OD['f']>", "y"),
        ("Parameters<typeof fd>", "y"),
        ("Parameters<G<number>['f']>", "y"),
    ] {
        assert_eq!(
            tuple_labels(IDENTICAL, probe),
            vec![Some(label.to_owned())],
            "`{probe}` reads the last identical overload"
        );
    }
    let failures = mismatches(
        IDENTICAL,
        &[
            ("Parameters<MDX['f']>[0]", "string"),
            ("Parameters<G<number>['f']>[0]", "number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The project root of a [`program_order_answer`] host.
const PROGRAM_ROOT: &str = "/program";

/// How each of `count` contributor files declares the merged symbol: file
/// `i` adds one `emit` overload taking `{ k<j>: 1 }` and returning `j`,
/// `j = count - 1 - i`, so a call whose argument carries every key returns
/// the `j` of the declaration it tries first.
#[derive(Clone, Copy, Debug)]
enum Contributor {
    /// A script's global `interface BenchBus`.
    GlobalInterface,
    /// A script's global `declare namespace BenchNs`.
    GlobalNamespace,
    /// A module's `declare module "./core"` augmenting `core.ts`'s `Props`.
    ModuleAugmentation,
}

impl Contributor {
    fn file(self, i: usize) -> String {
        match self {
            Self::GlobalInterface | Self::GlobalNamespace => format!("bus-{i:04}.ts"),
            Self::ModuleAugmentation => format!("plugin-{i:04}.ts"),
        }
    }

    fn source(self, i: usize, answer: usize) -> String {
        match self {
            Self::GlobalInterface => {
                format!("interface BenchBus {{ emit(e: {{ k{i}: 1 }}): {answer}; }}\n")
            }
            Self::GlobalNamespace => format!(
                "declare namespace BenchNs {{ function emit(e: {{ k{i}: 1 }}): {answer}; }}\n"
            ),
            Self::ModuleAugmentation => format!(
                "import './core';\n\
                 declare module './core' {{ interface Props {{ emit(e: {{ k{i}: 1 }}): {answer}; }} }}\n\
                 export {{}};\n"
            ),
        }
    }

    fn scenario(self, count: usize) -> String {
        let keys: Vec<String> = (0..count).map(|key| format!("k{key}: 1")).collect();
        let (head, call) = match self {
            Self::GlobalInterface => ("declare const bus: BenchBus;\n", "bus.emit(ev)"),
            Self::GlobalNamespace => ("", "BenchNs.emit(ev)"),
            Self::ModuleAugmentation => (
                "import type { Props } from './core';\ndeclare const bus: Props;\n",
                "bus.emit(ev)",
            ),
        };
        format!(
            "{head}declare const ev: {{ {} }};\nconst r = {call};\ntype __Probe = typeof r;\nexport {{}};\n",
            keys.join("; ")
        )
    }
}

/// The literal `__Probe` settles on in a project whose `tsconfig.json`
/// lists `lib.bench.d.ts`, the contributor files in `listed` order and
/// `scenario.ts`; each contributor `i` answers `answers[i]`. The files are
/// upserted in `upserted` order, then `edits` (`(i, answer)`) are applied
/// and the probe is read again; returns the answer before and after.
fn program_order_answer(
    kind: Contributor,
    answers: &[usize],
    listed: &[usize],
    upserted: &[usize],
    edits: &[(usize, usize)],
) -> (f64, f64) {
    use verter_type_engine::semantic_query::{LiteralValue, ProjectionMode, SemanticNodeData};
    let mut files: Vec<String> = vec!["lib.bench.d.ts".to_owned()];
    files.extend(listed.iter().map(|&i| kind.file(i)));
    files.push("scenario.ts".to_owned());
    let tsconfig = serde_json::json!({
        "compilerOptions": { "strict": true, "noLib": true, "noEmit": true },
        "files": files,
    })
    .to_string();
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[(PROGRAM_ROOT, &tsconfig)],
    );
    let upsert = |name: &str, source: &str| {
        let path = format!("{PROGRAM_ROOT}/{name}");
        let language = crate::LanguageRegistry::global()
            .classify_static(&path)
            .static_resolution();
        crate::u6_flow_shape_corpus_tests::upsert(&host, &path, source, language);
    };
    upsert(
        "lib.bench.d.ts",
        "interface Object { toString(): string; }\n",
    );
    if matches!(kind, Contributor::ModuleAugmentation) {
        upsert("core.ts", "export interface Props { base: -1 }\n");
    }
    for &i in upserted {
        upsert(&kind.file(i), &kind.source(i, answers[i]));
    }
    let scenario = format!("{PROGRAM_ROOT}/scenario.ts");
    upsert("scenario.ts", &kind.scenario(answers.len()));
    let read = || {
        let node = host
            .resolve_named_symbol(&scenario, "__Probe", Some(ProjectionMode::Expanded))
            .expect("the probe answers");
        let graph = host.project_type_store().semantic_graph();
        let mut node = node;
        loop {
            match graph.node_data(node).as_deref().expect("a live answer") {
                SemanticNodeData::Alias(inner) => node = *inner,
                SemanticNodeData::Literal(LiteralValue::Number(value)) => return *value,
                other => panic!("{kind:?}: `__Probe` is no number literal: {other:?}"),
            }
        }
    };
    let before = read();
    for &(i, answer) in edits {
        upsert(&kind.file(i), &kind.source(i, answer));
    }
    (before, read())
}

/// A deterministic permutation of `0..count`, different per `seed`.
fn shuffled(count: usize, seed: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).collect();
    let mut state = seed.wrapping_mul(2_654_435_761) | 1;
    for i in (1..count).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        order.swap(i, state % (i + 1));
    }
    order
}

/// Declarations of one merged symbol in several files order by the
/// program's file order — the `files` sequence — never by path: a call
/// tries the declaration of the file listed LAST first, for a global
/// interface's members, a global namespace's function overloads and a
/// module augmentation's members alike.
///
/// Measured on TypeScript 7.0.2 with N contributor files each adding one
/// overload `emit(e: { k<i>: 1 }): N-1-i` in file `i`: listing the files in
/// reverse path order answers `N-1` (the first file's), in forward order
/// `0`.
///
/// Mutation: ordering contributors by canonical path alone answers `0`
/// for the reverse listing.
#[test]
fn merged_declarations_follow_program_file_order() {
    for kind in [
        Contributor::GlobalInterface,
        Contributor::GlobalNamespace,
        Contributor::ModuleAugmentation,
    ] {
        for count in [10, 100] {
            let answers: Vec<usize> = (0..count).map(|i| count - 1 - i).collect();
            let forward: Vec<usize> = (0..count).collect();
            let reverse: Vec<usize> = forward.iter().rev().copied().collect();
            assert_eq!(
                program_order_answer(kind, &answers, &reverse, &forward, &[]).0,
                (count - 1) as f64,
                "{kind:?} N={count}: files listed in reverse path order"
            );
            assert_eq!(
                program_order_answer(kind, &answers, &forward, &forward, &[]).0,
                0.0,
                "{kind:?} N={count}: files listed in path order"
            );
        }
    }
}

/// The 500-file reproduction of the global-interface case.
#[test]
fn merged_global_interface_follows_program_file_order_at_scale() {
    let count = 500;
    let answers: Vec<usize> = (0..count).map(|i| count - 1 - i).collect();
    let forward: Vec<usize> = (0..count).collect();
    let reverse: Vec<usize> = forward.iter().rev().copied().collect();
    assert_eq!(
        program_order_answer(
            Contributor::GlobalInterface,
            &answers,
            &reverse,
            &forward,
            &[]
        )
        .0,
        (count - 1) as f64
    );
}

/// The merge order is the program's whatever order the files reach the
/// host in, and an edit answers what a cold build of the edited files
/// answers: cold, warm and incremental agree.
///
/// Mutation: ordering contributors by upsert (ingestion) order answers a
/// shuffled host differently from the cold one.
#[test]
fn merged_declaration_order_is_independent_of_upsert_order_and_edits() {
    let count = 24;
    let answers: Vec<usize> = (0..count).map(|i| count - 1 - i).collect();
    let forward: Vec<usize> = (0..count).collect();
    let listed = shuffled(count, 3);
    for kind in [
        Contributor::GlobalInterface,
        Contributor::GlobalNamespace,
        Contributor::ModuleAugmentation,
    ] {
        // The file listed last is tried first; editing its answer moves the
        // probe, editing an earlier one does not.
        let last = *listed.last().expect("files");
        let edits = [(last, 1000), (listed[0], 2000)];
        let mut edited = answers.clone();
        for &(i, answer) in &edits {
            edited[i] = answer;
        }
        let (cold, _) = program_order_answer(kind, &answers, &listed, &forward, &[]);
        let (cold_edited, _) = program_order_answer(kind, &edited, &listed, &forward, &[]);
        assert_eq!(cold, answers[last] as f64, "{kind:?}: cold answer");
        assert_eq!(
            cold_edited, 1000.0,
            "{kind:?}: cold answer of the edited files"
        );
        for seed in [5, 11] {
            let (warm, incremental) =
                program_order_answer(kind, &answers, &listed, &shuffled(count, seed), &edits);
            assert_eq!(warm, cold, "{kind:?} seed {seed}: shuffled upserts");
            assert_eq!(
                incremental, cold_edited,
                "{kind:?} seed {seed}: after editing two contributors"
            );
        }
    }
}
