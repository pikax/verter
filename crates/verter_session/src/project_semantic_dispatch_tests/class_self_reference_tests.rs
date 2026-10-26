//! A value's members read one at a time: a member whose type names the
//! value it belongs to — a static method returning its class, a method
//! reading its class through a parameter of the class's type, an object
//! literal's method reading the variable that holds it or returning its
//! own `this` — reads the member it names where the value declares it,
//! never the whole surface it is itself part of.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on this exact
//! fixture with `export const a: null = null! as <probe>;` read off the
//! TS2322 message (`tsc --noEmit --strict --ignoreConfig`), under all four
//! `strictNullChecks` × `noImplicitAny` settings; every answer below is the
//! same under all four.

use super::checker_probe_lane_tests::mismatches;

fn assert_contextual_receiver(source: &str, probe: &str, expected: &str) {
    let source = format!("export {{}}; declare global {{ interface ThisType<T> {{}} }}\n{source}");
    let failures = super::checker_probe_lane_tests::mismatches_in_one_host_observing(
        &source,
        &[(probe, expected), (probe, expected)],
        |_| {},
    );
    assert!(failures.is_empty(), "{source}\n{}", failures.join("\n"));
}

#[test]
fn contextual_receiver_resolves_transitive_forward_methods() {
    for methods in [
        "q(){return this.r()},r(){return this.m()},m(){return this.n}",
        "m(){return this.n},r(){return this.m()},q(){return this.r()}",
    ] {
        assert_contextual_receiver(
            &format!("declare function defineComponent<D,M>(o:{{data():D;methods:M & ThisType<D & M>}}):{{[K in keyof M]:M[K] extends ()=>infer R?R:never}};
const comp=defineComponent({{data(){{return {{n:3 as const}}}},methods:{{{methods}}}}});"),
            "typeof comp.q",
            "3",
        );
    }
}

#[test]
fn contextual_receiver_intersections_select_the_first_marker() {
    for (markers, expected) in [
        ("ThisType<{n:3}> & ThisType<{n:5}>", "3"),
        ("ThisType<{n:5}> & ThisType<{n:3}>", "5"),
    ] {
        assert_contextual_receiver(
            &format!("function obj(){{return {{m(){{return this.n}}}} satisfies {markers};}}"),
            "ReturnType<ReturnType<typeof obj>['m']>",
            expected,
        );
    }
}

#[test]
fn contextual_receiver_unions_combine_marker_arguments() {
    for markers in [
        "ThisType<{n:3}> | ThisType<{n:5}>",
        "ThisType<{n:5}> | ThisType<{n:3}>",
    ] {
        assert_contextual_receiver(
            &format!("function obj(){{return {{m(){{return this.n}}}} satisfies {markers};}}"),
            "ReturnType<ReturnType<typeof obj>['m']>",
            "3 | 5",
        );
    }
}

#[test]
fn contextual_receiver_signature_this_precedes_the_marker() {
    assert_contextual_receiver(
        "function obj(){return {m(){return this.n}} satisfies {m(this:{n:5}):5} & ThisType<{n:3}>;}",
        "ReturnType<ReturnType<typeof obj>['m']>",
        "5",
    );
    assert_contextual_receiver(
        "function outer(){return {n:9 as const,make(){return {m:()=>this.n} satisfies {m(this:{n:5}):9} & ThisType<{n:3}>;}};}",
        "ReturnType<ReturnType<ReturnType<typeof outer>['make']>['m']>",
        "9",
    );
}

#[test]
fn contextual_receiver_plain_spreads_supply_literal_members() {
    for (members, expected) in [
        ("...{n:3 as const},m(){return this.n}", "3"),
        ("...{n:3 as const},n:5 as const,m(){return this.n}", "5"),
        ("n:5 as const,...{n:3 as const},m(){return this.n}", "3"),
    ] {
        assert_contextual_receiver(
            &format!("function obj(){{return {{{members}}};}}"),
            "ReturnType<ReturnType<typeof obj>['m']>",
            expected,
        );
    }
}

#[test]
fn contextual_receiver_binds_computed_methods() {
    assert_contextual_receiver(
        "const key='m' as const;function obj(){return {[key](){return this.n}} satisfies ThisType<{n:3}>;}",
        "ReturnType<ReturnType<typeof obj>['m']>",
        "3",
    );
}

#[test]
fn contextual_receiver_orders_store_dependencies_across_spreads() {
    assert_contextual_receiver(
        "declare function defineStore<S,G,A>(o:{state():S;getters:G & ThisType<S>;actions:A & ThisType<S & A & {[K in keyof G]:G[K] extends ()=>infer R?R:never}>}):S & A;
const store=defineStore({...{},actions:{read(){return this.double}},getters:{double(){return this.count}},state(){return {count:1 as const}}});",
        "ReturnType<typeof store.read>",
        "1",
    );
}

#[test]
fn contextual_receiver_spread_methods_retain_inferred_siblings() {
    for (methods, expected) in [
        ("m(){return this.n},...{q(){return this.m()}}", "3"),
        ("...{q(){return this.m()}},m(){return this.n}", "3"),
        (
            "m(){return this.n},...({q(){return this.m()},other:5 as const} as const)",
            "3",
        ),
        (
            "m(){return this.n},...{m(){return 5 as const}},q(){return this.m()}",
            "5",
        ),
    ] {
        assert_contextual_receiver(
            &format!("declare function defineComponent<D,M>(o:{{data():D;methods:M & ThisType<D & M>}}):{{[K in keyof M]:M[K] extends ()=>infer R?R:never}};
const comp=defineComponent({{data(){{return {{n:3 as const}}}},methods:{{{methods}}}}});"),
            "typeof comp.q",
            expected,
        );
    }
}

#[test]
fn contextual_this_marker_preserves_inferred_sibling_methods() {
    for methods in [
        "m(){return this.n}, q(){return this.m()}",
        "q(){return this.m()}, m(){return this.n}",
    ] {
        assert_contextual_receiver(
            &format!("declare function defineComponent<D,M>(o:{{data():D;methods:M & ThisType<D & M>}}):{{[K in keyof M]:M[K] extends ()=>infer R?R:never}};
const comp=defineComponent({{data(){{return {{n:3 as const}}}},methods:{{{methods}}}}});"),
            "typeof comp",
            "{ m: 3; q: 3; }",
        );
    }
}

#[test]
fn contextual_this_marker_preserves_inferred_siblings_through_aliases() {
    assert_contextual_receiver(
        "type Methods<D,M>=M & ThisType<D & M>;
declare function defineComponent<D,M>(o:{data():D;methods:Methods<D,M>}):{[K in keyof M]:M[K] extends ()=>infer R?R:never};
const comp=defineComponent({data(){return {n:3 as const}},methods:{m(){return this.n},q(){return this.m()}}});",
        "typeof comp",
        "{ m: 3; q: 3; }",
    );
}

/// The marker is the GLOBAL `ThisType`, merged from every declaration of
/// it: a module that augments it beside an earlier declaration — another
/// folder's module augmenting it the same way, as every module of a
/// project may — still names the marker. TypeScript answers `{ m: 3; }`
/// both alone and beside the sibling.
#[test]
fn contextual_this_marker_is_named_by_every_global_contributor() {
    let module = "export {}; declare global { interface ThisType<T> {} }
declare function defineComponent<D,M>(o:{data():D;methods:M & ThisType<D & M>}):{[K in keyof M]:M[K] extends ()=>infer R?R:never};
const comp=defineComponent({data(){return {n:3 as const}},methods:{m(){return this.n}}});";
    let rows = [("typeof comp", "{ m: 3; }")];
    let alone = super::checker_probe_lane_tests::mismatches(module, &rows);
    assert!(alone.is_empty(), "alone: {}", alone.join("\n"));
    // The earlier declaration in a sibling module's `declare global`, in
    // the project's library, and in a global script.
    for (label, earlier) in [
        (
            "beside a sibling",
            super::checker_probe_lane_tests::ProbeProject {
                files: &[("a/scenario.ts", module)],
                ..Default::default()
            },
        ),
        (
            "beside the library",
            super::checker_probe_lane_tests::ProbeProject {
                ambient_lib: Some("interface ThisType<T> {}"),
                ..Default::default()
            },
        ),
        (
            "beside a global script",
            super::checker_probe_lane_tests::ProbeProject {
                files: &[("a/globals.d.ts", "interface ThisType<T> {}")],
                ..Default::default()
            },
        ),
    ] {
        let beside = super::checker_probe_lane_tests::mismatches_in(earlier, module, &rows);
        assert!(beside.is_empty(), "{label}: {}", beside.join("\n"));
    }
}

/// Every automatic library that declares `ThisType` declares the marker,
/// not only the first: a reference written in a second library, which
/// reads that library's own declaration, names it as the first library's
/// does. TypeScript answers `3` with the alias's library alone and beside
/// an earlier one.
#[test]
fn contextual_this_marker_is_named_by_every_declaring_library() {
    use super::checker_probe_lane_tests::{
        mismatches_in, mismatches_on_host, probe_host, ProbeProject,
    };
    let reader = "export function obj() { return { m() { return this.n; } } satisfies LibMethods<{ n: 3 }>; }";
    let rows = [("ReturnType<ReturnType<typeof obj>['m']>", "3")];
    let second = "interface ThisType<T> {}\ntype LibMethods<T> = ThisType<T>;";
    let alone = mismatches_in(
        ProbeProject {
            ambient_lib: Some(second),
            ..Default::default()
        },
        reader,
        &rows,
    );
    assert!(alone.is_empty(), "alone: {}", alone.join("\n"));
    let project = ProbeProject {
        ambient_lib: Some("interface ThisType<T> {}"),
        ..Default::default()
    };
    let host = probe_host(project);
    register_second_library(&host, second);
    let beside = mismatches_on_host(&host, project, reader, &rows);
    assert!(beside.is_empty(), "beside a library: {}", beside.join("\n"));
}

/// Registers `source` as a second automatic library of `host`'s probe
/// project, after the one its [`ProbeProject`] registered.
///
/// [`ProbeProject`]: super::checker_probe_lane_tests::ProbeProject
fn register_second_library(host: &crate::VerterHost, source: &str) {
    host.workspace()
        .register_ambient_lib(verter_workspace::AmbientLibSpec {
            project_id: None,
            canonical_id: std::sync::Arc::from("lib.second.d.ts"),
            source: std::sync::Arc::from(source),
        })
        .expect("the second library registers against the probe's project");
}

/// A module that declares a module-scope `ThisType` beside its own
/// `declare global` one contributes to the global marker, but its
/// declaration identity names the module-scope type. A reader in another
/// module names the global through a declaration that addresses it — the
/// library's, or a later module's or script's global one — so it keeps
/// its contextual receiver; a module importing the namesake reads an
/// ordinary type. TypeScript answers `3` for the global reader and `4`
/// for the importer in every row.
#[test]
fn a_global_reader_beside_a_module_local_namesake_keeps_the_marker() {
    use super::checker_probe_lane_tests::{mismatches_in, ProbeProject};
    let shadowing =
        "export interface ThisType<T> { shadow: T }\ndeclare global { interface ThisType<T> {} }";
    let reader = "type Methods<T> = ThisType<T>;
export function obj() { return { m() { return this.n; } } satisfies Methods<{ n: 3 }>; }";
    let importer = "import type { ThisType as Local } from './a';
export function obj() { return { shadow: { n: 4 as const }, m() { return this.shadow.n; } } satisfies Local<{ n: 4 }> & { m(): 4 }; }";
    let global_rows = [("ReturnType<ReturnType<typeof obj>['m']>", "3")];
    let local_rows = [("ReturnType<ReturnType<typeof obj>['m']>", "4")];
    let later_module = "export {}; declare global { interface ThisType<T> {} }";
    let later_script = "interface ThisType<T> {}";
    let mut failures = Vec::new();
    for (label, files, ambient_lib) in [
        (
            "beside the library",
            &[("a.ts", shadowing)][..],
            Some("interface ThisType<T> {}"),
        ),
        (
            "before a later module",
            &[("a.ts", shadowing), ("b.ts", later_module)][..],
            None,
        ),
        (
            "before a later script",
            &[("a.ts", shadowing), ("b.d.ts", later_script)][..],
            None,
        ),
    ] {
        let project = ProbeProject {
            files,
            ambient_lib,
            ..Default::default()
        };
        for (role, source, rows) in [
            ("global reader", reader, &global_rows),
            ("importer", importer, &local_rows),
        ] {
            failures.extend(
                mismatches_in(project, source, rows)
                    .into_iter()
                    .map(|failure| format!("{label}, {role}: {failure}")),
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// With no library declaring `ThisType`, the global marker may be declared
/// ONLY by modules that also declare a module-scope namesake: each one's
/// `declare global` contribution is still a declaration of the global, and
/// a reader in another module names the marker through it, cold and warm.
/// The same holds for any global so declared: the reader's `Box` merges
/// every module's `declare global` members, each naming types and values
/// in its own module's scope: its local types and values, the types and
/// values it imports, and the values its own `declare global` declares. An
/// importer of a namesake reads an ordinary type. TypeScript answers the
/// receiver `3`, `Box['first']` `2`, `Box['loc']` `5`, `Box['lv']` `6`,
/// `Box['iv']` `8`, `Box['ix']` `7`, `Box['wr']['w']` `5` and `Box['gv']`
/// `9` (`Box['second']` `3`) for the global reader, and the receiver `4`
/// and `LocalBox['local']` `1` for the importer, in every row.
#[test]
fn a_global_declared_only_beside_module_local_namesakes_keeps_the_marker() {
    use super::checker_probe_lane_tests::{mismatches_on_host, probe_host, ProbeProject};
    let imported = "export type X = 7;
export const importedVal = 8 as const;";
    let shadowing = "import type { X } from './x';
import { importedVal } from './x';
export interface ThisType<T> { shadow: T }
declare global { interface ThisType<T> {} }
export interface Box { local: 1 }
type Local = 5;
type Wrap<T> = { w: T };
const localVal = 6 as const;
declare global { const globalVal: 9; }
declare global { interface Box { first: 2; loc: Local; lv: typeof localVal; iv: typeof importedVal; ix: X; wr: Wrap<Local>; gv: typeof globalVal } }";
    let second = "export interface ThisType<T> { other: T }
declare global { interface ThisType<T> {} }
export interface Box { other: 1 }
declare global { interface Box { second: 3 } }";
    let reader = "type Methods<T> = ThisType<T>;
export function obj() { return { m() { return this.n; } } satisfies Methods<{ n: 3 }>; }
export type First = Box['first'];";
    let importer = "import type { ThisType as Local, Box as LocalBox } from './a';
export function obj() { return { shadow: { n: 4 as const }, m() { return this.shadow.n; } } satisfies Local<{ n: 4 }> & { m(): 4 }; }
export type First = LocalBox['local'];";
    let probe = "ReturnType<ReturnType<typeof obj>['m']>";
    // Each row is read twice on one host: cold, then warm. The global
    // reader's `Box` is the merged global every `declare global` block
    // contributes to; the importer's is the module-scope one.
    let local_rows = [(probe, "4"), ("First", "1"), (probe, "4"), ("First", "1")];
    let mut failures = Vec::new();
    let shared = [
        (probe, "3"),
        ("First", "2"),
        ("Box['loc']", "5"),
        ("Box['lv']", "6"),
        ("Box['iv']", "8"),
        ("Box['ix']", "7"),
        ("Box['wr']['w']", "5"),
        ("Box['gv']", "9"),
    ];
    let one: Vec<_> = shared.iter().chain(&shared).copied().collect();
    let with_second: Vec<_> = shared
        .iter()
        .copied()
        .chain([("Box['second']", "3")])
        .collect();
    let two: Vec<_> = with_second.iter().chain(&with_second).copied().collect();
    for (label, files, global_rows) in [
        (
            "one shadowing module",
            &[("x.ts", imported), ("a.ts", shadowing)][..],
            &one[..],
        ),
        (
            "two shadowing modules",
            &[("x.ts", imported), ("a.ts", shadowing), ("c.ts", second)][..],
            &two[..],
        ),
    ] {
        let project = ProbeProject {
            files,
            ..Default::default()
        };
        for (role, source, rows) in [
            ("global reader", reader, global_rows),
            ("importer", importer, &local_rows[..]),
        ] {
            let host = probe_host(project);
            failures.extend(
                mismatches_on_host(&host, project, source, rows)
                    .into_iter()
                    .map(|failure| format!("{label}, {role}: {failure}")),
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Inside a module's `declare global` block a name the block declares —
/// a type or a value — is the global one, ahead of the module's own
/// namesake: the block's `Methods<T> = ThisType<T>` names the global
/// marker beside a module-scope `ThisType`, the global `Box`'s `self` and
/// `peer` members name the global `Box` and `Peer` beside module-scope
/// ones, `typeof value` reads the block's `value: 9` beside a module
/// `const value = 6`, and `Alias` the block's `2` beside a module `1`.
/// The module's own references keep its namesakes. TypeScript answers the
/// reader's receiver `3`, `Box['self']['peer']['p']` `4`, `Box['gv']` and
/// `Box['self']['gv']` `9` and `Box['al']` `2` — and the same through a
/// global `Solo` the module does not declare and the global value
/// `holder`'s annotation — and the importer's `LocalBox['local']` `1`,
/// `Outer` `6` and `OuterAlias` `1`, cold and warm.
#[test]
fn a_global_blocks_own_names_win_over_module_namesakes_inside_it() {
    use super::checker_probe_lane_tests::{mismatches_on_host, probe_host, ProbeProject};
    let augmenting = "export interface ThisType<T> { shadow: T }
export interface Box { local: 1 }
export interface Peer { local: 1 }
const value = 6 as const;
type Alias = 1;
export const outer: typeof value = value;
export type OuterAlias = Alias;
declare global {
    interface ThisType<T> {}
    type Methods<T> = ThisType<T>;
    const value: 9;
    type Alias = 2;
    interface Peer { p: 4 }
    interface Box { self: Box; peer: Peer; gv: typeof value; al: Alias }
    interface Solo { gv: typeof value; peer: Peer; al: Alias }
    const holder: { v: typeof value; a: Alias };
}";
    let reader =
        "export function obj() { return { m() { return this.n; } } satisfies Methods<{ n: 3 }>; }";
    let importer = "import type { Box as LocalBox, OuterAlias } from './a';
import { outer } from './a';
export type First = LocalBox['local'];
export type Outer = typeof outer;
export type Aliased = OuterAlias;";
    let global_rows = [
        ("ReturnType<ReturnType<typeof obj>['m']>", "3"),
        ("Box['self']['peer']['p']", "4"),
        ("Box['gv']", "9"),
        ("Box['self']['gv']", "9"),
        ("Box['al']", "2"),
        ("Solo['gv']", "9"),
        ("Solo['peer']['p']", "4"),
        ("Solo['al']", "2"),
        ("(typeof holder)['v']", "9"),
        ("(typeof holder)['a']", "2"),
    ];
    let local_rows = [("First", "1"), ("Outer", "6"), ("Aliased", "1")];
    let files = [("a.ts", augmenting)];
    let project = ProbeProject {
        files: &files,
        ..Default::default()
    };
    let mut failures = Vec::new();
    for (role, source, rows) in [
        ("global reader", reader, &global_rows[..]),
        ("importer", importer, &local_rows[..]),
    ] {
        let rows: Vec<_> = rows.iter().chain(rows).copied().collect();
        let host = probe_host(project);
        failures.extend(
            mismatches_on_host(&host, project, source, &rows)
                .into_iter()
                .map(|failure| format!("{role}: {failure}")),
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A module whose `declare global` block contributes to a global its own
/// import names (`import type { Box }` beside `declare global { interface
/// Box … }`) still contributes: the import names the imported type in the
/// module, the block the global. A reader in another module reads the
/// global's member, cold and warm, exactly as when the import is renamed;
/// the module's own `Box` stays the imported one. TypeScript answers
/// `Box['first']` `2`, the reader's `read` return `2` and `Mine` `1` in
/// every row.
#[test]
fn an_imported_namesake_does_not_hide_a_modules_global_contribution() {
    use super::checker_probe_lane_tests::{mismatches_on_host, probe_host, ProbeProject};
    let local = "export interface Box { local: 1 }";
    let imported = "import type { Box } from './local';
declare global { interface Box { first: 2 } }
export type Mine = Box['local'];";
    let renamed = "import type { Box as LocalBox } from './local';
declare global { interface Box { first: 2 } }
export type Mine = LocalBox['local'];";
    let reader = "declare const v: Box['first'];
export function read() { return v; }";
    let importer = "import type { Mine } from './a';";
    let global_rows = [
        ("Box['first']", "2"),
        ("ReturnType<typeof read>", "2"),
        ("Box['first']", "2"),
        ("ReturnType<typeof read>", "2"),
    ];
    let local_rows = [("Mine", "1"), ("Mine", "1")];
    let mut failures = Vec::new();
    for (label, augmenting) in [("imported namesake", imported), ("renamed import", renamed)] {
        let files = [("local.ts", local), ("a.ts", augmenting)];
        let project = ProbeProject {
            files: &files,
            ..Default::default()
        };
        for (role, source, rows) in [
            ("global reader", reader, &global_rows[..]),
            ("importer", importer, &local_rows[..]),
        ] {
            let host = probe_host(project);
            failures.extend(
                mismatches_on_host(&host, project, source, rows)
                    .into_iter()
                    .map(|failure| format!("{label}, {role}: {failure}")),
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A module's OWN `interface ThisType` — not in `declare global`, beside a
/// global one in one library or in two, or beside its own `declare global`
/// one — is a different symbol from the global marker: the literal's
/// receiver stays its contextual type, as TypeScript reads it.
#[test]
fn module_local_this_type_interface_is_not_the_contextual_this_marker() {
    let local = "interface ThisType<T> { shadow: T }
export function shadowed() { return { shadow: { n: 4 as const }, m() { return this.shadow.n; } } satisfies ThisType<{ n: 4 }> & { m(): 4 }; }";
    let rows = [("ReturnType<ReturnType<typeof shadowed>['m']>", "4")];
    let library = super::checker_probe_lane_tests::ProbeProject {
        ambient_lib: Some("interface ThisType<T> {}"),
        ..Default::default()
    };
    for (label, project, source) in [
        ("beside the library", library, local.to_owned()),
        (
            "beside its own global one",
            super::checker_probe_lane_tests::ProbeProject::default(),
            format!("declare global {{ interface ThisType<T> {{}} }}\n{local}"),
        ),
    ] {
        let failures = super::checker_probe_lane_tests::mismatches_in(project, &source, &rows);
        assert!(failures.is_empty(), "{label}: {}", failures.join("\n"));
    }
    let host = super::checker_probe_lane_tests::probe_host(library);
    register_second_library(&host, "interface ThisType<T> {}");
    let failures =
        super::checker_probe_lane_tests::mismatches_on_host(&host, library, local, &rows);
    assert!(
        failures.is_empty(),
        "beside two libraries: {}",
        failures.join("\n")
    );
}

#[test]
fn contextual_this_marker_is_inherited_by_nested_literals() {
    assert_contextual_receiver(
        "function obj(){return {nested:{m(){return this.n}}} satisfies {nested:{m():3}} & ThisType<{n:3}>;}",
        "ReturnType<ReturnType<typeof obj>['nested']['m']>",
        "3",
    );
}

#[test]
fn contextual_this_marker_stops_at_expression_boundaries() {
    for (nested, projection) in [
        ("[{n:5 as const,m(){return this.n}}]", "[number]"),
        ("hold({n:5 as const,m(){return this.n}})", ""),
        (
            "flag ? {n:5 as const,m(){return this.n}} : {n:5 as const,m(){return this.n}}",
            "",
        ),
    ] {
        let source = format!(
            "declare function hold<T>(o:T):T;
function obj(flag:boolean){{return {{nested:{nested},m(){{return this.n}}}} satisfies ThisType<{{n:3}}>;}}"
        );
        assert_contextual_receiver(
            &source,
            &format!("ReturnType<ReturnType<typeof obj>['nested']{projection}['m']>"),
            "5",
        );
        assert_contextual_receiver(&source, "ReturnType<ReturnType<typeof obj>['m']>", "3");
    }
}

#[test]
fn contextual_receiver_keeps_array_members_out_of_inferred_siblings() {
    assert_contextual_receiver(
        "declare function defineComponent<D,M>(o:{data():D;methods:M & ThisType<D & M>}):M;
const comp=defineComponent({data(){return {n:3 as const}},methods:{nested:[{n:5 as const,m(){return this.n}}],m(){return this.n}}});",
        "ReturnType<typeof comp.nested[number]['m']>",
        "5",
    );
    assert_contextual_receiver(
        "declare function defineComponent<D,M>(o:{data():D;methods:M & ThisType<D & M>}):M;
const comp=defineComponent({data(){return {n:3 as const}},methods:{nested:[{n:5 as const,m(){return this.n}}],m(){return this.n}}});",
        "ReturnType<typeof comp.m>",
        "3",
    );
}

#[test]
fn contextual_this_marker_survives_object_spreads() {
    for spread in ["{}", "extra"] {
        assert_contextual_receiver(
            &format!("const extra={{a:1}};function obj(){{return {{...{spread},m(){{return this.n}}}} satisfies ThisType<{{n:3}}>;}}"),
            "ReturnType<ReturnType<typeof obj>['m']>",
            "3",
        );
    }
    assert_contextual_receiver(
        "const extra={a:1};function obj(){return {m(){return this.n},...extra} satisfies ThisType<{n:3}>;}",
        "ReturnType<ReturnType<typeof obj>['m']>",
        "3",
    );
    assert_contextual_receiver(
        "function obj(){return {m(){return this.n},...{m(){return 5 as const}}} satisfies ThisType<{n:3}>;}",
        "ReturnType<ReturnType<typeof obj>['m']>",
        "5",
    );
}

#[test]
fn contextual_this_marker_is_found_in_union_contexts() {
    assert_contextual_receiver(
        "function obj(){return {m(){return this.n}} satisfies (ThisType<{n:3}> | {m():3});}",
        "ReturnType<ReturnType<typeof obj>['m']>",
        "3",
    );
}

#[test]
fn contextual_this_marker_yields_to_an_explicit_receiver() {
    assert_contextual_receiver(
        "function obj(){return {m(this:{n:5}){return this.n}} satisfies ThisType<{n:3}>;}",
        "ReturnType<ReturnType<typeof obj>['m']>",
        "5",
    );
    assert_contextual_receiver(
        "function obj(){return {m(this:{n:5}){return ()=>this.n}} satisfies ThisType<{n:3}>;}",
        "ReturnType<ReturnType<ReturnType<typeof obj>['m']>>",
        "5",
    );
}

#[test]
fn contextual_this_marker_types_top_level_literals() {
    assert_contextual_receiver(
        "const top={m(){return this.n}} satisfies ThisType<{n:3}>;",
        "ReturnType<typeof top.m>",
        "3",
    );
}

#[test]
fn contextual_this_marker_types_function_properties() {
    assert_contextual_receiver(
        "function obj(){return {m:function(){return this.n}} satisfies ThisType<{n:3}>;}",
        "ReturnType<ReturnType<typeof obj>['m']>",
        "3",
    );
}

#[test]
fn contextual_this_marker_survives_generic_aliases() {
    assert_contextual_receiver(
        "type Marker<T>=ThisType<T>;function obj(){return {m(){return this.n}} satisfies Marker<{n:3}>;}",
        "ReturnType<ReturnType<typeof obj>['m']>",
        "3",
    );
}

#[test]
fn contextual_this_marker_infers_getters_before_earlier_actions() {
    assert_contextual_receiver(
        "declare function defineStore<S,G,A>(o:{state():S;getters:G & ThisType<S>;actions:A & ThisType<S & A & {[K in keyof G]:G[K] extends ()=>infer R?R:never}>}):S & A;
const store=defineStore({actions:{read(){return this.double}},getters:{double(){return this.count}},state(){return {count:1 as const}}});",
        "ReturnType<typeof store.read>",
        "1",
    );
}

#[test]
fn contextual_this_marker_returning_this_preserves_the_marker_argument() {
    let source = "export {}; declare global { interface ThisType<T> {} }
function whole() { return { m() { return this; } } satisfies ThisType<{ n: 3 }>; }";
    let failures = mismatches(
        source,
        &[("ReturnType<ReturnType<typeof whole>['m']>", "{ n: 3; }")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn contextual_this_marker_uses_inferred_data_before_methods() {
    let source = "\
export {};
declare global { interface ThisType<T> {} }
declare function defineComponent<D, M>(o: { data(): D; methods: M & ThisType<D & M> }):
  { [K in keyof M]: M[K] extends () => infer R ? R : never };
const comp = defineComponent({
  data() { return { d0: 0 as const, d1: 1 as const }; },
  methods: { m0() { return this.d0; }, m1() { return this.d1; } },
});
";
    let failures = mismatches(source, &[("typeof comp", "{ m0: 0; m1: 1; }")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A component of `count` data members and as many methods, each method
/// returning its data member through the contextual `this` marker, and the
/// mapped answer the checker gives for it.
fn this_marker_component(count: usize) -> (String, String) {
    let data = (0..count)
        .map(|i| format!("d{i}: {i} as const"))
        .collect::<Vec<_>>()
        .join(", ");
    let methods = (0..count)
        .map(|i| format!("m{i}() {{ return this.d{i}; }}"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "export {{}}; declare global {{ interface ThisType<T> {{}} }}
declare function defineComponent<D, M>(o: {{ data(): D; methods: M & ThisType<D & M> }}):
  {{ [K in keyof M]: M[K] extends () => infer R ? R : never }};
const comp = defineComponent({{ data() {{ return {{ {data} }}; }}, methods: {{ {methods} }} }});"
    );
    let expected = format!(
        "{{ {} }}",
        (0..count)
            .map(|i| format!("m{i}: {i};"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    (source, expected)
}

/// The relation checks one host spends answering the component twice.
fn this_marker_relation_work(count: usize) -> u64 {
    let (source, expected) = this_marker_component(count);
    let mut relation_work = 0;
    let failures = super::checker_probe_lane_tests::mismatches_in_one_host_observing(
        &source,
        &[("typeof comp", &expected), ("typeof comp", &expected)],
        |dispatch| relation_work = dispatch.graph().stats_snapshot().relation_check_count,
    );
    assert!(
        failures.is_empty(),
        "{count}: {}",
        failures.join(
            "
"
        )
    );
    relation_work
}

/// The component at 5, 10 and 15 members: the gate version of the 20-, 100-
/// and 400-member test in the nightly scale tier. Each answers exactly; its
/// relation checks grow by the same amount per step, so they are affine in
/// the members, and at 20, 100 and 400 members they meet the full-size
/// test's checks: increasing, and the last step at most five times the first.
#[test]
fn contextual_this_marker_relation_work_is_affine_and_meets_the_full_size_check() {
    let sizes = [5, 10, 15];
    let work = crate::scale_gate::affine(
        "the contextual receiver",
        sizes,
        vec![crate::scale_gate::Counter {
            name: "relation checks",
            values: sizes
                .iter()
                .map(|&count| this_marker_relation_work(count))
                .collect(),
        }],
    );
    let [small, medium, large] = [20, 100, 400].map(|count| work.at("relation checks", count));
    assert!(small > 0 && small < medium && medium < large);
    assert!(
        large - medium <= 5 * (medium - small),
        "member growth repeats prior relation work: {small}, {medium}, {large}"
    );
}

#[test]
fn contextual_this_marker_and_lexical_arrow_controls() {
    let source = "export {};
declare global { interface ThisType<T> {} }
type Context = { m(): 3 } & ({} & ThisType<{ n: 3 }>);
function marked() { return { m() { return this.n; } } satisfies Context; }
function plain() { return { n: 7 as const, m() { return this.n; } }; }
function contextual() { return { n: 7 as const, m() { return this.n; } } satisfies { n: number; m(): number }; }
declare function hold<T>(o: { nested: T }): T;
const authored = hold({ nested: { value: null as any, method() { return this.value; } } });
const outer = { n: 9 as const, make() { return { n: 3 as const, arrow: () => this.n, m() { return this.n; } } satisfies ThisType<{ n: 3 }>; } };
";
    let failures = mismatches(
        source,
        &[
            ("ReturnType<ReturnType<typeof marked>['m']>", "3"),
            ("ReturnType<ReturnType<typeof plain>['m']>", "7"),
            ("ReturnType<ReturnType<typeof contextual>['m']>", "number"),
            ("typeof authored.value", "any"),
            ("ReturnType<typeof authored.method>", "any"),
            ("ReturnType<ReturnType<typeof outer.make>['arrow']>", "9"),
            ("ReturnType<ReturnType<typeof outer.make>['m']>", "3"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let failures = mismatches(
        "type ThisType<T> = { shadow: T };
export function shadowed() { return { shadow: { n: 4 as const }, m() { return this.shadow.n; } } satisfies ThisType<{ n: 4 }> & { m(): 4 }; }",
        &[("ReturnType<ReturnType<typeof shadowed>['m']>", "4")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let failures = super::checker_probe_lane_tests::mismatches_in(
        super::checker_probe_lane_tests::ProbeProject {
            ambient_lib: Some("interface ThisType<T> {}"),
            ..Default::default()
        },
        &source.replace("declare global { interface ThisType<T> {} }", ""),
        &[("ReturnType<ReturnType<typeof marked>['m']>", "3")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let failures = super::checker_probe_lane_tests::mismatches_in(
        super::checker_probe_lane_tests::ProbeProject {
            compiler_options: Some(r#"{ "strict": true, "noImplicitThis": false }"#),
            ..Default::default()
        },
        source,
        &[("ReturnType<ReturnType<typeof marked>['m']>", "any")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn contextual_this_marker_types_store_getters_and_actions() {
    let source = "export {};
declare global { interface ThisType<T> {} }
declare function defineStore<S, G, A>(o: {
  state(): S;
  getters: G & ThisType<S>;
  actions: A & ThisType<S & A & { [K in keyof G]: G[K] extends () => infer R ? R : never }>;
}): S & A & { [K in keyof G]: G[K] extends () => infer R ? R : never };
const store = defineStore({ state() { return { count: 1 as const }; },
  getters: { doubled() { return this.count; } },
  actions: { read() { return this.doubled; }, stateValue() { return this.count; } },
});";
    let failures = mismatches(
        source,
        &[
            ("typeof store.doubled", "1"),
            ("ReturnType<typeof store.read>", "1"),
            ("ReturnType<typeof store.stateValue>", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const STATICS: &str = "\
export class S { static k = 1; static s() { return this; } static e() { return S; } static kk() { return S.k; } static viaS() { return this.s(); } }
export class B0s { static bk = 1; static bm() { return 'b' as string; } }
export class S2 extends B0s { static r() { return S2.bk; } static q() { return S2.bm(); } static w() { return this.bk; } static me() { return S2; } static ann: number = 1; static y() { return this.ann; } static x() { return this.bm(); } }
export class G2<T> { t!: T; static k = 1; static r() { return G2.k; } }
";

/// A static method returning its class — as `this`, by name, or through a
/// sibling returning `this` — returns the class's constructor type, read
/// by reference; a static reading its class's own or inherited static by
/// name or through `this` reads that static alone.
///
/// Measured on TypeScript 7.0.2 (all four `strictNullChecks` ×
/// `noImplicitAny` settings): `ReturnType<typeof S.s>`, `ReturnType<typeof
/// S.e>` and `ReturnType<typeof S.viaS>` are `typeof S`, and `typeof S`
/// extends the first two and they extend it (`1` each way);
/// `ReturnType<typeof S.s>['k']`, `ReturnType<typeof S.e>['k']` and
/// `ReturnType<typeof S.kk>` are `number`; over `S2 extends B0s`,
/// `ReturnType<typeof S2.r>`, `…w` and `…y` are `number`, and `…q` and
/// `…x` are `string`;
/// `ReturnType<typeof G2.r>` is `number`. Read through the whole
/// constructor type, `ReturnType<(typeof S)['s']>['k']`,
/// `ReturnType<(typeof S)['e']>['k']`, `ReturnType<(typeof S)['kk']>`
/// and `ReturnType<(typeof S2)['r']>` are `number` and
/// `ReturnType<(typeof S2)['q']>` is `string`.
#[test]
fn a_static_reading_its_own_class_reads_the_member_alone() {
    let failures = mismatches(
        STATICS,
        &[
            ("ReturnType<typeof S.s> extends typeof S ? 1 : 0", "1"),
            ("typeof S extends ReturnType<typeof S.s> ? 1 : 0", "1"),
            ("ReturnType<typeof S.e> extends typeof S ? 1 : 0", "1"),
            ("typeof S extends ReturnType<typeof S.e> ? 1 : 0", "1"),
            ("ReturnType<typeof S.viaS> extends typeof S ? 1 : 0", "1"),
            ("ReturnType<typeof S.s>['k']", "number"),
            ("ReturnType<typeof S.e>['k']", "number"),
            ("ReturnType<typeof S.kk>", "number"),
            ("ReturnType<typeof S2.r>", "number"),
            ("ReturnType<typeof S2.q>", "string"),
            ("ReturnType<typeof S2.w>", "number"),
            ("ReturnType<typeof S2.y>", "number"),
            ("ReturnType<typeof S2.x>", "string"),
            ("ReturnType<typeof G2.r>", "number"),
            ("ReturnType<(typeof S)['s']>['k']", "number"),
            ("ReturnType<(typeof S)['e']>['k']", "number"),
            ("ReturnType<(typeof S)['kk']>", "number"),
            ("ReturnType<(typeof S2)['r']>", "number"),
            ("ReturnType<(typeof S2)['q']>", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A static returning its class by name, beside statics that read inherited
/// statics through `this`, returns the class's constructor type: over
/// `S2 extends B0s`, `ReturnType<typeof S2.me>` is `typeof S2` (measured on
/// TypeScript 7.0.2, all four settings).
#[test]
#[ignore = "a static returning its class by name is the class's constructor type beside statics reading inherited statics through `this`"]
fn a_static_returning_its_derived_class_is_its_constructor_type() {
    let failures = mismatches(
        STATICS,
        &[("ReturnType<typeof S2.me> extends typeof S2 ? 1 : 0", "1")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const THROUGH_PARAMETERS: &str = "\
export class H2 { v = 1; m(h: H2) { return h.v; } n(h: H2) { return h.m(h); } }
export class H3 { v = 1; m(h: H3) { const o: H3 = h; return o.v; } k(h: H3) { return h.m(h); } }
";

/// A method reading its own class through a parameter of the class's type
/// (directly, through a local of that type, or by calling a sibling with
/// the parameter) reads the member where the class declares it.
///
/// Measured on TypeScript 7.0.2 (all four settings): `ReturnType<H2['m']>`,
/// `ReturnType<H2['n']>`, `ReturnType<H3['m']>` and `ReturnType<H3['k']>`
/// are `number`.
#[test]
fn a_method_reading_its_class_through_a_parameter_reads_the_member() {
    let failures = mismatches(
        THROUGH_PARAMETERS,
        &[
            ("ReturnType<H2['m']>", "number"),
            ("ReturnType<H2['n']>", "number"),
            ("ReturnType<H3['m']>", "number"),
            ("ReturnType<H3['k']>", "number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const LITERAL_BY_NAME: &str = "\
export const obj = { v: 1, m() { return obj.v; }, n() { return obj.m(); } };
export const o2 = { a: { b: 1 }, m() { return o2.a.b; }, n() { return o2.m(); }, self() { return o2; } };
";

/// An object literal's method reading the variable that holds the literal
/// reads the member it names where the literal declares it.
///
/// Measured on TypeScript 7.0.2 (all four settings): `ReturnType<typeof
/// obj.m>`, `ReturnType<typeof obj.n>`, `ReturnType<typeof o2.m>`,
/// `ReturnType<typeof o2.n>`, `ReturnType<typeof o2.self>['a']['b']` and,
/// through the whole literal, `ReturnType<(typeof obj)['m']>` are `number`.
#[test]
fn a_literal_method_reading_its_variable_reads_the_member() {
    let failures = mismatches(
        LITERAL_BY_NAME,
        &[
            ("ReturnType<typeof obj.m>", "number"),
            ("ReturnType<typeof obj.n>", "number"),
            ("ReturnType<typeof o2.m>", "number"),
            ("ReturnType<typeof o2.n>", "number"),
            ("ReturnType<typeof o2.self>['a']['b']", "number"),
            ("ReturnType<(typeof obj)['m']>", "number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const LITERAL_THIS: &str = "\
export const own = { v: 1, me() { return this; } };
export function selfFn() { return { v: 1, me() { return this; } }; }
export function viaLocal() { const o = { w: 'a' as string, me() { return this; } }; return o; }
";

/// An object literal whose method returns its own `this` has the checker's
/// recursive anonymous type, which prints `{ v: number; me(): ...; }`: a
/// variable's literal is named by `typeof own`, and a literal a function
/// returns is its own identity. Either way the type behaves as the
/// checker's one type does — its members read, `me` returns the literal
/// again, and it relates to itself both ways but not to a one-level copy.
/// The prints differ (the checker elides the recursion; this lane names
/// `typeof own`, or prints the literal with `this` at the recursion),
/// which the ledger records as presentation only; these rows are the
/// effect equality.
///
/// Measured on TypeScript 7.0.2 (all four settings under `strict`; with
/// `noImplicitThis` off a literal's `this` is `any` instead):
/// `ReturnType<typeof own.me>['v']` and `ReturnType<ReturnType<typeof
/// own.me>['me']>['v']` and, through the whole literal,
/// `ReturnType<(typeof own)['me']>['v']` are `number`, and `typeof own
/// extends ReturnType<typeof own.me>` is `1`; over `selfFn` the return is `{ v:
/// number; me(): ...; }`, the `v` of the return, of `me`'s return and of
/// `me`'s return's `me`'s return are `number`, the return and `me`'s
/// return extend each other (`1`, `1`), the return extends `{ v: number
/// }` (`1`), and `{ v: number; me(): { v: number } }` does not extend the
/// return (`0`); over `viaLocal` the return is `{ w: string; me(): ...;
/// }`, the `w` of `me`'s return is `string`, and the return extends `me`'s
/// return (`1`).
#[test]
fn a_literal_returning_its_this_is_one_recursive_type() {
    let failures = mismatches(
        LITERAL_THIS,
        &[
            ("ReturnType<typeof own.me>['v']", "number"),
            ("ReturnType<ReturnType<typeof own.me>['me']>['v']", "number"),
            ("typeof own extends ReturnType<typeof own.me> ? 1 : 0", "1"),
            ("ReturnType<(typeof own)['me']>['v']", "number"),
            ("ReturnType<typeof selfFn>['v']", "number"),
            ("ReturnType<ReturnType<typeof selfFn>['me']>['v']", "number"),
            (
                "ReturnType<ReturnType<ReturnType<typeof selfFn>['me']>['me']>['v']",
                "number",
            ),
            (
                "ReturnType<typeof selfFn> extends ReturnType<ReturnType<typeof selfFn>['me']> ? 1 : 0",
                "1",
            ),
            (
                "ReturnType<ReturnType<typeof selfFn>['me']> extends ReturnType<typeof selfFn> ? 1 : 0",
                "1",
            ),
            ("ReturnType<typeof selfFn> extends { v: number } ? 1 : 0", "1"),
            (
                "{ v: number; me(): { v: number } } extends ReturnType<typeof selfFn> ? 1 : 0",
                "0",
            ),
            ("ReturnType<ReturnType<typeof viaLocal>['me']>['w']", "string"),
            (
                "ReturnType<typeof viaLocal> extends ReturnType<ReturnType<typeof viaLocal>['me']> ? 1 : 0",
                "1",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const LITERAL_GETTER: &str = "\
export const og = { n: 1, get self() { return this; } };
export function useOg() { return og.self.n; }
export function useOgDeep() { return og.self.self.n; }
export function getterFn() { return { n: 'a' as string, get self() { return this; } }; }
";

/// An object literal's getter returning its own `this` reads as the
/// literal, as a method returning `this` does.
///
/// Measured on TypeScript 7.0.2 (all four settings under `strict`):
/// `typeof og.self.n` is `number`, `typeof og extends typeof og.self` is
/// `1`, `ReturnType<typeof useOg>` and `ReturnType<typeof useOgDeep>` are
/// `number`; over `getterFn`, the return's `self`'s `n` and its `self`'s
/// `self`'s `n` are `string`, and the return's `self` extends the return
/// (`1`).
#[test]
fn a_literal_getter_returning_its_this_reads_the_literal() {
    let failures = mismatches(
        LITERAL_GETTER,
        &[
            ("typeof og.self.n", "number"),
            ("typeof og extends typeof og.self ? 1 : 0", "1"),
            ("ReturnType<typeof useOg>", "number"),
            ("ReturnType<typeof useOgDeep>", "number"),
            ("ReturnType<typeof getterFn>['self']['n']", "string"),
            ("ReturnType<typeof getterFn>['self']['self']['n']", "string"),
            (
                "ReturnType<typeof getterFn>['self'] extends ReturnType<typeof getterFn> ? 1 : 0",
                "1",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An object literal's getter is a PROPERTY of its return type on the
/// literal's whole type, never a method.
///
/// Measured on TypeScript 7.0.2 (all four settings): over `const q = { get
/// g() { return 1; } }`, `typeof q extends { g: number }` is `1`; over
/// `og`, `typeof og extends { self: Function }` is `0` and `typeof og.self
/// extends typeof og` is `1`. The lane reads the getter as a method on the
/// whole type (`{ g: () => number }`), so the first two answer `0` and `1`
/// and the last stays an unreduced conditional.
#[test]
#[ignore = "an object literal's getter is a property of its return type on the literal's whole type"]
fn a_literal_getter_is_a_property_of_the_whole_literal() {
    let source = "\
export const q = { get g() { return 1; } };
export const og = { n: 1, get self() { return this; } };
";
    let failures = mismatches(
        source,
        &[
            ("typeof q extends { g: number } ? 1 : 0", "1"),
            ("typeof og extends { self: Function } ? 1 : 0", "0"),
            ("typeof og.self extends typeof og ? 1 : 0", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Without `noImplicitThis` an object literal's method or getter reads
/// `this` as `any`: the `this` keyword, a member read or a call through
/// it, and an arrow the method creates. A name reading the variable that
/// holds the literal still reads the literal.
///
/// Measured on TypeScript 7.0.2 with `--strict --noImplicitThis false`
/// (all four `strictNullChecks` × `noImplicitAny` settings alike):
/// `ReturnType<typeof own.me>`, `…own.w>`, `…own.c>`,
/// `ReturnType<ReturnType<typeof own.a>>` and `typeof og.self` are `any`
/// (`0 extends 1 & T` holds of each), `ReturnType<typeof own.o>` is
/// `number`, and over the literal a function returns,
/// `ReturnType<ReturnType<typeof inner>['me']>` and `…['w']>` are `any`.
/// With `noImplicitThis` on, the same members read the literal
/// ([`a_literal_getter_returning_its_this_reads_the_literal`]).
#[test]
fn a_literal_this_is_any_without_no_implicit_this() {
    let source = "\
export const own = { v: 1, me() { return this; }, w() { return this.v; }, c() { return this.w(); }, a() { return () => this; }, o() { return own.v; } };
export const og = { n: 1, get self() { return this; } };
export function inner() { return { v: 1, me() { return this; }, w() { return this.v; } }; }
";
    for compiler_options in [
        r#"{ "strict": true, "noImplicitThis": false }"#,
        r#"{ "strict": true, "noImplicitThis": false, "noImplicitAny": false }"#,
        r#"{ "strict": true, "noImplicitThis": false, "strictNullChecks": false }"#,
        r#"{ "strict": true, "noImplicitThis": false, "strictNullChecks": false, "noImplicitAny": false }"#,
    ] {
        let failures = super::checker_probe_lane_tests::mismatches_in(
            super::checker_probe_lane_tests::ProbeProject {
                files: &[],
                compiler_options: Some(compiler_options),
                ambient_lib: None,
            },
            source,
            &[
                ("ReturnType<typeof own.me>", "any"),
                ("ReturnType<typeof own.w>", "any"),
                ("ReturnType<typeof own.c>", "any"),
                ("ReturnType<ReturnType<typeof own.a>>", "any"),
                ("ReturnType<typeof own.o>", "number"),
                ("typeof og.self", "any"),
                ("ReturnType<ReturnType<typeof inner>['me']>", "any"),
                ("ReturnType<ReturnType<typeof inner>['w']>", "any"),
            ],
        );
        assert!(
            failures.is_empty(),
            "{compiler_options}:\n{}",
            failures.join("\n")
        );
    }
}

/// A class expression's members read through the value holding it: a
/// top-level `const` initializer, an instance property initializer read
/// through an instantiation of its class, and a static property
/// initializer.
///
/// Measured on TypeScript 7.0.2 (all four settings):
/// `ReturnType<InstanceType<typeof K>['m']>` is `number`, `…['s']>` is
/// `string`, `ReturnType<typeof K.st>` is `number`, and
/// `ReturnType<ReturnType<InstanceType<typeof K>['me']>['m']>` is
/// `number`; `InstanceType<Outer2<string>['inner']>['u']` is `string`,
/// `ReturnType<InstanceType<Outer2<string>['inner']>['g']>` is `number`,
/// and `ReturnType<InstanceType<typeof Outer2.sinner>['m']>` is `number`.
/// The lane answers every row with the missing-member marker.
#[test]
#[ignore = "a class expression held by a `const` or a property initializer serves its members"]
fn a_class_expression_initializer_serves_its_members() {
    let source = "\
export class Outer2<U> { inner = class { u!: U; me() { return this; } g() { return 1; } }; static sinner = class { w = 1; m() { return this.w; } }; }
export const K = class { v = 1; m() { return this.v; } s() { return 'a' as string; } me() { return this; } static st() { return 2; } };
";
    let failures = mismatches(
        source,
        &[
            ("ReturnType<InstanceType<typeof K>['m']>", "number"),
            ("ReturnType<InstanceType<typeof K>['s']>", "string"),
            ("ReturnType<typeof K.st>", "number"),
            (
                "ReturnType<ReturnType<InstanceType<typeof K>['me']>['m']>",
                "number",
            ),
            ("InstanceType<Outer2<string>['inner']>['u']", "string"),
            (
                "ReturnType<InstanceType<Outer2<string>['inner']>['g']>",
                "number",
            ),
            (
                "ReturnType<InstanceType<typeof Outer2.sinner>['m']>",
                "number",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A method called directly on a `new` expression or on a call's result
/// reads the method through the constructed or returned value, and a
/// method reading `this.v` or calling `this.m()` reads the receiver's
/// instantiation.
///
/// Measured on TypeScript 7.0.2 (all four settings): `ReturnType<typeof
/// …>` of `n1`, `n2`, `g1`, `g6` and `useOwn` is `number`, of `g2`
/// `string`, and `ReturnType<typeof g3>['v']` is `boolean`. The lane reads
/// the member call on such a receiver as an unmodelled position (the same
/// methods called through a `const` or a parameter of the class type
/// already read `number` and `string`).
#[test]
#[ignore = "a method called on a `new` expression or a call's result reads the receiver's member"]
fn a_method_called_on_a_constructed_value_reads_the_receiver() {
    let source = "\
export class G<T> { constructor(public v: T) {} get() { return this.v; } call() { return this.get(); } me() { return this; } }
export class N { v = 1; get() { return this.v; } call() { return this.get(); } }
export class GF<T> { f!: T; get() { return this.f; } }
export function n1() { return new N().get(); }
export function n2() { return new N().call(); }
export function g1() { return new G(1).get(); }
export function g2() { return new G('s').call(); }
export function g3() { return new G(true).me(); }
export function g6() { return new GF<number>().get(); }
export const own = { v: 1, me() { return this; } };
export function useOwn() { return own.me().v; }
";
    let failures = mismatches(
        source,
        &[
            ("ReturnType<typeof n1>", "number"),
            ("ReturnType<typeof n2>", "number"),
            ("ReturnType<typeof g1>", "number"),
            ("ReturnType<typeof g2>", "string"),
            ("ReturnType<typeof g3>['v']", "boolean"),
            ("ReturnType<typeof g6>", "number"),
            ("ReturnType<typeof useOwn>", "number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `this` in a module-level function declaration has no receiver the
/// checker can type: it is `any` (with TS2683 under `noImplicitThis`),
/// read directly, through a local copy or through a member.
///
/// Measured on TypeScript 7.0.2 (all four `strictNullChecks` ×
/// `noImplicitAny` settings alike, `noImplicitThis` on and off):
/// `ReturnType<typeof f>`, `ReturnType<typeof g>` and `ReturnType<typeof
/// m>` are `any`, and `0 extends 1 & ReturnType<typeof f> ? 1 : 0` is `1`.
#[test]
fn this_in_a_module_function_is_any() {
    let source = "\
export function f() { return this; }
export function g() { const t = this; return t; }
export function m() { return this.x; }
";
    for compiler_options in [None, Some(r#"{ "strict": true, "noImplicitThis": false }"#)] {
        let failures = super::checker_probe_lane_tests::mismatches_in(
            super::checker_probe_lane_tests::ProbeProject {
                files: &[],
                compiler_options,
                ambient_lib: None,
            },
            source,
            &[
                ("ReturnType<typeof f>", "any"),
                ("ReturnType<typeof g>", "any"),
                ("ReturnType<typeof m>", "any"),
                ("0 extends 1 & ReturnType<typeof f> ? 1 : 0", "1"),
            ],
        );
        assert!(
            failures.is_empty(),
            "{compiler_options:?}:\n{}",
            failures.join("\n")
        );
    }
}

/// Object literals whose method returns the next: whether a literal's
/// members name its own `this` is read by one walk of each node however
/// deep the nest, so the walks grow by the same count for every level a
/// nest adds. A nested literal's members were walked again for every
/// literal around it, the square of the nesting.
///
/// Measured on TypeScript 7.0.2 (all four settings): `ReturnType<typeof pf>
/// extends object ? 1 : 2` is `1` over the 8-, 16- and 32-deep nests.
#[test]
fn an_object_method_nest_walks_each_member_once_for_its_own_this() {
    let walked = |depth: usize| {
        let source = format!(
            "export function pf(b: boolean) {{ return {}1{}; }}\n",
            "{ m() { return ".repeat(depth),
            "; } }".repeat(depth)
        );
        let before = super::flow_return::receiver_walk_visits_for_tests();
        assert_eq!(
            mismatches(
                &source,
                &[("ReturnType<typeof pf> extends object ? 1 : 2", "1")]
            ),
            Vec::<String>::new()
        );
        super::flow_return::receiver_walk_visits_for_tests() - before
    };
    let [eight, sixteen, thirty_two] = [8, 16, 32].map(walked);
    assert_eq!(
        thirty_two - sixteen,
        2 * (sixteen - eight),
        "the walks grow linearly with the nest: {eight}, {sixteen}, {thirty_two}"
    );
}

/// Full-size workloads, run by the nightly scale tier (`.config/nextest.toml`).
/// Each has a gate version outside this module.
mod nightly_scale {
    #[test]
    fn contextual_this_marker_scales_across_object_members() {
        let mut work = Vec::new();
        for count in [20, 100, 400] {
            let data = (0..count)
                .map(|i| format!("d{i}: {i} as const"))
                .collect::<Vec<_>>()
                .join(", ");
            let methods = (0..count)
                .map(|i| format!("m{i}() {{ return this.d{i}; }}"))
                .collect::<Vec<_>>()
                .join(", ");
            let source = format!(
                "export {{}}; declare global {{ interface ThisType<T> {{}} }}
    declare function defineComponent<D, M>(o: {{ data(): D; methods: M & ThisType<D & M> }}):
      {{ [K in keyof M]: M[K] extends () => infer R ? R : never }};
    const comp = defineComponent({{ data() {{ return {{ {data} }}; }}, methods: {{ {methods} }} }});"
            );
            let expected = format!(
                "{{ {} }}",
                (0..count)
                    .map(|i| format!("m{i}: {i};"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let mut relation_work = 0;
            let failures = super::super::checker_probe_lane_tests::mismatches_in_one_host_observing(
                &source,
                &[("typeof comp", &expected), ("typeof comp", &expected)],
                |dispatch| relation_work = dispatch.graph().stats_snapshot().relation_check_count,
            );
            assert!(failures.is_empty(), "{count}: {}", failures.join("\n"));
            work.push(relation_work);
        }
        let [small, medium, large] = work.as_slice() else {
            unreachable!()
        };
        eprintln!("contextual receiver relation work (20, 100, 400): {work:?}");
        assert!(*small > 0 && small < medium && medium < large);
        assert!(
            large - medium <= 5 * (medium - small),
            "member growth repeats prior relation work: {work:?}"
        );
    }
}
