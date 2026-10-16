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

/// A host over one project rooted at [`PROGRAM_ROOT`], configured by
/// `tsconfig`, holding `files` (`(name, source)`), upserted in order.
fn program_host(tsconfig: &serde_json::Value, files: &[(String, String)]) -> crate::VerterHost {
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[(PROGRAM_ROOT, &tsconfig.to_string())],
    );
    for (name, source) in files {
        program_upsert(&host, name, source);
    }
    host
}

fn program_upsert(host: &crate::VerterHost, name: &str, source: &str) {
    let path = format!("{PROGRAM_ROOT}/{name}");
    let language = crate::LanguageRegistry::global()
        .classify_static(&path)
        .static_resolution();
    crate::u6_flow_shape_corpus_tests::upsert(host, &path, source, language);
}

/// The literal `__Probe` in `scenario.ts` settles on.
fn probe(host: &crate::VerterHost) -> f64 {
    literal_number(host, "__Probe")
}

/// The number literal the type `name` in `scenario.ts` settles on.
fn literal_number(host: &crate::VerterHost, name: &str) -> f64 {
    let scenario = format!("{PROGRAM_ROOT}/scenario.ts");
    literal_number_at(host, &scenario, name)
}

fn literal_number_at(host: &crate::VerterHost, scenario: &str, name: &str) -> f64 {
    use verter_type_engine::semantic_query::{LiteralValue, ProjectionMode, SemanticNodeData};
    let mut node = host
        .resolve_named_symbol(scenario, name, Some(ProjectionMode::Expanded))
        .expect("the probe answers")
        .id();
    let graph = host.project_type_store().semantic_graph();
    loop {
        match graph.node_data(node).as_deref().expect("a live answer") {
            SemanticNodeData::Alias(inner) => node = *inner,
            SemanticNodeData::Literal(LiteralValue::Number(value)) => return *value,
            other => panic!("`{name}` is no number literal: {other:?}"),
        }
    }
}

/// `lib.bench.d.ts`, the program's whole library under `noLib`.
fn bench_lib() -> (String, String) {
    (
        "lib.bench.d.ts".to_owned(),
        "interface Object { toString(): string; }\n".to_owned(),
    )
}

/// The literal `__Probe` settles on in a project whose `tsconfig.json`
/// lists `lib.bench.d.ts`, the contributor files in `listed` order and
/// `scenario.ts`; each contributor `i` answers `answers[i]`. The files are
/// upserted in `upserted` order and the probe is read twice — a cold, then
/// a warm read of the unchanged host — then `edits` (`(i, answer)`) are
/// applied and the probe is read again. Returns the cold, warm and
/// incremental answers.
fn program_order_answer(
    kind: Contributor,
    answers: &[usize],
    listed: &[usize],
    upserted: &[usize],
    edits: &[(usize, usize)],
) -> (f64, f64, f64) {
    let mut listing: Vec<String> = vec!["lib.bench.d.ts".to_owned()];
    listing.extend(listed.iter().map(|&i| kind.file(i)));
    listing.push("scenario.ts".to_owned());
    let tsconfig = serde_json::json!({
        "compilerOptions": { "strict": true, "noLib": true, "noEmit": true },
        "files": listing,
    });
    let mut files = vec![bench_lib()];
    if matches!(kind, Contributor::ModuleAugmentation) {
        files.push((
            "core.ts".to_owned(),
            "export interface Props { base: -1 }\n".to_owned(),
        ));
    }
    files.extend(
        upserted
            .iter()
            .map(|&i| (kind.file(i), kind.source(i, answers[i]))),
    );
    files.push(("scenario.ts".to_owned(), kind.scenario(answers.len())));
    let host = program_host(&tsconfig, &files);
    let cold = probe(&host);
    let warm = probe(&host);
    for &(i, answer) in edits {
        program_upsert(&host, &kind.file(i), &kind.source(i, answer));
    }
    (cold, warm, probe(&host))
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
/// host in; a warm read of the unchanged host answers what its cold read
/// did; and an edit answers what a cold build of the edited files answers:
/// cold, warm and incremental agree.
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
        let (cold, cold_warm, _) = program_order_answer(kind, &answers, &listed, &forward, &[]);
        let (cold_edited, _, _) = program_order_answer(kind, &edited, &listed, &forward, &[]);
        assert_eq!(cold, answers[last] as f64, "{kind:?}: cold answer");
        assert_eq!(cold_warm, cold, "{kind:?}: warm read of the cold host");
        assert_eq!(
            cold_edited, 1000.0,
            "{kind:?}: cold answer of the edited files"
        );
        for seed in [5, 11] {
            let (shuffled_cold, warm, incremental) =
                program_order_answer(kind, &answers, &listed, &shuffled(count, seed), &edits);
            assert_eq!(
                shuffled_cold, cold,
                "{kind:?} seed {seed}: shuffled upserts"
            );
            assert_eq!(
                warm, cold,
                "{kind:?} seed {seed}: warm read, unchanged host"
            );
            assert_eq!(
                incremental, cold_edited,
                "{kind:?} seed {seed}: after editing two contributors"
            );
        }
    }
}

