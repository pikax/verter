//! The callback-check contract fixtures as complete SFCs for the production
//! IDE route.
//!
//! [`super::fixtures`] drives the reference generator from bare templates with
//! declared contracts. These are the same contract cases written as real
//! single-file components — script bindings, props, `reactive` state and typed
//! components supply every type and contextual contract — so the production
//! `CompileTarget::IDE` emitter and both real providers can be held to the
//! exact diagnostics they must produce. Each fixture carries a script canary
//! (`TS2322` on `flowCanary`) that proves a provider checked the file before an
//! empty template result is accepted. Spans are recorded while the source is
//! written, never searched for afterwards.

use super::fixtures::{Expected, HoverExpectation};
use super::seam::Span;

/// One SFC and exactly the diagnostics a provider must report for it.
#[derive(Debug, Clone)]
pub struct SfcFixture {
    pub name: String,
    /// TypeScript (`lang="ts"`) or JavaScript script.
    pub typescript: bool,
    pub source: String,
    /// Exactly the error diagnostics a provider must report (canary included),
    /// sorted.
    pub expected: Vec<Expected>,
    pub hovers: Vec<HoverExpectation>,
    /// Callbacks the template authors under a condition (each must be guarded).
    pub guarded_callbacks: usize,
    /// Authored `v-if` / `v-else-if` conditions.
    pub conditions: usize,
    /// The canary diagnostic's span.
    pub canary: Span,
}

/// How a narrowed reference is rooted in production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfcRoot {
    /// A `defineProps` member: `__props.u`, a readonly property of a const.
    Prop,
    /// A `<script setup>` const: `u`, a constant identifier.
    Constant,
    /// A `reactive` member: `state.u`, a mutable property access.
    Reactive,
}

impl SfcRoot {
    pub const ALL: [SfcRoot; 3] = [SfcRoot::Prop, SfcRoot::Constant, SfcRoot::Reactive];

    fn label(self) -> &'static str {
        match self {
            SfcRoot::Prop => "prop",
            SfcRoot::Constant => "constant",
            SfcRoot::Reactive => "reactive",
        }
    }

    /// The template expression naming the root.
    fn expr(self) -> &'static str {
        match self {
            SfcRoot::Reactive => "state.u",
            _ => "u",
        }
    }

    /// The identifier a callback parameter must use to shadow the root.
    fn shadow(self) -> &'static str {
        match self {
            SfcRoot::Reactive => "state",
            _ => "u",
        }
    }

    fn declare(self, ty: &str, script: &mut Sfc) {
        script.push(&match self {
            SfcRoot::Prop => format!("defineProps<{{ u: {ty} }}>();\n"),
            SfcRoot::Constant => format!("const u = {{}} as {ty};\n"),
            SfcRoot::Reactive => format!("const state = reactive({{ u: {{}} as {ty} }});\n"),
        });
    }
}

const TS2322: u32 = 2322;
const TS2339: u32 = 2339;
const TS2345: u32 = 2345;

/// Writes an SFC while recording authored spans.
struct Sfc {
    text: String,
}

impl Sfc {
    fn typescript() -> Self {
        Self {
            text: "<script setup lang=\"ts\">\n\
                   import { reactive, defineComponent, type PropType, type SlotsType } from 'vue';\n"
                .into(),
        }
    }

    fn javascript() -> Self {
        Self {
            text: "<script setup>\nimport { reactive } from 'vue';\n".into(),
        }
    }

    fn push(&mut self, text: &str) -> &mut Self {
        self.text.push_str(text);
        self
    }

    /// Append `text` and return its span.
    fn mark(&mut self, text: &str) -> Span {
        let start = self.text.len() as u32;
        self.text.push_str(text);
        Span::new(start, self.text.len() as u32)
    }

    /// Close the script with the canary and open the template.
    fn template(&mut self, typescript: bool) -> Span {
        let canary = if typescript {
            self.push("const ");
            let span = self.mark("flowCanary");
            self.push(": number = \"checked\";\n");
            span
        } else {
            self.push("/** @type {number} */\nconst ");
            let span = self.mark("flowCanary");
            self.push(" = \"checked\";\n");
            span
        };
        self.push("</script>\n<template>\n");
        canary
    }

