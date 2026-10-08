//! Source-backed fixtures shared by the compiler contract tests and the
//! paired real-provider tests.
//!
//! Each fixture is an authored SFC template plus the declarations its
//! bindings are typed by, and the exact diagnostics (code and authored span) a
//! TypeScript provider must report for its generated check. Spans are recorded
//! while the source is written, never searched for afterwards.

use super::builder::{build_plan, BuildError, BuildWork, PlanContext};
use super::generator::{generate_with, GuardStrategy};
use super::seam::{CheckPlan, GeneratedCheck, Span};
use crate::template::code_gen::binding::BindingType;

/// One diagnostic a provider must report, by TypeScript code and the
/// authored span its range maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Expected {
    pub code: u32,
    pub authored: Span,
}

/// Hover expectation: hovering the authored identifier at `authored` shows a
/// type text containing `contains`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverExpectation {
    pub authored: Span,
    pub contains: &'static str,
}

#[derive(Debug, Clone)]
pub struct Fixture {
    pub name: String,
    pub source: String,
    declarations: String,
    bindings: Vec<(String, BindingType)>,
    contracts: Vec<(String, String)>,
    /// Exactly the diagnostics a provider must report, sorted.
    pub expected: Vec<Expected>,
    pub hovers: Vec<HoverExpectation>,
    /// Callbacks the template authors (every one must be checked).
    pub authored_callbacks: usize,
}

impl Fixture {
    pub fn plan(&self) -> Result<(CheckPlan, BuildWork), BuildError> {
        let bindings: Vec<(&str, BindingType)> = self
            .bindings
            .iter()
            .map(|(name, kind)| (name.as_str(), *kind))
            .collect();
        let contracts: Vec<(&str, &str)> = self
            .contracts
            .iter()
            .map(|(key, contract)| (key.as_str(), contract.as_str()))
            .collect();
        build_plan(
            &self.source,
            &PlanContext {
                declarations: &self.declarations,
                parameters: "()",
                bindings: &bindings,
                contracts: &contracts,
            },
        )
    }

    pub fn generate(&self) -> GeneratedCheck {
        self.generate_with(GuardStrategy::Snapshot)
    }

    pub fn generate_with(&self, strategy: GuardStrategy) -> GeneratedCheck {
        let (plan, _) = self
            .plan()
            .unwrap_or_else(|error| panic!("{}: plan refused: {error:?}", self.name));
        generate_with(&plan, &self.source, strategy)
            .unwrap_or_else(|error| panic!("{}: generation refused: {error:?}", self.name))
    }
}

/// How a narrowed reference is rooted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    /// A prop: `__props.u`, a mutable property access.
    MutableProperty,
    /// A `<script setup>` const: `u`, a constant identifier.
    ConstantReference,
    /// A readonly property of a const: `cfg.u`.
    ReadonlyProperty,
}

impl Root {
    pub const ALL: [Root; 3] = [
        Root::MutableProperty,
        Root::ConstantReference,
        Root::ReadonlyProperty,
    ];

    fn label(self) -> &'static str {
        match self {
            Root::MutableProperty => "mutable-property",
            Root::ConstantReference => "constant-reference",
            Root::ReadonlyProperty => "readonly-property",
        }
    }

    /// The template expression naming the root.
    fn expr(self) -> &'static str {
        match self {
            Root::ReadonlyProperty => "cfg.u",
            _ => "u",
        }
    }

    fn declare(self, ty: &str, decls: &mut String, bindings: &mut Vec<(String, BindingType)>) {
        match self {
            Root::MutableProperty => {
                decls.push_str(&format!("declare const __props: {{ u: {ty} }};\n"));
                bindings.push(("u".into(), BindingType::Props));
            }
            Root::ConstantReference => {
                decls.push_str(&format!("declare const u: {ty};\n"));
                bindings.push(("u".into(), BindingType::SetupConst));
            }
            Root::ReadonlyProperty => {
                decls.push_str(&format!("declare const cfg: {{ readonly u: {ty} }};\n"));
                bindings.push(("cfg".into(), BindingType::SetupConst));
            }
        }
    }
}