/// A script adding one `emit` overload answering `answer` to the global
/// `interface BenchBus`, after `prefix` (triple-slash references).
fn bus_script(prefix: &str, answer: usize) -> String {
    format!("{prefix}interface BenchBus {{ emit(e: {{ k{answer}: 1 }}): {answer}; }}\n")
}

/// `scenario.ts` calling `BenchBus.emit` with every key of `answers`.
fn bus_scenario(answers: &[usize]) -> String {
    let keys: Vec<String> = answers.iter().map(|key| format!("k{key}: 1")).collect();
    format!(
        "declare const bus: BenchBus;\ndeclare const ev: {{ {} }};\n\
         const r = bus.emit(ev);\ntype __Probe = typeof r;\nexport {{}};\n",
        keys.join("; ")
    )
}

/// A noLib project listing `listed` between `lib.bench.d.ts` and
/// `scenario.ts`, with an optional `include`.
fn bus_tsconfig(listed: &[&str], include: Option<&[&str]>) -> serde_json::Value {
    let mut files = vec!["lib.bench.d.ts"];
    files.extend_from_slice(listed);
    files.push("scenario.ts");
    let mut tsconfig = serde_json::json!({
        "compilerOptions": { "strict": true, "noLib": true, "noEmit": true },
        "files": files,
    });
    if let Some(include) = include {
        tsconfig["include"] = serde_json::json!(include);
    }
    tsconfig
}

fn bus_project(
    tsconfig: &serde_json::Value,
    sources: &[(&str, String)],
    answers: &[usize],
) -> crate::VerterHost {
    let mut files = vec![bench_lib()];
    files.extend(
        sources
            .iter()
            .map(|(name, source)| ((*name).to_owned(), source.clone())),
    );
    files.push(("scenario.ts".to_owned(), bus_scenario(answers)));
    program_host(tsconfig, &files)
}

/// The program places a file before the first file that pulls it in —
/// through an import or a `/// <reference path>` (with or without an
/// extension) — not at the end, however it is listed. Each case lists
/// `a.ts` then `b.ts`; `c.ts` is unlisted, or listed after `b.ts`, and
/// `a.ts` pulls it in, so the program is `c, a, b` and `b.ts`'s overload,
/// the last declaration's, is tried first.
///
/// Measured on TypeScript 7.0.2: every case answers `2`. Ordering by the
/// `files` sequence alone ranks the unlisted `c.ts` last (answer `3`).
#[test]
fn files_a_program_pulls_in_precede_the_file_that_pulls_them() {
    let cases: [(&str, &[&str], &str); 4] = [
        (
            "an import of an unlisted file",
            &["a.ts", "b.ts"],
            "import './c';\nexport {};\n",
        ),
        (
            "an import of a file listed later",
            &["a.ts", "b.ts", "c.ts"],
            "import './c';\nexport {};\n",
        ),
        (
            "a path reference",
            &["a.ts", "b.ts"],
            "/// <reference path=\"./c.ts\" />\nexport {};\n",
        ),
        (
            "an extensionless path reference",
            &["a.ts", "b.ts"],
            "/// <reference path=\"c\" />\nexport {};\n",
        ),
    ];
    for (case, listed, a_source) in cases {
        let host = bus_project(
            &bus_tsconfig(listed, None),
            &[
                ("a.ts", a_source.to_owned()),
                ("b.ts", bus_script("", 2)),
                ("c.ts", bus_script("", 3)),
            ],
            &[2, 3],
        );
        assert_eq!(probe(&host), 2.0, "{case}");
        assert_eq!(probe(&host), 2.0, "{case}: warm");
    }
}