    fn finish(mut self) -> String {
        self.text.push_str("</template>\n");
        self.text
    }
}

struct Draft {
    name: String,
    typescript: bool,
    sfc: Sfc,
    canary: Span,
    expected: Vec<Expected>,
    hovers: Vec<HoverExpectation>,
    guarded_callbacks: usize,
    conditions: usize,
}

impl Draft {
    /// Start a fixture: `script` writes the script body after the imports.
    fn new(name: String, typescript: bool, script: impl FnOnce(&mut Sfc)) -> Self {
        let mut sfc = if typescript {
            Sfc::typescript()
        } else {
            Sfc::javascript()
        };
        script(&mut sfc);
        let canary = sfc.template(typescript);
        Self {
            name,
            typescript,
            sfc,
            canary,
            expected: Vec::new(),
            hovers: Vec::new(),
            guarded_callbacks: 0,
            conditions: 0,
        }
    }

    fn expect(&mut self, code: u32, authored: Span) {
        self.expected.push(Expected { code, authored });
    }

    /// Append `text`, expecting `code` on it when `when`.
    fn mark_if(&mut self, when: bool, code: u32, text: &str) {
        let span = self.sfc.mark(text);
        if when {
            self.expect(code, span);
        }
    }

    fn finish(mut self) -> SfcFixture {
        self.expected.push(Expected {
            code: TS2322,
            authored: self.canary,
        });
        self.expected.sort();
        SfcFixture {
            name: self.name,
            typescript: self.typescript,
            source: self.sfc.finish(),
            expected: self.expected,
            hovers: self.hovers,
            guarded_callbacks: self.guarded_callbacks,
            conditions: self.conditions,
            canary: self.canary,
        }
    }
}

const TAKE: &str = "function take(value: string): void {}\n";

/// `v-if` / `v-else-if` / `v-else` over a three-member union, one callback per
/// branch reading that branch's member, plus an unguarded callback that must
/// fail. With `misplaced`, the middle callback reads the last branch's member.
pub fn three_branch_discriminator(root: SfcRoot, misplaced: bool) -> SfcFixture {
    let r = root.expr();
    let mut d = Draft::new(
        format!(
            "ThreeBranch{}{}",
            pascal(root.label()),
            if misplaced { "Misplaced" } else { "" }
        ),
        true,
        |s| {
            s.push("type U3 = { kind: 'a'; a: string } | { kind: 'b'; b: string } | { kind: 'c'; c: string };\n");
            s.push(TAKE);
            root.declare("U3", s);
        },
    );
    d.sfc.push(&format!(
        "  <button v-if=\"{r}.kind === 'a'\" @click=\"() => take({r}.a)\"></button>\n"
    ));
    d.sfc.push(&format!(
        "  <button v-else-if=\"{r}.kind === 'b'\" @click=\"() => take({r}."
    ));
    d.mark_if(misplaced, TS2339, if misplaced { "c" } else { "b" });
    d.sfc.push(")\"></button>\n");
    d.sfc.push(&format!(
        "  <button v-else @click=\"() => take({r}.c)\"></button>\n"
    ));
    d.sfc.push(&format!("  <button @click=\"() => take({r}."));
    d.mark_if(true, TS2339, "a");
    d.sfc.push(")\"></button>\n");
    d.guarded_callbacks = 3;
    d.conditions = 2;
    d.finish()
}