/// Writes a template while recording authored spans.
#[derive(Default)]
struct Src {
    text: String,
}

impl Src {
    fn new() -> Self {
        Self {
            text: "<template>\n".into(),
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

    fn finish(mut self) -> String {
        self.text.push_str("</template>\n");
        self.text
    }
}

const TS2322: u32 = 2322;
const TS2339: u32 = 2339;
const TS2345: u32 = 2345;

fn setup(names: &[&str]) -> Vec<(String, BindingType)> {
    names
        .iter()
        .map(|name| ((*name).to_string(), BindingType::SetupConst))
        .collect()
}

fn contracts(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, contract)| ((*key).to_string(), (*contract).to_string()))
        .collect()
}

#[allow(clippy::too_many_arguments)] // one fixture's fields, assembled in one place
fn finish(
    name: String,
    src: Src,
    declarations: String,
    bindings: Vec<(String, BindingType)>,
    contracts: Vec<(String, String)>,
    mut expected: Vec<Expected>,
    hovers: Vec<HoverExpectation>,
    authored_callbacks: usize,
) -> Fixture {
    expected.sort();
    Fixture {
        name,
        source: src.finish(),
        declarations,
        bindings,
        contracts,
        expected,
        hovers,
        authored_callbacks,
    }
}

/// `v-if` / `v-else-if` / `v-else` over a three-member union, one callback per
/// branch reading that branch's member, plus an unguarded callback that must
/// fail. With `misplaced`, the middle callback reads the last branch's member.
pub fn three_branch_discriminator(root: Root, misplaced: bool) -> Fixture {
    let r = root.expr();
    let mut decls = String::from(
        "type U3 = { kind: 'a'; a: string } | { kind: 'b'; b: string } | { kind: 'c'; c: string };\n\
         declare function take(value: string): void;\n",
    );
    let mut bindings = setup(&["take"]);
    root.declare("U3", &mut decls, &mut bindings);
    let mut expected = Vec::new();
    let mut src = Src::new();
    src.push(&format!(
        "  <button v-if=\"{r}.kind === 'a'\" @click=\"() => take({r}.a)\"></button>\n"
    ));
    src.push(&format!(
        "  <button v-else-if=\"{r}.kind === 'b'\" @click=\"() => take({r}."
    ));
    let member = src.mark(if misplaced { "c" } else { "b" });
    src.push(")\"></button>\n");
    if misplaced {
        expected.push(Expected {
            code: TS2339,
            authored: member,
        });
    }
    src.push(&format!(
        "  <button v-else @click=\"() => take({r}.c)\"></button>\n"
    ));
    src.push(&format!("  <button @click=\"() => take({r}."));
    expected.push(Expected {
        code: TS2339,
        authored: src.mark("a"),
    });
    src.push(")\"></button>\n");
    finish(
        format!(
            "three-branch-{}{}",
            root.label(),
            if misplaced { "-misplaced" } else { "" }
        ),
        src,
        decls,
        bindings,
        contracts(&[("@click", "(event: MouseEvent) => void")]),
        expected,
        Vec::new(),
        4,
    )
}