/// `include` roots follow `files`, grouped by the first authored entry a
/// file matches, each entry's files in directory-walk order — a
/// directory's files before its subdirectories — not path order.
///
/// Measured on TypeScript 7.0.2 with `"include": ["lib/**/*",
/// "app/**/*"]`: the program lists `lib/a.ts`, `app/z.ts`,
/// `app/sub/m.ts`, so `app/sub/m.ts`'s overload is tried first (`2`).
/// Path order would end with `lib/a.ts` (`3`); walk order without entry
/// grouping with `app/z.ts` (`1`).
#[test]
fn include_roots_follow_entry_then_directory_walk_order() {
    let host = bus_project(
        &bus_tsconfig(&[], Some(&["lib/**/*", "app/**/*"])),
        &[
            ("app/z.ts", bus_script("", 1)),
            ("app/sub/m.ts", bus_script("", 2)),
            ("lib/a.ts", bus_script("", 3)),
        ],
        &[1, 2, 3],
    );
    assert_eq!(probe(&host), 2.0);
}

/// An edit to a file that declares nothing — adding or removing the
/// import that pulls a contributor in — reorders the program and so
/// changes which overload wins; a warm answer from before the edit never
/// survives it, and every state answers what a cold build of it does.
///
/// Measured on TypeScript 7.0.2, listing `a.ts`, `b.ts`, `c.ts`: with
/// `a.ts` importing nothing the answer is `3` (`c.ts` last); importing
/// `./c` it is `2` (`c, a, b`).
///
/// Mutation: not observing the program's file order on the merged
/// result serves the stale warm `3` after the import is added.
#[test]
fn a_non_contributor_import_edit_reorders_the_merge() {
    let tsconfig = bus_tsconfig(&["a.ts", "b.ts", "c.ts"], None);
    let detached = "export {};\n".to_owned();
    let importing = "import './c';\nexport {};\n".to_owned();
    let project = |a_source: &str| {
        bus_project(
            &tsconfig,
            &[
                ("a.ts", a_source.to_owned()),
                ("b.ts", bus_script("", 2)),
                ("c.ts", bus_script("", 3)),
            ],
            &[2, 3],
        )
    };
    let cold_detached = probe(&project(&detached));
    let cold_importing = probe(&project(&importing));
    assert_eq!(cold_detached, 3.0, "cold, a.ts imports nothing");
    assert_eq!(cold_importing, 2.0, "cold, a.ts imports ./c");

    let host = project(&detached);
    assert_eq!(probe(&host), cold_detached, "first read");
    assert_eq!(probe(&host), cold_detached, "warm read");
    program_upsert(&host, "a.ts", &importing);
    assert_eq!(probe(&host), cold_importing, "after adding the import");
    assert_eq!(probe(&host), cold_importing, "warm after adding the import");
    program_upsert(&host, "a.ts", &detached);
    assert_eq!(probe(&host), cold_detached, "after removing the import");
}