/// A flat `n`-branch discriminant chain whose `v-else` is valid only after
/// every predecessor negation (the essential one `n - 1` terms back). Each
/// branch has a callback and an interpolation reading its own member. With
/// `drop_first`, the opening branch is removed, so the `v-else` reads fail.
pub fn flat_chain(root: SfcRoot, n: usize, drop_first: bool) -> SfcFixture {
    assert!(n >= 3);
    let r = root.expr();
    let mut d = Draft::new(
        format!(
            "FlatChain{n}{}{}",
            pascal(root.label()),
            if drop_first { "WithoutFirst" } else { "" }
        ),
        true,
        |s| {
            let members: Vec<String> = (0..n)
                .map(|i| format!("{{ kind: 'k{i}'; f{i}: string }}"))
                .collect();
            s.push(&format!("type UN = {};\n", members.join(" | ")));
            s.push(TAKE);
            root.declare("UN", s);
        },
    );
    let first = usize::from(drop_first);
    for i in first..n - 1 {
        let directive = if i == first { "v-if" } else { "v-else-if" };
        d.sfc.push(&format!(
            "  <p {directive}=\"{r}.kind === 'k{i}'\" @click=\"() => take({r}.f{i})\">{{{{ {r}.f{i} }}}}</p>\n"
        ));
        d.guarded_callbacks += 1;
        d.conditions += 1;
    }
    let last = n - 1;
    d.sfc.push(&format!("  <p v-else @click=\"() => take({r}."));
    d.mark_if(drop_first, TS2339, &format!("f{last}"));
    d.sfc.push(&format!(")\">{{{{ {r}."));
    d.mark_if(drop_first, TS2339, &format!("f{last}"));
    d.sfc.push(" }}</p>\n");
    d.guarded_callbacks += 1;
    d.finish()
}

/// `depth` nested `v-if` levels on optional `reactive` members `a0..`, a callback at every
/// level reading the outermost positive, and innermost a `v-if`/`v-else`
/// discriminant chain whose callbacks also read it. With `drop_outer`, the
/// outermost level loses its condition, so every read of `a0` fails.
pub fn deep_positives(depth: usize, drop_outer: bool) -> SfcFixture {
    let mut d = Draft::new(
        format!(
            "DeepPositives{depth}{}",
            if drop_outer { "WithoutOuter" } else { "" }
        ),
        true,
        |s| {
            s.push("function take(outer: string, value: string): void {}\n");
            s.push("type Q = { kind: 'x'; x: string } | { kind: 'y'; y: string };\n");
            s.push("const state = reactive({\n");
            for i in 0..depth {
                s.push(&format!("  a{i}: undefined as string | undefined,\n"));
            }
            s.push("  q: {} as Q,\n});\n");
        },
    );
    let mut a0_reads = Vec::new();
    for i in 0..depth {
        if i == 0 && drop_outer {
            d.sfc.push("<div>");
        } else {
            d.sfc.push(&format!("<div v-if=\"state.a{i}\">"));
            d.conditions += 1;
        }
        // TypeScript reports the first failing argument of a call, so the
        // outer read comes first.
        d.sfc.push("<i @click=\"() => take(");
        a0_reads.push(d.sfc.mark("state.a0"));
        d.sfc.push(&format!(", state.a{i})\"></i>\n"));
        if i > 0 || !drop_outer {
            d.guarded_callbacks += 1;
        }
    }
    d.sfc
        .push("<b v-if=\"state.q.kind === 'x'\" @click=\"() => take(");
    a0_reads.push(d.sfc.mark("state.a0"));
    d.sfc
        .push(", state.q.x)\"></b>\n<b v-else @click=\"() => take(");
    a0_reads.push(d.sfc.mark("state.a0"));
    d.sfc.push(", state.q.y)\"></b>\n");
    d.guarded_callbacks += 2;
    d.conditions += 1;
    for _ in 0..depth {
        d.sfc.push("</div>");
    }
    d.sfc.push("\n");
    if drop_outer {
        for read in a0_reads {
            d.expect(TS2345, read);
        }
    }
    d.finish()
}

const PICK: &str = "const Pick = defineComponent({\n\
  props: {\n\
    onPick: { type: Function as PropType<(id: number) => string>, required: true },\n\
    onLoad: { type: Function as PropType<(key: string) => Promise<number>>, required: true },\n\
    onMap: { type: Function as PropType<<T>(value: T) => T>, required: true },\n\
    onCount: { type: Function as PropType<(n: number) => void>, required: true },\n\
  },\n\
});\n";