/// A flat `n`-branch discriminant chain whose `v-else` is valid only after
/// every predecessor negation (the essential one `n - 1` terms back). Each
/// branch has a callback and an interpolation reading its own member. With
/// `drop_first`, the opening branch (`k0`) is removed, so the `v-else` reads
/// fail.
pub fn flat_chain(root: Root, n: usize, drop_first: bool) -> Fixture {
    assert!(n >= 3);
    let r = root.expr();
    let members: Vec<String> = (0..n)
        .map(|i| format!("{{ kind: 'k{i}'; f{i}: string }}"))
        .collect();
    let mut decls = format!(
        "type UN = {};\ndeclare function take(value: string): void;\n",
        members.join(" | ")
    );
    let mut bindings = setup(&["take"]);
    root.declare("UN", &mut decls, &mut bindings);
    let mut expected = Vec::new();
    let mut src = Src::new();
    let first = usize::from(drop_first);
    let mut callbacks = 0;
    for i in first..n - 1 {
        let directive = if i == first { "v-if" } else { "v-else-if" };
        src.push(&format!(
            "  <p {directive}=\"{r}.kind === 'k{i}'\" @click=\"() => take({r}.f{i})\">{{{{ {r}.f{i} }}}}</p>\n"
        ));
        callbacks += 1;
    }
    let last = n - 1;
    src.push(&format!("  <p v-else @click=\"() => take({r}."));
    let in_callback = src.mark(&format!("f{last}"));
    src.push(&format!(")\">{{{{ {r}."));
    let in_interpolation = src.mark(&format!("f{last}"));
    src.push(" }}</p>\n");
    callbacks += 1;
    if drop_first {
        expected.push(Expected {
            code: TS2339,
            authored: in_callback,
        });
        expected.push(Expected {
            code: TS2339,
            authored: in_interpolation,
        });
    }
    finish(
        format!(
            "flat-chain-{n}-{}{}",
            root.label(),
            if drop_first { "-without-first" } else { "" }
        ),
        src,
        decls,
        bindings,
        contracts(&[("@click", "(event: MouseEvent) => void")]),
        expected,
        Vec::new(),
        callbacks,
    )
}

/// `depth` nested `v-if` levels on optional props `a0..`, a callback at every
/// level reading the outermost positive, and innermost a `v-if`/`v-else`
/// discriminant chain whose callbacks also read it. With `drop_outer`, the
/// outermost level loses its condition, so every read of `a0` fails.
pub fn deep_positives(depth: usize, drop_outer: bool) -> Fixture {
    let mut decls = String::from(
        "declare function take(outer: string, value: string): void;\n\
         type Q = { kind: 'x'; x: string } | { kind: 'y'; y: string };\n\
         declare const __props: {\n",
    );
    let mut bindings = setup(&["take"]);
    for i in 0..depth {
        decls.push_str(&format!("  a{i}?: string;\n"));
        bindings.push((format!("a{i}"), BindingType::Props));
    }
    decls.push_str("  q: Q;\n};\n");
    bindings.push(("q".into(), BindingType::Props));
    let mut expected = Vec::new();
    let mut src = Src::new();
    let mut a0_reads = Vec::new();
    let mut callbacks = 0;
    for i in 0..depth {
        if i == 0 && drop_outer {
            src.push("<div>");
        } else {
            src.push(&format!("<div v-if=\"a{i}\">"));
        }
        // TypeScript reports the first failing argument of a call, so the
        // outer read comes first.
        src.push("<i @click=\"() => take(");
        a0_reads.push(src.mark("a0"));
        src.push(&format!(", a{i})\"></i>\n"));
        callbacks += 1;
    }
    src.push("<b v-if=\"q.kind === 'x'\" @click=\"() => take(");
    a0_reads.push(src.mark("a0"));
    src.push(", q.x)\"></b>\n<b v-else @click=\"() => take(");
    a0_reads.push(src.mark("a0"));
    src.push(", q.y)\"></b>\n");
    callbacks += 2;
    for _ in 0..depth {
        src.push("</div>");
    }
    src.push("\n");
    if drop_outer {
        expected.extend(a0_reads.into_iter().map(|authored| Expected {
            code: TS2345,
            authored,
        }));
    }
    finish(
        format!(
            "deep-positives-{depth}{}",
            if drop_outer { "-without-outer" } else { "" }
        ),
        src,
        decls,
        bindings,
        contracts(&[("@click", "(event: MouseEvent) => void")]),
        expected,
        Vec::new(),
        callbacks,
    )
}