/// A module augmentation names the module it merges into without pulling
/// that module into the program; only an import does.
///
/// Measured on TypeScript 7.0.2, listing `aug.ts`, `b.ts`, `core.ts`,
/// `c.ts`, with `core.ts` importing `./c`: when `aug.ts` only augments
/// `./core` the program is `aug, b, c, core` and the answer is `3`; when
/// `aug.ts` also imports `./core` it is `c, core, aug, b` and the answer
/// is `2`. Either way the augmentation still merges into `core.ts`.
///
/// Mutation: visiting an augmentation's target as an import answers `2`
/// for the augmentation alone.
#[test]
fn a_module_augmentation_does_not_pull_its_target_in() {
    let augmenting = "export {};
declare module './core' { interface Core { extra: 1 } }
";
    let importing = "import './core';
declare module './core' { interface Core { extra: 1 } }
";
    let project = |aug_source: &str| {
        bus_project(
            &bus_tsconfig(&["aug.ts", "b.ts", "core.ts", "c.ts"], None),
            &[
                ("aug.ts", aug_source.to_owned()),
                ("b.ts", bus_script("", 2)),
                (
                    "core.ts",
                    "import './c';
export interface Core {}
"
                    .to_owned(),
                ),
                ("c.ts", bus_script("", 3)),
            ],
            &[2, 3],
        )
    };
    for (case, source, answer) in [
        ("augmenting", augmenting, 3.0),
        ("importing", importing, 2.0),
    ] {
        let host = project(source);
        assert_eq!(probe(&host), answer, "{case}: cold");
        assert_eq!(probe(&host), answer, "{case}: warm");
        program_upsert(
            &host,
            "scenario.ts",
            "import type { Core } from './core';
type __Extra = Core['extra'];
export {};
",
        );
        assert_eq!(
            literal_number(&host, "__Extra"),
            1.0,
            "{case}: the augmentation merges into core.ts"
        );
    }
    let host = project(augmenting);
    assert_eq!(probe(&host), 3.0, "before the edit");
    program_upsert(&host, "aug.ts", importing);
    assert_eq!(probe(&host), 2.0, "after adding the import");
    program_upsert(&host, "aug.ts", augmenting);
    assert_eq!(probe(&host), 3.0, "after removing the import");
}

/// The core of a library: the globals every program needs.
const LIBRARY_CORE: &str = "interface Array<T> { length: number; [n: number]: T; }\n\
interface Boolean {}\ninterface Function {}\ninterface IArguments {}\n\
interface Number {}\ninterface Object {}\ninterface RegExp {}\ninterface String {}\n";

/// A library declaring the global `Bus` with one `emit` overload answering
/// `answer`, after `head` (its own references).
fn bus_library(head: &str, answer: usize) -> String {
    format!("{head}interface Bus {{ emit(e: {{ k{answer}: 1 }}): {answer}; }}\n")
}

/// The libraries the library-order fixtures register: `lib.es5.d.ts`
/// (answering `1`, with the core globals), `lib.es2015.core.d.ts` (`2`),
/// `lib.es2015.collection.d.ts` (`3`), and `lib.es2015.d.ts`, which
/// declares nothing and references the other three as TypeScript's does.
fn bus_libraries() -> Vec<(&'static str, String)> {
    vec![
        (
            "lib.es5.d.ts",
            format!("{LIBRARY_CORE}{}", bus_library("", 1)),
        ),
        ("lib.es2015.core.d.ts", bus_library("", 2)),
        ("lib.es2015.collection.d.ts", bus_library("", 3)),
        (
            "lib.es2015.d.ts",
            "/// <reference lib=\"es5\" />\n/// <reference lib=\"es2015.core\" />\n\
             /// <reference lib=\"es2015.collection\" />\n"
                .to_owned(),
        ),
    ]
}

/// A `lib: ["es5"]` project listing `ref.ts` then `scenario.ts`, with
/// `libraries` registered in the order given.
fn library_project(ref_source: &str, libraries: &[(&str, String)]) -> crate::VerterHost {
    let tsconfig = serde_json::json!({
        "compilerOptions": { "strict": true, "lib": ["es5"], "noEmit": true },
        "files": ["ref.ts", "scenario.ts"],
    });
    let host = program_host(&tsconfig, &[]);
    for (name, source) in libraries {
        host.workspace()
            .register_ambient_lib(verter_workspace::AmbientLibSpec {
                project_id: None,
                canonical_id: std::sync::Arc::from(*name),
                source: std::sync::Arc::from(source.as_str()),
            })
            .expect("the library registers against the project");
    }
    program_upsert(&host, "ref.ts", ref_source);
    program_upsert(
        &host,
        "scenario.ts",
        "declare const bus: Bus;\ndeclare const ev: { k1: 1; k2: 1; k3: 1 };\n\
         const r = bus.emit(ev);\ntype __Probe = typeof r;\nexport {};\n",
    );
    host
}