/// Contextual callback typing under narrowing: an event parameter, a value
/// return contract, an async contract, a generic contract, a function
/// expression and a generated handler. With `broken`, each contract is
/// violated once.
pub fn contextual(root: SfcRoot, broken: bool) -> SfcFixture {
    let r = root.expr();
    let mut d = Draft::new(
        format!(
            "Contextual{}{}",
            pascal(root.label()),
            if broken { "Broken" } else { "" }
        ),
        true,
        |s| {
            s.push("type UC = { kind: 'a'; a: string } | { kind: 'b'; b: number };\n");
            s.push(TAKE);
            s.push("function takeNumber(value: number): void {}\n");
            s.push("function onMouse(event: MouseEvent, value: string): void {}\n");
            s.push("function onText(event: string, value: string): void {}\n");
            s.push("async function wait(id: string): Promise<void> {}\n");
            s.push(PICK);
            root.declare("UC", s);
        },
    );
    d.sfc.push(&format!("  <div v-if=\"{r}.kind === 'a'\">\n"));
    d.conditions = 1;

    // Event parameter.
    d.sfc.push("    <button @click=\"(");
    let event = d.sfc.mark("e");
    d.hovers.push(HoverExpectation {
        authored: event,
        contains: "PointerEvent",
    });
    d.sfc.push(&format!(
        ") => {}(",
        if broken { "onText" } else { "onMouse" }
    ));
    d.mark_if(broken, TS2345, "e");
    d.sfc.push(&format!(", {r}.a)\"></button>\n"));

    d.sfc.push("    <Pick\n");
    // Value return contract.
    d.sfc.push("      :onPick=\"(");
    let id = d.sfc.mark("id");
    d.hovers.push(HoverExpectation {
        authored: id,
        contains: "number",
    });
    d.sfc.push(") => ");
    if broken {
        d.mark_if(true, TS2322, "id");
    } else {
        d.sfc.push(&format!("{r}.a + id"));
    }
    d.sfc.push("\"\n");

    // Async contract. A contract violated by the inferred return type of an
    // async or generic callback is reported on the attribute name.
    d.sfc.push("      :");
    d.mark_if(broken, TS2322, "onLoad");
    d.sfc.push("=\"async (");
    let key = d.sfc.mark("key");
    d.hovers.push(HoverExpectation {
        authored: key,
        contains: "string",
    });
    d.sfc.push(") => { await wait(key); return ");
    d.sfc.push(&if broken {
        "key".to_string()
    } else {
        format!("{r}.a.length")
    });
    d.sfc.push(" }\"\n");

    // Generic contract.
    d.sfc.push("      :");
    d.mark_if(broken, TS2322, "onMap");
    d.sfc.push("=\"(");
    let value = d.sfc.mark("value");
    d.hovers.push(HoverExpectation {
        authored: value,
        contains: "T",
    });
    d.sfc.push(&format!(") => {{ take({r}.a); return "));
    d.sfc.push(&if broken {
        format!("{r}.a")
    } else {
        "value".to_string()
    });
    d.sfc.push(" }\"\n");

    // Function expression (the narrowed read goes through a local, so its
    // diagnostic lands on an authored identifier for every root).
    d.sfc.push(&format!(
        "      :onCount=\"function (n) {{ const local = {r}.a; take(local); takeNumber("
    ));
    d.mark_if(broken, TS2345, if broken { "local" } else { "n" });
    d.sfc.push(") }\"\n    />\n");

    // Generated handler around an inline statement.
    d.sfc.push(if broken {
        "    <button @click=\"onText("
    } else {
        "    <button @click=\"onMouse("
    });
    d.mark_if(broken, TS2345, "$event");
    d.sfc.push(&format!(", {r}.a)\"></button>\n"));
    d.sfc.push("  </div>\n");
    d.guarded_callbacks = 6;
    d.finish()
}

const ROW: &str = "const Row = defineComponent({\n\
  slots: Object as SlotsType<{ default: { row: { note?: string } } }>,\n\
});\n\
const Count = defineComponent({\n\
  props: { onCount: { type: Function as PropType<(value: number) => void>, required: true } },\n\
});\n";