/// Contextual callback typing under narrowing: event parameters, a value
/// return contract, an async contract, a generic contract, a function
/// expression and a generated handler. With `broken`, each contract is
/// violated once.
pub fn contextual(root: Root, broken: bool) -> Fixture {
    let r = root.expr();
    let mut decls = String::from(
        "type UC = { kind: 'a'; a: string } | { kind: 'b'; b: number };\n\
         declare function take(value: string): void;\n\
         declare function takeNumber(value: number): void;\n\
         declare function onMouse(event: MouseEvent, value: string): void;\n\
         declare function onText(event: string, value: string): void;\n\
         declare function wait(id: string): Promise<void>;\n",
    );
    let mut bindings = setup(&["take", "takeNumber", "onMouse", "onText", "wait"]);
    root.declare("UC", &mut decls, &mut bindings);
    let mut expected = Vec::new();
    let mut hovers = Vec::new();
    let mut src = Src::new();
    src.push(&format!("  <div v-if=\"{r}.kind === 'a'\">\n"));

    // Event parameter.
    src.push("    <button @click=\"(");
    let event = src.mark("e");
    hovers.push(HoverExpectation {
        authored: event,
        contains: "MouseEvent",
    });
    src.push(&format!(
        ") => {}(",
        if broken { "onText" } else { "onMouse" }
    ));
    let event_use = src.mark("e");
    src.push(&format!(", {r}.a)\"></button>\n"));
    if broken {
        expected.push(Expected {
            code: TS2345,
            authored: event_use,
        });
    }

    // Value return contract.
    src.push("    <span :on-pick=\"(");
    let id = src.mark("id");
    hovers.push(HoverExpectation {
        authored: id,
        contains: "number",
    });
    src.push(") => ");
    if broken {
        expected.push(Expected {
            code: TS2322,
            authored: src.mark("id"),
        });
    } else {
        src.push(&format!("{r}.a + id"));
    }
    src.push("\"></span>\n");

    // Async contract. A contract violated by the inferred return type of an
    // async or generic callback is reported on the whole checked callback.
    src.push("    <span :on-load=\"");
    let async_start = src.text.len() as u32;
    src.push("async (");
    let key = src.mark("key");
    hovers.push(HoverExpectation {
        authored: key,
        contains: "string",
    });
    src.push(") => { await wait(key); return ");
    src.push(&if broken {
        "key".to_string()
    } else {
        format!("{r}.a.length")
    });
    src.push(" }");
    let async_function = Span::new(async_start, src.text.len() as u32);
    src.push("\"></span>\n");
    if broken {
        expected.push(Expected {
            code: TS2345,
            authored: async_function,
        });
    }

    // Generic contract.
    src.push("    <span :on-map=\"");
    let generic_start = src.text.len() as u32;
    src.push("(");
    let value = src.mark("value");
    hovers.push(HoverExpectation {
        authored: value,
        contains: "T",
    });
    src.push(&format!(") => {{ take({r}.a); return "));
    src.push(&if broken {
        format!("{r}.a")
    } else {
        "value".to_string()
    });
    src.push(" }");
    let generic_function = Span::new(generic_start, src.text.len() as u32);
    src.push("\"></span>\n");
    if broken {
        expected.push(Expected {
            code: TS2345,
            authored: generic_function,
        });
    }

    // Function expression.
    src.push(&format!(
        "    <span :on-count=\"function (n) {{ take({r}.a); takeNumber("
    ));
    if broken {
        expected.push(Expected {
            code: TS2345,
            authored: src.mark(&format!("{r}.a")),
        });
    } else {
        src.push("n");
    }
    src.push(") }\"></span>\n");

    // Generated handler around an inline statement.
    src.push(if broken {
        "    <button @submit=\"onText("
    } else {
        "    <button @submit=\"onMouse("
    });
    let handler_event = src.mark("$event");
    if broken {
        expected.push(Expected {
            code: TS2345,
            authored: handler_event,
        });
    }
    src.push(&format!(", {r}.a)\"></button>\n"));
    src.push("  </div>\n");
    finish(
        format!(
            "contextual-{}{}",
            root.label(),
            if broken { "-broken" } else { "" }
        ),
        src,
        decls,
        bindings,
        contracts(&[
            ("@click", "(event: MouseEvent) => void"),
            ("@submit", "(event: MouseEvent) => void"),
            (":on-pick", "(id: number) => string"),
            (":on-load", "(key: string) => Promise<number>"),
            (":on-map", "<T>(value: T) => T"),
            (":on-count", "(n: number) => void"),
        ]),
        expected,
        hovers,
        6,
    )
}