/// A file's `/// <reference lib>` loads that library into the program; the
/// program lists its libraries before its own files, by TypeScript's
/// library priority, so a later library's declaration of a global is tried
/// first. A library the program does not load contributes nothing, however
/// it was registered; editing the reference in a file that declares
/// nothing moves the answer, and every state answers what a cold host of
/// it does.
///
/// Measured on TypeScript 7.0.2 with `lib: ["es5"]` and the merged `Bus`
/// declared by `lib.es5.d.ts` (`1`), `lib.es2015.core.d.ts` (`2`) and
/// `lib.es2015.collection.d.ts` (`3`): `ref.ts` referencing no library
/// answers `1`, `es2015.core` `2`, `es2015.collection` `3`.
///
/// Mutation: merging every registered library answers `3` with no
/// reference; ordering libraries by registration answers the first
/// registered.
#[test]
fn a_library_reference_loads_and_orders_its_library() {
    let states = [
        ("export {};\n", 1.0),
        ("/// <reference lib=\"es2015.core\" />\nexport {};\n", 2.0),
        (
            "/// <reference lib=\"es2015.collection\" />\nexport {};\n",
            3.0,
        ),
        ("export {};\n", 1.0),
    ];
    let libraries = bus_libraries();
    let mut reversed = libraries.clone();
    reversed.reverse();
    for (source, answer) in states {
        for registered in [&libraries, &reversed] {
            let host = library_project(source, registered);
            assert_eq!(probe(&host), answer, "cold: {source:?}");
            assert_eq!(probe(&host), answer, "warm: {source:?}");
        }
    }
    let host = library_project(states[0].0, &libraries);
    for (source, answer) in states {
        program_upsert(&host, "ref.ts", source);
        assert_eq!(probe(&host), answer, "incremental: {source:?}");
        assert_eq!(probe(&host), answer, "warm incremental: {source:?}");
    }
}

/// A library's own references load the libraries it names, placed by
/// priority whatever order the walk reaches them in.
///
/// Measured on TypeScript 7.0.2 with `lib: ["es5"]` and `ref.ts`
/// referencing `es2015`: the program lists `lib.es5.d.ts`,
/// `lib.es2015.d.ts`, `lib.es2015.core.d.ts`, `lib.es2015.collection.d.ts`
/// and answers `3`.
#[test]
fn a_referenced_library_loads_the_libraries_it_references() {
    let libraries = bus_libraries();
    let mut reversed = libraries.clone();
    reversed.reverse();
    for registered in [&libraries, &reversed] {
        let host = library_project("/// <reference lib=\"es2015\" />\nexport {};\n", registered);
        assert_eq!(probe(&host), 3.0);
        assert_eq!(probe(&host), 3.0, "warm");
    }
}

/// A program's requesting file and every contributor can be reachable but
/// absent from its configured roots. The first semantic read must use the
/// completed population and its visit order, independently of prior reads.
#[test]
fn disk_reachable_globals_and_unlisted_requesters_share_program_order() {
    use std::sync::Arc;
    use verter_workspace::WorkspaceAccess;
    for edges in [
        "import './z'; import './a'; import './scenario'; export {};",
        "/// <reference path=\"./z.ts\" />\n/// <reference path=\"./a.ts\" />\n/// <reference path=\"./scenario.ts\" />\nexport {};",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let directory = dir.path().canonicalize().unwrap();
        let root = directory.to_str().unwrap();
        let config = directory.join("tsconfig.json");
        std::fs::write(&config, r#"{"compilerOptions":{"strict":true,"noLib":true},"files":["lib.bench.d.ts","entry.ts"]}"#).unwrap();
        for (name, source) in [
            bench_lib(),
            ("entry.ts".into(), edges.into()),
            ("z.ts".into(), Contributor::GlobalInterface.source(0, 1)),
            ("a.ts".into(), Contributor::GlobalInterface.source(1, 2)),
            ("scenario.ts".into(), bus_scenario(&[0, 1])),
        ] {
            std::fs::write(directory.join(name), source).unwrap();
        }
        let make = || {
            let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()));
            let project = verter_workspace::configured_ide_project(workspace.as_ref(), root.into(), root.into(), config.to_str().unwrap());
            workspace.configure_resolver(vec![project]);
            crate::VerterHost::new(crate::HostConfig::default(), workspace)
        };
        let scenario = format!("{root}/scenario.ts");
        let read = |host: &crate::VerterHost| literal_number_at(host, &scenario, "__Probe");
        let host = make();
        assert_eq!(read(&host), 2.0, "first read: {edges}");
        assert_eq!(read(&host), 2.0, "warm");
        assert_eq!(read(&make()), 2.0, "independently cold");
        let source = Contributor::GlobalInterface.source(1, 3);
        let edited = directory.join("a.ts");
        std::fs::write(&edited, &source).unwrap();
        crate::u6_flow_shape_corpus_tests::upsert(&host, edited.to_str().unwrap(), &source, crate::FileLanguage::script_ts());
        assert_eq!(read(&host), 3.0, "incremental");
        assert_eq!(read(&make()), 3.0, "cold after edit");
    }
}