/// Lexical identity: `v-for` aliases (one shadowing a setup binding), a
/// destructured alias, slot parameters, a captured callback local and a
/// parameter shadowing the narrowed root — each read under narrowing.
pub fn lexical(root: SfcRoot) -> SfcFixture {
    let r = root.expr();
    let shadow = root.shadow();
    let mut d = Draft::new(format!("Lexical{}", pascal(root.label())), true, |s| {
        s.push("type UL = { kind: 'a'; a: string } | { kind: 'b'; b: number };\n");
        s.push(TAKE);
        s.push("function takeNumber(value: number): void {}\n");
        s.push("const items = [] as { label?: string }[];\n");
        s.push("const item = 0;\n");
        s.push(ROW);
        root.declare("UL", s);
    });
    d.sfc.push(&format!("  <div v-if=\"{r}.kind === 'a'\">\n"));
    d.sfc.push(&format!(
        "    <p v-for=\"item in items\"><i v-if=\"item.label\" @click=\"() => take(item.label + {r}.a)\"></i></p>\n"
    ));
    d.sfc.push(&format!(
        "    <p v-for=\"({{ label }}, index) in items\"><i v-if=\"label\" @click=\"() => take(label + index + {r}.a)\"></i></p>\n"
    ));
    d.sfc.push(&format!(
        "    <Row v-slot=\"{{ row }}\"><i v-if=\"row.note\" @click=\"() => take(row.note + {r}.a)\"></i></Row>\n"
    ));
    d.sfc.push(&format!(
        "    <i @click=\"() => {{ const local = {r}.a; take(local) }}\"></i>\n"
    ));
    d.sfc.push(&format!(
        "    <Count :on-count=\"({shadow}) => takeNumber({shadow})\" />\n"
    ));
    d.sfc.push("  </div>\n");
    d.guarded_callbacks = 5;
    d.conditions = 4;
    d.finish()
}

/// A mutation of the narrowed `reactive` member and a nested authored
/// closure: both lose the narrowing, exactly as TypeScript decides for the
/// same code in one function.
pub fn mutation_and_nested_closure() -> SfcFixture {
    let mut d = Draft::new("MutationAndNestedClosure".into(), true, |s| {
        s.push(TAKE);
        s.push("const state = reactive({ note: undefined as string | undefined });\n");
    });
    d.sfc.push("  <div v-if=\"state.note\">\n");
    d.sfc
        .push("    <i @click=\"() => { state.note = undefined; take(");
    d.mark_if(true, TS2345, "state.note");
    d.sfc.push(") }\"></i>\n");
    d.sfc
        .push("    <i @click=\"() => { [1].forEach(() => take(");
    d.mark_if(true, TS2345, "state.note");
    d.sfc.push(")) }\"></i>\n");
    d.sfc
        .push("    <i @click=\"() => take(state.note)\"></i>\n");
    d.sfc.push("  </div>\n");
    d.guarded_callbacks = 3;
    d.conditions = 1;
    d.finish()
}

/// Authored identifiers after non-ASCII text on the same line, and a `v-if`
/// on a `v-for` element (a lifted chain whose callback sits in the frame).
pub fn mapping() -> SfcFixture {
    let mut d = Draft::new("Mapping".into(), true, |s| {
        s.push("type UM = { kind: 'a'; a: string; label: string } | { kind: 'b'; b: number };\n");
        s.push("function take(value: string, other: string): void {}\n");
        s.push("function takeItem(item: number, other: string): void {}\n");
        s.push("const items = [] as number[];\n");
        s.push("defineProps<{ u: UM }>();\n");
    });
    d.sfc.push(
        "  <p v-if=\"u.kind === 'a' && u.label !== 'ñandú 🦀'\" @click=\"() => take('ñ🦀', u.",
    );
    d.mark_if(true, TS2339, "b");
    d.sfc.push(")\">ü {{ u.a }} 🦀 {{ u.");
    d.mark_if(true, TS2339, "b");
    d.sfc.push(" }}</p>\n");
    d.sfc.push(
        "  <li v-for=\"item in items\" v-if=\"u.kind === 'a'\" @click=\"() => takeItem(item, u.",
    );
    d.mark_if(true, TS2339, "zz");
    d.sfc.push(")\"></li>\n");
    d.guarded_callbacks = 2;
    d.conditions = 2;
    d.finish()
}