/// Lexical identity: `v-for` aliases, a destructured alias, slot parameters,
/// a captured callback local, a parameter shadowing the narrowed root and a
/// `v-for` alias shadowing a setup binding — each read under narrowing.
pub fn lexical(root: Root) -> Fixture {
    let r = root.expr();
    let mut decls = String::from(
        "type UL = { kind: 'a'; a: string } | { kind: 'b'; b: number };\n\
         declare function take(value: string): void;\n\
         declare function takeNumber(value: number): void;\n\
         declare const items: { label?: string }[];\n\
         declare const item: number;\n",
    );
    let mut bindings = setup(&["take", "takeNumber", "items", "item"]);
    root.declare("UL", &mut decls, &mut bindings);
    let shadow = match root {
        Root::ReadonlyProperty => "cfg",
        _ => "u",
    };
    let mut src = Src::new();
    src.push(&format!("  <div v-if=\"{r}.kind === 'a'\">\n"));
    src.push(&format!(
        "    <p v-for=\"item in items\"><i v-if=\"item.label\" @click=\"() => take(item.label + {r}.a)\"></i></p>\n"
    ));
    src.push(&format!(
        "    <p v-for=\"({{ label }}, index) in items\"><i v-if=\"label\" @click=\"() => take(label + index + {r}.a)\"></i></p>\n"
    ));
    src.push(&format!(
        "    <Row v-slot=\"{{ row }}\"><i v-if=\"row.note\" @click=\"() => take(row.note + {r}.a)\"></i></Row>\n"
    ));
    src.push(&format!(
        "    <i @click=\"() => {{ const local = {r}.a; take(local) }}\"></i>\n"
    ));
    src.push(&format!(
        "    <i :on-count=\"({shadow}) => takeNumber({shadow})\"></i>\n"
    ));
    src.push("  </div>\n");
    finish(
        format!("lexical-{}", root.label()),
        src,
        decls,
        bindings,
        contracts(&[
            ("@click", "(event: MouseEvent) => void"),
            (":on-count", "(value: number) => void"),
            ("v-slot", "(props: { row: { note?: string } }) => unknown"),
        ]),
        Vec::new(),
        Vec::new(),
        5,
    )
}

/// A mutation of the narrowed prop and a nested authored closure: both lose
/// the narrowing exactly as they do when the condition path is replayed.
pub fn mutation_and_nested_closure() -> Fixture {
    let decls = String::from(
        "declare function take(value: string): void;\n\
         declare const __props: { note?: string };\n",
    );
    let mut bindings = setup(&["take"]);
    bindings.push(("note".into(), BindingType::Props));
    let mut expected = Vec::new();
    let mut src = Src::new();
    src.push("  <div v-if=\"note\">\n");
    src.push("    <i @click=\"() => { note = undefined; take(");
    expected.push(Expected {
        code: TS2345,
        authored: src.mark("note"),
    });
    src.push(") }\"></i>\n");
    src.push("    <i @click=\"() => { [1].forEach(() => take(");
    expected.push(Expected {
        code: TS2345,
        authored: src.mark("note"),
    });
    src.push(")) }\"></i>\n");
    src.push("    <i @click=\"() => take(note)\"></i>\n");
    src.push("  </div>\n");
    finish(
        "mutation-and-nested-closure".into(),
        src,
        decls,
        bindings,
        contracts(&[("@click", "(event: MouseEvent) => void")]),
        expected,
        Vec::new(),
        3,
    )
}