/// Reachability of a sole contributor is a dependency even when no merge
/// needs ordering. A non-contributor edit can retract it without changing
/// the indexed declaration or the requesting source.
#[test]
fn a_single_imported_global_retires_when_its_incoming_edge_is_removed() {
    use verter_type_engine::semantic_query::{LiteralValue, ProjectionMode, SemanticNodeData};
    let config = serde_json::json!({ "compilerOptions": { "noLib": true }, "files": ["scenario.ts", "bridge.ts"] });
    let files = |bridge: &str| {
        vec![
            (
                "scenario.ts".into(),
                "type __Probe = X['value']; type __Other = X['value']; export {};".into(),
            ),
            ("bridge.ts".into(), bridge.into()),
            ("globals.d.ts".into(), "interface X { value: 7 }".into()),
        ]
    };
    let host = program_host(&config, &files("import './globals'; export {};"));
    let read = |host: &crate::VerterHost, name: &str| {
        let mut node = host
            .resolve_named_symbol(
                &format!("{PROGRAM_ROOT}/scenario.ts"),
                name,
                Some(ProjectionMode::Expanded),
            )?
            .id();
        let graph = host.project_type_store().semantic_graph();
        loop {
            match graph.node_data(node)?.as_ref() {
                SemanticNodeData::Alias(inner) => node = *inner,
                SemanticNodeData::Literal(LiteralValue::Number(value)) => return Some(*value),
                _ => return None,
            }
        }
    };
    assert_eq!(read(&host, "__Probe"), Some(7.0));
    assert_eq!(read(&host, "__Probe"), Some(7.0));
    assert_eq!(
        read(&host, "__Other"),
        Some(7.0),
        "a distinct demand reuses the population view"
    );
    program_upsert(&host, "bridge.ts", "export {};");
    let cold = read(&program_host(&config, &files("export {};")), "__Probe");
    assert_eq!(
        cold, None,
        "a fresh program excludes the detached declaration"
    );
    assert_eq!(
        read(&host, "__Probe"),
        cold,
        "the warm singleton must retire"
    );
    assert_eq!(
        read(&host, "__Other"),
        cold,
        "the reused population view carries membership facts"
    );
}

#[test]
fn two_programs_can_share_an_imported_global_contributor() {
    let config = r#"{"compilerOptions":{"noLib":true},"files":["main.ts"]}"#;
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[("/one", config), ("/two", config)],
    );
    for (file, source) in [
        ("/shared/globals.d.ts", "interface Shared { value: 7 }"),
        (
            "/one/main.ts",
            "import '../shared/globals'; type __Probe = Shared['value']; export {};",
        ),
        (
            "/two/main.ts",
            "import '../shared/globals'; type __Probe = Shared['value']; export {};",
        ),
    ] {
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            file,
            source,
            crate::FileLanguage::script_ts(),
        );
    }
    for main in ["/two/main.ts", "/one/main.ts", "/two/main.ts"] {
        assert_eq!(literal_number_at(&host, main, "__Probe"), 7.0, "{main}");
    }
}

#[test]
fn an_unconfigured_project_retains_imported_globals_outside_its_root() {
    let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/one".into(),
        "/one".into(),
        None,
    )]);
    for (file, source) in [
        ("/shared/globals.d.ts", "interface Shared { value: 7 }"),
        (
            "/one/main.ts",
            "import '../shared/globals'; type __Probe = Shared['value']; export {};",
        ),
    ] {
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            file,
            source,
            crate::FileLanguage::script_ts(),
        );
    }
    assert_eq!(literal_number_at(&host, "/one/main.ts", "__Probe"), 7.0);
}