/// A lifted chain (its members carry `v-for`): a frame branch, a plain branch
/// that must be wrapped to hold its snapshots, and a `v-else`. With
/// `misplaced`, the `v-else` callback reads the opening branch's member.
pub fn lifted_chain(root: SfcRoot, misplaced: bool) -> SfcFixture {
    let r = root.expr();
    let mut d = Draft::new(
        format!(
            "LiftedChain{}{}",
            pascal(root.label()),
            if misplaced { "Misplaced" } else { "" }
        ),
        true,
        |s| {
            s.push("type U3 = { kind: 'a'; a: string } | { kind: 'b'; b: string } | { kind: 'c'; c: string };\n");
            s.push(TAKE);
            s.push("const nums = [] as number[];\n");
            root.declare("U3", s);
        },
    );
    d.sfc.push(&format!(
        "  <li v-if=\"{r}.kind === 'a'\" v-for=\"n in nums\" @click=\"() => take({r}.a + n)\"></li>\n"
    ));
    d.sfc.push(&format!(
        "  <p v-else-if=\"{r}.kind === 'b'\" @click=\"() => take({r}.b)\">{{{{ {r}.b }}}}</p>\n"
    ));
    d.sfc.push(&format!("  <p v-else @click=\"() => take({r}."));
    d.mark_if(misplaced, TS2339, if misplaced { "a" } else { "c" });
    d.sfc.push(")\"></p>\n");
    d.guarded_callbacks = 3;
    d.conditions = 2;
    d.finish()
}

/// A multi-statement inline handler under a condition; in the `v-else` one of
/// its statements reads the other branch's member.
pub fn multi_statement_handler(root: SfcRoot) -> SfcFixture {
    let r = root.expr();
    let mut d = Draft::new(
        format!("MultiStatement{}", pascal(root.label())),
        true,
        |s| {
            s.push("type U2 = { kind: 'a'; a: string } | { kind: 'b'; b: string };\n");
            s.push(TAKE);
            root.declare("U2", s);
        },
    );
    d.sfc.push(&format!(
        "  <button v-if=\"{r}.kind === 'a'\" @click=\"take({r}.a); take({r}.a)\"></button>\n"
    ));
    d.sfc
        .push(&format!("  <button v-else @click=\"take({r}.b); take({r}."));
    d.mark_if(true, TS2339, "a");
    d.sfc.push(")\"></button>\n");
    d.guarded_callbacks = 2;
    d.conditions = 1;
    d.finish()
}

/// A JavaScript SFC: the same re-narrowing in a checked JavaScript carrier.
pub fn javascript() -> SfcFixture {
    let mut d = Draft::new("JavaScriptSetup".into(), false, |s| {
        s.push("/** @param {string} value */\nfunction take(value) {}\n");
        s.push("const state = reactive({\n  u: /** @type {{ kind: 'a', a: string } | { kind: 'b', b: string }} */ ({ kind: 'a', a: '' }),\n});\n");
    });
    d.sfc.push(
        "  <button v-if=\"state.u.kind === 'a'\" @click=\"() => take(state.u.a)\"></button>\n",
    );
    d.sfc
        .push("  <button v-else @click=\"() => { take(state.u.b); take(state.u.");
    d.mark_if(true, TS2339, "a");
    d.sfc.push(") }\"></button>\n");
    d.guarded_callbacks = 2;
    d.conditions = 1;
    d.finish()
}

/// Every semantic fixture of the exactness contract, in a fixed order.
pub fn semantic_suite() -> Vec<SfcFixture> {
    let mut fixtures = Vec::new();
    for root in SfcRoot::ALL {
        fixtures.push(three_branch_discriminator(root, false));
        fixtures.push(three_branch_discriminator(root, true));
        fixtures.push(flat_chain(root, 40, false));
        fixtures.push(flat_chain(root, 40, true));
        fixtures.push(contextual(root, false));
        fixtures.push(contextual(root, true));
        fixtures.push(lexical(root));
        fixtures.push(lifted_chain(root, false));
        fixtures.push(lifted_chain(root, true));
        fixtures.push(multi_statement_handler(root));
    }
    fixtures.push(deep_positives(40, false));
    fixtures.push(deep_positives(40, true));
    fixtures.push(mutation_and_nested_closure());
    fixtures.push(mapping());
    fixtures.push(javascript());
    fixtures
}