/// Authored identifiers after non-ASCII text on the same line, and a `v-if`
/// written after `v-for` on one element (the condition is emitted first, so
/// it is moved ahead of the authored `v-for`).
pub fn mapping() -> Fixture {
    let decls = String::from(
        "type UM = { kind: 'a'; a: string; label: string } | { kind: 'b'; b: number };\n\
         declare function take(value: string, other: string): void;\n\
         declare function takeItem(item: number, other: string): void;\n\
         declare const items: number[];\n\
         declare const __props: { u: UM };\n",
    );
    let mut bindings = setup(&["take", "takeItem", "items"]);
    bindings.push(("u".into(), BindingType::Props));
    let mut expected = Vec::new();
    let mut src = Src::new();
    src.push(
        "  <p v-if=\"u.kind === 'a' && u.label !== 'ñandú 🦀'\" @click=\"() => take('ñ🦀', u.",
    );
    expected.push(Expected {
        code: TS2339,
        authored: src.mark("b"),
    });
    src.push(")\">ü {{ u.a }} 🦀 {{ u.");
    expected.push(Expected {
        code: TS2339,
        authored: src.mark("b"),
    });
    src.push(" }}</p>\n");
    src.push(
        "  <li v-for=\"item in items\" v-if=\"u.kind === 'a'\" @click=\"() => takeItem(item, u.",
    );
    expected.push(Expected {
        code: TS2339,
        authored: src.mark("zz"),
    });
    src.push(")\"></li>\n");
    finish(
        "mapping".into(),
        src,
        decls,
        bindings,
        contracts(&[("@click", "(event: MouseEvent) => void")]),
        expected,
        Vec::new(),
        2,
    )
}

/// The linear-representation matrix: an `n`-branch chain on optional props
/// `c0..` with `callbacks` callbacks per branch, the `v-else` reading the
/// negation of the opening condition. With `broken`, every callback reads a
/// reference its branch does not narrow, so each must report exactly once.
pub fn flat_matrix(n: usize, callbacks: usize, broken: bool) -> Fixture {
    let mut decls = String::from(
        "declare function take(value: string): void;\n\
         declare function none(value: undefined): void;\n\
         declare const __props: {\n",
    );
    let mut bindings = setup(&["take", "none"]);
    for i in 0..n {
        decls.push_str(&format!("  c{i}?: string;\n"));
        bindings.push((format!("c{i}"), BindingType::Props));
    }
    decls.push_str("};\n");
    let mut expected = Vec::new();
    let mut src = Src::new();
    for i in 0..n {
        let opening = match i {
            0 => format!("<b v-if=\"c{i} !== undefined\">"),
            _ if i == n - 1 => "<b v-else>".to_string(),
            _ => format!("<b v-else-if=\"c{i} !== undefined\">"),
        };
        src.push(&opening);
        for _ in 0..callbacks {
            if i == n - 1 {
                src.push(if broken {
                    "<i @click=\"() => take("
                } else {
                    "<i @click=\"() => none("
                });
                let read = src.mark("c0");
                if broken {
                    expected.push(Expected {
                        code: TS2345,
                        authored: read,
                    });
                }
            } else {
                src.push("<i @click=\"() => take(");
                let read = src.mark(&format!("c{}", if broken { i + 1 } else { i }));
                if broken {
                    expected.push(Expected {
                        code: TS2345,
                        authored: read,
                    });
                }
            }
            src.push(")\"></i>");
        }
        src.push("</b>\n");
    }
    finish(
        format!(
            "flat-matrix-{n}x{callbacks}{}",
            if broken { "-broken" } else { "" }
        ),
        src,
        decls,
        bindings,
        contracts(&[("@click", "(event: MouseEvent) => void")]),
        expected,
        Vec::new(),
        n * callbacks,
    )
}