/// The linear-representation matrix: an `n`-branch chain on optional
/// `reactive` members `c0..` with one callback per branch, the `v-else` reading the negation of
/// the opening condition. With `broken`, every callback reads a reference its
/// branch does not narrow, so each must report exactly once.
pub fn flat_matrix(n: usize, broken: bool) -> SfcFixture {
    let mut d = Draft::new(
        format!("FlatMatrix{n}{}", if broken { "Broken" } else { "" }),
        true,
        |s| {
            s.push(TAKE);
            s.push("function none(value: undefined): void {}\n");
            s.push("const state = reactive({\n");
            for i in 0..n {
                s.push(&format!("  c{i}: undefined as string | undefined,\n"));
            }
            s.push("});\n");
        },
    );
    for i in 0..n {
        let opening = match i {
            0 => format!("<b v-if=\"state.c{i} !== undefined\">"),
            _ if i == n - 1 => "<b v-else>".to_string(),
            _ => format!("<b v-else-if=\"state.c{i} !== undefined\">"),
        };
        if i < n - 1 {
            d.conditions += 1;
        }
        d.sfc.push(&opening);
        if i == n - 1 {
            d.sfc.push(if broken {
                "<i @click=\"() => take("
            } else {
                "<i @click=\"() => none("
            });
            d.mark_if(broken, TS2345, "state.c0");
        } else {
            d.sfc.push("<i @click=\"() => take(");
            d.mark_if(
                broken,
                TS2345,
                &format!("state.c{}", if broken { i + 1 } else { i }),
            );
        }
        d.sfc.push(")\"></i></b>\n");
        d.guarded_callbacks += 1;
    }
    d.finish()
}

/// The nested-scope matrix: `n` branches as `n / 4` nested levels under one
/// outermost `v-if` on the `reactive` member `top`. Level `i` is a four-branch chain on `s{i}` whose
/// opening branch holds level `i + 1` (every fourth level inside a `v-for`
/// frame); every branch has a callback reading the outermost positive and its
/// own narrowed member. With `broken`, each callback reads the next level's
/// (not yet narrowed) member instead.
pub fn nested_matrix(n: usize, broken: bool) -> SfcFixture {
    assert!(n.is_multiple_of(4) && n >= 8);
    let levels = n / 4;
    let mut d = Draft::new(
        format!("NestedMatrix{n}{}", if broken { "Broken" } else { "" }),
        true,
        |s| {
            s.push("type S = 'a' | 'b' | 'c' | 'd';\n");
            s.push("function takeA(top: 'on', own: 'a', row: number): void {}\n");
            s.push("function takeB(top: 'on', own: 'b'): void {}\n");
            s.push("function takeC(top: 'on', own: 'c'): void {}\n");
            s.push("function takeD(top: 'on', own: 'd'): void {}\n");
            s.push("const rows = [] as number[];\n");
            s.push("const state = reactive({\n  top: 'on' as 'on' | 'off',\n");
            for i in 0..=levels {
                s.push(&format!("  s{i}: 'a' as S,\n"));
            }
            s.push("});\n");
        },
    );
    let read = |d: &mut Draft, i: usize| {
        d.mark_if(
            broken,
            TS2345,
            &format!("state.s{}", if broken { i + 1 } else { i }),
        );
        d.guarded_callbacks += 1;
    };
    d.sfc.push("<main v-if=\"state.top === 'on'\">\n");
    d.conditions += 1;
    let mut row = String::from("0");
    for i in 0..levels {
        d.sfc.push(&format!("<div v-if=\"state.s{i} === 'a'\">"));
        d.conditions += 1;
        if i % 4 == 3 {
            d.sfc.push(&format!("<template v-for=\"r{i} in rows\">"));
            row = format!("r{i}");
        }
        d.sfc.push("<i @click=\"() => takeA(state.top, ");
        read(&mut d, i);
        d.sfc.push(&format!(", {row})\"></i>\n"));
    }
    for i in (0..levels).rev() {
        if i % 4 == 3 {
            d.sfc.push("</template>");
        }
        d.sfc.push("</div>\n");
        for (directive, take, condition) in [
            (format!("v-else-if=\"state.s{i} === 'b'\""), "takeB", true),
            (format!("v-else-if=\"state.s{i} === 'c'\""), "takeC", true),
            ("v-else".to_string(), "takeD", false),
        ] {
            if condition {
                d.conditions += 1;
            }
            d.sfc.push(&format!(
                "<div {directive}><i @click=\"() => {take}(state.top, "
            ));
            read(&mut d, i);
            d.sfc.push(")\"></i></div>\n");
        }
    }
    d.sfc.push("</main>\n");
    d.finish()
}

fn pascal(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}