/// The nested-scope matrix: `n` branches as `n / 4` nested levels. Everything
/// sits under one outermost `v-if` on `top`; level `i` is a four-branch chain
/// on `s{i}` whose opening branch holds level `i + 1` (every fourth level
/// inside a `v-for` frame), and every branch has a callback reading the
/// outermost positive and its own narrowed member. With `broken`, each
/// callback reads the next level's (not yet narrowed) member instead.
pub fn nested_matrix(n: usize, broken: bool) -> Fixture {
    assert!(n.is_multiple_of(4) && n >= 8);
    let levels = n / 4;
    let mut decls = String::from(
        "type S = 'a' | 'b' | 'c' | 'd';\n\
         declare function takeA(top: 'on', own: 'a', row: number): void;\n\
         declare function takeB(top: 'on', own: 'b'): void;\n\
         declare function takeC(top: 'on', own: 'c'): void;\n\
         declare function takeD(top: 'on', own: 'd'): void;\n\
         declare const rows: number[];\n\
         declare const __props: {\n  top: 'on' | 'off';\n",
    );
    let mut bindings = setup(&["takeA", "takeB", "takeC", "takeD", "rows"]);
    bindings.push(("top".into(), BindingType::Props));
    for i in 0..=levels {
        decls.push_str(&format!("  s{i}: S;\n"));
        bindings.push((format!("s{i}"), BindingType::Props));
    }
    decls.push_str("};\n");
    let mut expected = Vec::new();
    let mut src = Src::new();
    let mut read = |src: &mut Src, i: usize| {
        let span = src.mark(&format!("s{}", if broken { i + 1 } else { i }));
        if broken {
            expected.push(Expected {
                code: TS2345,
                authored: span,
            });
        }
    };
    src.push("<main v-if=\"top === 'on'\">\n");
    let mut row = String::from("0");
    for i in 0..levels {
        src.push(&format!("<div v-if=\"s{i} === 'a'\">"));
        if i % 4 == 3 {
            src.push(&format!("<template v-for=\"r{i} in rows\">"));
            row = format!("r{i}");
        }
        src.push("<i @click=\"() => takeA(top, ");
        read(&mut src, i);
        src.push(&format!(", {row})\"></i>\n"));
    }
    for i in (0..levels).rev() {
        if i % 4 == 3 {
            src.push("</template>");
        }
        src.push("</div>\n");
        for (directive, take) in [
            (format!("v-else-if=\"s{i} === 'b'\""), "takeB"),
            (format!("v-else-if=\"s{i} === 'c'\""), "takeC"),
            ("v-else".to_string(), "takeD"),
        ] {
            src.push(&format!("<div {directive}><i @click=\"() => {take}(top, "));
            read(&mut src, i);
            src.push(")\"></i></div>\n");
        }
    }
    src.push("</main>\n");
    finish(
        format!("nested-matrix-{n}{}", if broken { "-broken" } else { "" }),
        src,
        decls,
        bindings,
        contracts(&[("@click", "(event: MouseEvent) => void")]),
        expected,
        Vec::new(),
        n,
    )
}

/// Every semantic fixture of the exactness contract (valid and invalid
/// variants), in a fixed order.
pub fn semantic_suite() -> Vec<Fixture> {
    let mut fixtures = Vec::new();
    for root in Root::ALL {
        fixtures.push(three_branch_discriminator(root, false));
        fixtures.push(three_branch_discriminator(root, true));
        fixtures.push(flat_chain(root, 40, false));
        fixtures.push(flat_chain(root, 40, true));
        fixtures.push(contextual(root, false));
        fixtures.push(contextual(root, true));
        fixtures.push(lexical(root));
    }
    fixtures.push(deep_positives(40, false));
    fixtures.push(deep_positives(40, true));
    fixtures.push(mutation_and_nested_closure());
    fixtures.push(mapping());
    fixtures
}

/// The chain lengths and nesting depths of the linear-representation matrix.
pub const MATRIX_SIZES: [usize; 4] = [128, 256, 512, 1024];
