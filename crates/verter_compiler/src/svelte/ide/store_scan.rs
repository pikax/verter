//! Syntactic classification of Svelte legacy store auto-subscriptions (F11).
//!
//! A `$`-prefixed identifier in a Svelte component can be one of four things:
//!
//! 1. a STORE AUTO-SUBSCRIPTION (`$count` reads the value of the `count` store;
//!    `$count = v` writes through `count.set(v)`) — the legacy reactive sugar
//!    this module classifies and the projector rewrites;
//! 2. a RUNE (`$props`/`$state`/`$derived`/`$effect`/`$inspect`/`$host`/
//!    `$bindable`, including member forms `$state.raw` / `$derived.by` / …) —
//!    typed by the ambient prelude, NEVER a store-sub;
//! 3. the `$$`-MAGIC (`$$props`/`$$restProps`/`$$slots`) — handled by F12 as
//!    ambient prelude declarations, NEVER a store-sub (every `$$`-prefixed name
//!    is excluded);
//! 4. a LOCAL BINDING literally named `$x` (a `let $x = …` / a `$x` parameter /
//!    an import) — an ordinary variable, NEVER a store-sub.
//!
//! The classification is STRUCTURAL, from the OXC AST (the same grammar-correct
//! front-end the rest of the compiler uses), NOT a nominal name match: a
//! `$`-identifier is a store-sub only when it is a `$NAME` reference that is
//! neither a rune, nor `$$`-magic, nor lexically declared as a local `$NAME`
//! binding IN SCOPE at the reference. The local-binding suppression is
//! LEXICALLY SCOPED to the precise JS scope model — a scope-frame stack tracking
//! the program, every function / arrow body, every `{ … }` block, `for`-loop and
//! `catch` scope. `let`/`const` / params / function-declaration ids / catch
//! params bind in their introducing scope; `var` hoists to the enclosing
//! function/program frame; a named function-EXPRESSION id binds only in its own
//! body. A `$NAME` binding inside an UNRELATED scope (a nested function, a sibling
//! block, a loop) does NOT suppress a `$NAME` store-sub at a different scope (an
//! over-suppression that would strand a raw `$NAME` in the projected TSX), while
//! a binding in an enclosing scope still suppresses. Whether an occurrence is a READ or a WRITE is decided by AST
//! position — a `$NAME` that is the simple assignment TARGET (`$NAME = …`) is a
//! WRITE (it requires a `Writable<T>`); every other reference is a READ.
//!
//! Each occurrence is returned as a [`StoreSub`] carrying the `$`-byte / `=`
//! operator spans the projector rewrites — the rewrite touches ONLY those
//! spans, preserving the original identifier / RHS bytes so the projected
//! position maps back token-precisely (hover on a rewritten `$store` lands on
//! the original identifier bytes).
//!
//! Documented bounded boundary: a MEMBER write rooted at a store-sub
//! (`$obj.x = 1` / `$obj.x++`) classifies the `$obj` base as a READ — the
//! projection `__verter_store_get(obj).x = 1` mutates the read object's member,
//! valid TSX but it does not REQUIRE the store be `Writable` (Svelte's
//! `$obj.x = v` is a whole-object store set). A precise member-store-write
//! projection (read → mutate → set) is a follow-up; the common scalar
//! `$store = v` / `$store += v` / `$store++` writable forms ARE writable-checked.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, AssignmentExpression, AssignmentTarget, BindingIdentifier,
    BlockStatement, CatchClause, Class, ClassType, ForInStatement, ForOfStatement, ForStatement,
    Function, IdentifierReference, ImportOrExportKind, Program, SwitchStatement,
    VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use oxc_syntax::scope::ScopeFlags;

/// The Svelte 5 rune names — a `$`-identifier whose base name (after the single
/// leading `$`) matches one of these is a RUNE, never a store-subscription. The
/// member forms (`$state.raw`, `$derived.by`, …) share the base-name callee, so
/// matching the bare `$rune` identifier covers every member form.
const RUNE_NAMES: &[&str] = &[
    "$props",
    "$state",
    "$derived",
    "$effect",
    "$inspect",
    "$host",
    "$bindable",
];

/// Whether `name` is a Svelte 5 rune identifier (the `$`-prefixed form).
fn is_rune(name: &str) -> bool {
    RUNE_NAMES.contains(&name)
}

/// Whether `name` is the legacy `$$`-magic family (`$$props`/`$$restProps`/
/// `$$slots`) — and, defensively, ANY `$$`-prefixed identifier. The `$$`-magic
/// is typed by the F12 prelude declarations, NEVER rewritten as a store-sub.
fn is_double_dollar_magic(name: &str) -> bool {
    name.starts_with("$$")
}

/// A classified store auto-subscription occurrence (F11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoreSub {
    /// The byte offset (relative to the scanned text) of the leading `$`.
    pub dollar: u32,
    /// The byte offset just past the `$NAME` identifier (the identifier end).
    pub ident_end: u32,
    /// The store NAME (the identifier WITHOUT the leading `$`) — re-injected as
    /// text for the compound/update read+set projection, where the identifier
    /// appears twice (the original occurrence keeps its source span; the injected
    /// duplicate is unmapped).
    pub name: String,
    /// How the occurrence is projected.
    pub kind: StoreSubKind,
}

/// The projection shape of a classified store-sub occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StoreSubKind {
    /// A READ (`$NAME`) → `__verter_store_get(NAME)`.
    Read,
    /// A READ in an OBJECT-LITERAL SHORTHAND position (`{ $NAME }`). A bare
    /// `__verter_store_get(NAME)` is invalid in a shorthand slot, so the rewrite
    /// inserts the key: `{ $NAME: __verter_store_get(NAME) }` (the `$NAME` key
    /// bytes stay, the value side becomes the store-get).
    ShorthandRead,
    /// An LVALUE WRITE LEAF in a destructuring / `for`-of assignment TARGET
    /// (`[$NAME] = xs` / `({ x: $NAME } = obj)` / `for ($NAME of xs)`) →
    /// `__verter_store_lvalue(NAME).value` — a VALID assignment-target member
    /// access referencing only `NAME`, checking the destructured / iterated
    /// element against the store's `Writable<T>` value type. The bare `$NAME`
    /// identifier is the WRITE leaf; only its `$` byte is rewritten (the helper
    /// open) and the `.value` member is appended past the identifier.
    LvalueWrite,
    /// An LVALUE WRITE LEAF in an OBJECT-SHORTHAND destructuring target
    /// (`({ $NAME } = obj)`). A bare `__verter_store_lvalue(NAME).value` is
    /// invalid in a shorthand slot (no key), so the rewrite inserts the key:
    /// `({ $NAME: __verter_store_lvalue(NAME).value } = obj)` (the `$NAME` key
    /// bytes stay, the value side becomes the writable lvalue).
    ShorthandLvalueWrite,
    /// A simple-`=` WRITE (`$NAME = rhs`) → `__verter_store_set(NAME, rhs)`.
    SimpleWrite {
        /// The byte offset of the `=` operator.
        eq: u32,
        /// The byte offset just past the `=` operator (`eq + 1`).
        eq_end: u32,
        /// The byte offset just past the RHS (where the closing `)` goes).
        rhs_end: u32,
    },
    /// A COMPOUND WRITE (`$NAME OP= rhs`, e.g. `+=`) → a Writable-checked read+set
    /// `__verter_store_set(NAME, __verter_store_get(NAME) OP_BASE (rhs))`.
    CompoundWrite {
        /// The base binary operator (`+` for `+=`, `*` for `*=`, …).
        op_base: &'static str,
        /// The byte offset of the compound operator (`OP=`).
        op: u32,
        /// The byte offset just past the compound operator.
        op_end: u32,
        /// The byte offset just past the RHS (where the closing `))` goes).
        rhs_end: u32,
    },
    /// An UPDATE (`$NAME++` / `--$NAME`) → a Writable-checked numeric-precise
    /// read+set `__verter_store_set(NAME, __verter_store_update(
    /// __verter_store_get(NAME)))`. The `__verter_store_update<T extends number |
    /// bigint>` helper enforces the exact `++`/`--` operand constraint while
    /// preserving the value type (a `bigint` store passes, a `string`/`boolean`
    /// store FAILS — a plain `get(NAME) ± 1` would mis-judge both), so the
    /// projection does NOT spell out `OP_BASE 1`; `op_base` is retained for the
    /// operator-span rewrite (the `++`/`--` byte run) and the +/- direction is
    /// subsumed by the type-preserving `__verter_store_update` helper.
    Update {
        /// `+` for `++`, `-` for `--`.
        op_base: &'static str,
        /// The byte offset of the `++`/`--` operator.
        op: u32,
        /// The byte offset just past the `++`/`--` operator.
        op_end: u32,
        /// `true` for a PREFIX update (`++$x`), `false` for postfix (`$x++`).
        prefix: bool,
    },
}

/// Scan `text` (an instance/module script body OR a markup interpolation
/// expression) for legacy store auto-subscription occurrences.
///
/// Returns one [`StoreSub`] per store-sub `$NAME` reference, EXCLUDING runes,
/// `$$`-magic, and any `$NAME` that is lexically declared as a local binding in
/// `text`. An unparseable fragment yields no occurrences (fail-open) — the
/// projection's own validity is unaffected.
pub(super) fn scan_store_subscriptions_with(
    text: &str,
    extra_declared: &[String],
) -> Vec<StoreSub> {
    // Cheap bail: no `$`-identifier can exist without a `$` byte.
    if !text.contains('$') {
        return Vec::new();
    }
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parsed = Parser::new(&allocator, text, source_type).parse();
    if parsed.panicked {
        return Vec::new();
    }
    // Classify every `$NAME` reference with LEXICALLY-SCOPED local-binding
    // suppression: the collector tracks a scope stack (function/arrow bodies) and
    // suppresses a `$NAME` reference only when `$NAME` is declared in the
    // reference's own scope or an enclosing scope — NOT when an unrelated nested
    // function declares it. The caller-supplied `extra_declared` set (the
    // component script's TOP-LEVEL `$`-bindings) seeds the outermost (script)
    // scope so a markup expression respects a `let $x` declared at script scope
    // even though the script parses as a separate fragment.
    let mut collector = StoreSubCollector {
        source: text,
        extra_declared,
        scopes: Vec::new(),
        subs: Vec::new(),
    };
    collector.visit_program(&parsed.program);
    collector.subs
}

/// Scan a fragment with no extra (script-supplied) declared-name context — the
/// convenience form for a self-contained script body.
pub(super) fn scan_store_subscriptions(text: &str) -> Vec<StoreSub> {
    scan_store_subscriptions_with(text, &[])
}

/// Collect the SCRIPT-SCOPE (top-level) lexically-declared `$NAME` bindings in
/// `text` (a script body) — the names a markup expression must treat as ORDINARY
/// locals (not store-subs) even though it parses as a separate fragment. ONLY
/// top-level bindings are collected: a `$NAME` declared inside a nested function
/// body in the script is NOT in scope for a markup expression, so collecting it
/// would silently strand a markup `$NAME` store-sub (P1-4). A fragment that does
/// not parse yields an empty set.
pub(super) fn collect_declared_dollar_names(text: &str) -> Vec<String> {
    if !text.contains('$') {
        return Vec::new();
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, text, SourceType::tsx()).parse();
    if parsed.panicked {
        return Vec::new();
    }
    top_level_dollar_bindings(&parsed.program)
}

/// Collect the `$`-prefixed binding names introduced by a markup BLOCK BINDING
/// fragment — an `{#each list as PATTERN, INDEX}` item/index, a `{#await p then
/// PATTERN}` / `{:catch PATTERN}` binding, a `{#snippet name(PARAMS)}` param
/// list, or a `let:` slot-prop value. These bindings lexically scope to their
/// block subtree, so the projector must treat a `$`-named block binding as an
/// ORDINARY local (NOT a store-sub) when scanning that subtree's expressions.
///
/// The fragment is a BINDING PATTERN (or a comma-separated param list), not a
/// full statement, so it is wrapped in a throwaway binding context (`const
/// <pattern> = null as any;`) and the binding identifiers are collected
/// STRUCTURALLY from the parsed pattern — runes / `$$`-magic are excluded (they
/// are never block bindings). A fragment that does not parse yields no names
/// (fail-open — the projector then scans the subtree with the script-scope set
/// only, the prior behaviour).
pub(super) fn collect_pattern_dollar_names(pattern_text: &str) -> Vec<String> {
    if !pattern_text.contains('$') {
        return Vec::new();
    }
    // Wrap the pattern in an array-destructuring `const` so a bare identifier, a
    // destructuring pattern (`{a, b}` / `[a, b]`), AND a comma-separated param
    // list (`a, b` — a snippet's params) all parse as binding identifiers in one
    // declarator. `const [<text>] = null as any;` makes `a, b` two array
    // elements and `{x}` / `[y]` a single nested pattern element.
    let wrapped = format!("const [{pattern_text}] = null as any;");
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked {
        return Vec::new();
    }
    let mut names = LexicalDollarBindings::default();
    names.scan_statements_direct(&parsed.program.body);
    names.names
}

/// The byte length of the `const [` prefix the pattern-scanning wrapper prepends
/// (`const [<pattern>] = null as any;`) — every wrapped-fragment span offset is
/// translated back to the original pattern text by subtracting this.
const PATTERN_WRAPPER_PREFIX_LEN: u32 = "const [".len() as u32;

/// Scan a markup BLOCK-BINDING PATTERN fragment (`{#each list as PATTERN}` item,
/// `{:then PATTERN}` / `{:catch PATTERN}` binding, `{#snippet n(PARAMS)}` param
/// list, `let:` slot-prop value) for store auto-subscription READS inside its
/// DEFAULT-VALUE EXPRESSIONS (`{ x = $store }` / `($item = $store)`). The bound
/// NAMES are binding identifiers, never references, so they are NEVER classified
/// — only the default initializers (ordinary read contexts) yield store-subs.
///
/// Returns one [`StoreSub`] per default-expression store-read, with `dollar` /
/// `ident_end` offsets translated back to be RELATIVE TO `pattern_text` (the
/// wrapper prefix subtracted). `extra_declared` seeds the surrounding declared
/// `$`-names (script scope ∪ enclosing block bindings) so a default that
/// references a declared local stays an ordinary reference. A fragment that does
/// not parse, or whose subs fall before the wrapper prefix, yields nothing
/// (fail-open).
pub(super) fn scan_pattern_default_store_subs(
    pattern_text: &str,
    extra_declared: &[String],
) -> Vec<StoreSub> {
    if !pattern_text.contains('$') {
        return Vec::new();
    }
    // The SAME wrapper `collect_pattern_dollar_names` uses, so a bare identifier,
    // a destructuring pattern, and a comma-separated param list all parse as one
    // declarator's binding pattern. Default initializers parse as expressions —
    // the store-sub classifier finds reads there; the binding names are binding
    // identifiers (not references) and are never recorded.
    let wrapped = format!("const [{pattern_text}] = null as any;");
    let mut subs = scan_store_subscriptions_with(&wrapped, extra_declared);
    // A binding-pattern default is a pure VALUE expression — only READ-shaped
    // store-subs (`Read` / `ShorthandRead`, which carry just the `dollar` /
    // `ident_end` spans) are valid here. Restrict to those (a write-shaped kind,
    // whose extra operator/RHS offsets are NOT translated, cannot legitimately
    // appear in a default position) and drop any sub landing inside the wrapper
    // machinery (defensive — initializers are after the prefix).
    subs.retain(|s| {
        matches!(s.kind, StoreSubKind::Read | StoreSubKind::ShorthandRead)
            && s.dollar >= PATTERN_WRAPPER_PREFIX_LEN
    });
    for s in &mut subs {
        s.dollar -= PATTERN_WRAPPER_PREFIX_LEN;
        s.ident_end -= PATTERN_WRAPPER_PREFIX_LEN;
    }
    subs
}

/// Collect the SCRIPT-SCOPE (program/function-top-level) `$NAME` bindings: the
/// program's directly-declared `let`/`const`/function-declaration ids PLUS every
/// `var $NAME` hoisted from anywhere in the program body (descending blocks but
/// NOT nested functions). Used to seed the cross-fragment markup set — only the
/// program scope's effective bindings count.
fn top_level_dollar_bindings(program: &Program<'_>) -> Vec<String> {
    let mut names = LexicalDollarBindings::default();
    names.scan_statements_direct(&program.body);
    names.scan_var_hoists(&program.body);
    names.names
}

/// Collect a FUNCTION's own-scope `$NAME` bindings: its params, the named
/// function-EXPRESSION id (the recursion name is in scope only inside the
/// expression body — a function DECLARATION's id is in the ENCLOSING scope, not
/// here), the body's directly-declared `let`/`const`/inner-function-declaration
/// ids, PLUS every `var $NAME` hoisted from anywhere in the body (descending
/// blocks but NOT nested functions).
fn function_own_dollar_bindings(func: &Function<'_>) -> Vec<String> {
    let mut names = LexicalDollarBindings::default();
    names.add_formal_params(&func.params);
    // A function EXPRESSION's own id is bound in its own scope (recursion name).
    // A declaration's id is recorded by the ENCLOSING scope's direct scan, so it
    // must NOT be re-added here (it would still be correct — same name — but the
    // expression case is the one that REQUIRES the own-scope binding).
    if !func.is_declaration() {
        if let Some(id) = &func.id {
            names.add_binding(id);
        }
    }
    if let Some(body) = &func.body {
        names.scan_statements_direct(&body.statements);
        names.scan_var_hoists(&body.statements);
    }
    names.names
}

/// Collect an ARROW's own-scope `$NAME` bindings: its params + its body's
/// directly-declared `let`/`const`/function-declaration ids + hoisted `var`s
/// (arrows have no own id).
fn arrow_own_dollar_bindings(arrow: &ArrowFunctionExpression<'_>) -> Vec<String> {
    let mut names = LexicalDollarBindings::default();
    names.add_formal_params(&arrow.params);
    names.scan_statements_direct(&arrow.body.statements);
    names.scan_var_hoists(&arrow.body.statements);
    names.names
}

/// Collect a BLOCK scope's own `$NAME` bindings: the `let`/`const`/function-
/// declaration ids declared DIRECTLY in the block (no descent into nested blocks
/// or functions — `var` belongs to the enclosing function frame, not here).
fn block_own_dollar_bindings(block: &BlockStatement<'_>) -> Vec<String> {
    let mut names = LexicalDollarBindings::default();
    names.scan_statements_direct(&block.body);
    names.names
}

/// Collect the lexical `$NAME` bindings of a `for-in` / `for-of` LEFT — a
/// `for (const $x of …)` binds `$x` block-scoped over the loop (a `var` left
/// hoists to the enclosing function and is handled by the function frame's var
/// scan, so it is NOT collected here).
fn for_left_lexical_bindings(left: &oxc_ast::ast::ForStatementLeft<'_>) -> Vec<String> {
    let mut names = LexicalDollarBindings::default();
    if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(decl) = left {
        if !matches!(decl.kind, VariableDeclarationKind::Var) {
            for d in &decl.declarations {
                names.add_pattern(&d.id);
            }
        }
    }
    names.names
}

/// Whether `text` (an instance/module script body) USES a Svelte 5 rune — any
/// reference to a rune name (`$props`/`$state`/`$derived`/…, including member
/// forms `$state.raw` whose base callee is the rune identifier). A component
/// that uses ANY rune is in RUNES mode; one that uses none is LEGACY mode — and
/// only legacy mode has the `$$props`/`$$restProps`/`$$slots` magic (F12). The
/// classification is STRUCTURAL (OXC identifier references), not a substring
/// scan, so a rune NAME inside a string literal / comment does not mis-classify.
pub(super) fn text_uses_runes(text: &str) -> bool {
    if !text.contains('$') {
        return false;
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, text, SourceType::tsx()).parse();
    if parsed.panicked {
        // A fragment that does not parse cleanly cannot be classified — treat as
        // NOT-runes (legacy) is the conservative default for the magic objects,
        // but an unparseable script is already a type error the body surfaces.
        return false;
    }
    let mut detector = RuneUseDetector { used: false };
    detector.visit_program(&parsed.program);
    detector.used
}

/// Detects whether any rune identifier is referenced.
struct RuneUseDetector {
    used: bool,
}

impl<'a> Visit<'a> for RuneUseDetector {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if is_rune(it.name.as_str()) {
            self.used = true;
        }
        walk::walk_identifier_reference(self, it);
    }
}

/// Accumulator + scanners for the `$`-prefixed lexical bindings of ONE scope.
///
/// Two scan modes model JS lexical scoping precisely so a binding suppresses a
/// store-sub ONLY where it is actually in scope (no over-suppression that would
/// strand a raw `$NAME`, no under-suppression that would mis-rewrite a real
/// local):
///
/// - [`scan_statements_direct`] collects the `let`/`const` declarator names +
///   function-DECLARATION ids declared DIRECTLY in a statement list — it does
///   NOT descend into nested blocks / `for`-bodies / `catch` blocks (each opens
///   its own block scope, pre-scanned when the classifier enters it) NOR into
///   nested function / arrow bodies.
/// - [`scan_var_hoists`] collects `var $NAME` declared ANYWHERE under a function
///   body (descending blocks / `for` / `if` / `catch`, since `var` hoists to the
///   function scope) but NOT into nested functions / arrows.
#[derive(Default)]
struct LexicalDollarBindings {
    names: Vec<String>,
}

impl LexicalDollarBindings {
    /// Record a `$`-prefixed binding identifier (skipping runes / `$$`-magic).
    fn add_binding(&mut self, id: &BindingIdentifier<'_>) {
        let name = id.name.as_str();
        if name.starts_with('$') && !is_rune(name) && !is_double_dollar_magic(name) {
            self.names.push(name.to_string());
        }
    }

    /// Record every `$`-prefixed binding identifier in a binding PATTERN (an
    /// identifier, or a destructuring pattern — `const { $a, $b } = …`).
    fn add_pattern(&mut self, pattern: &oxc_ast::ast::BindingPattern<'_>) {
        let mut collector = PatternBindings { out: self };
        collector.visit_binding_pattern(pattern);
    }

    /// Record the `$`-prefixed binding identifiers of a function/arrow params list.
    fn add_formal_params(&mut self, params: &oxc_ast::ast::FormalParameters<'_>) {
        for param in &params.items {
            self.add_pattern(&param.pattern);
        }
        if let Some(rest) = &params.rest {
            self.add_pattern(&rest.rest.argument);
        }
    }

    /// Collect the DIRECT scope-introducing bindings of a statement list (NOT
    /// descending nested blocks / functions): `let`/`const`/`using` declarators,
    /// function / class / enum / namespace DECLARATION ids, IMPORT locals, and the
    /// same forms re-exported via `export …` / `export default function/class`. A
    /// `var` is hoisted by `scan_var_hoists`, not collected here.
    fn scan_statements_direct(&mut self, stmts: &oxc_allocator::Vec<oxc_ast::ast::Statement<'_>>) {
        use oxc_ast::ast::{ExportDefaultDeclarationKind, ImportDeclarationSpecifier, Statement};
        for stmt in stmts {
            match stmt {
                Statement::ImportDeclaration(import) => {
                    // Each VALUE import specifier binds a LOCAL in this scope. A
                    // whole `import type { … }` (or a per-specifier `import { type
                    // Foo as $x }`) is a TYPE-only import — it binds NO value, so it
                    // must NOT suppress a value-position `$x` store-sub.
                    if matches!(import.import_kind, ImportOrExportKind::Value) {
                        if let Some(specifiers) = &import.specifiers {
                            for spec in specifiers {
                                let local = match spec {
                                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                                        if matches!(s.import_kind, ImportOrExportKind::Type) {
                                            continue; // `import { type Foo as $x }` — type-only
                                        }
                                        &s.local
                                    }
                                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                                        &s.local
                                    }
                                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                                        &s.local
                                    }
                                };
                                self.add_binding(local);
                            }
                        }
                    }
                }
                Statement::TSImportEqualsDeclaration(import_eq)
                    if matches!(import_eq.import_kind, ImportOrExportKind::Value) =>
                {
                    // `import $foo = require("x")` / `import $ns = A.B` binds a
                    // VALUE local `$foo`/`$ns` (a type-only `import type X = …` does
                    // not).
                    self.add_binding(&import_eq.id);
                }
                Statement::ExportNamedDeclaration(export) => {
                    if let Some(decl) = &export.declaration {
                        self.add_declaration(decl);
                    }
                }
                Statement::ExportDefaultDeclaration(export) => {
                    // `export default function $f(){}` / `export default class $C {}`
                    // bind a name in this scope when named.
                    match &export.declaration {
                        ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                            if let Some(id) = &func.id {
                                self.add_binding(id);
                            }
                        }
                        ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                            if let Some(id) = &class.id {
                                self.add_binding(id);
                            }
                        }
                        _ => {}
                    }
                }
                _ if stmt.is_declaration() => {
                    if let Some(decl) = stmt.as_declaration() {
                        self.add_declaration(decl);
                    }
                }
                _ => {}
            }
        }
    }

    /// Record the scope-binding name(s) of a `Declaration` (a `let`/`const`
    /// variable group, a function / class / enum / namespace declaration). `var`
    /// is excluded here (function-scoped — collected by `scan_var_hoists`).
    fn add_declaration(&mut self, decl: &oxc_ast::ast::Declaration<'_>) {
        use oxc_ast::ast::Declaration;
        match decl {
            Declaration::VariableDeclaration(v)
                if !matches!(v.kind, VariableDeclarationKind::Var) =>
            {
                for d in &v.declarations {
                    self.add_pattern(&d.id);
                }
            }
            Declaration::FunctionDeclaration(func) => {
                if let Some(id) = &func.id {
                    self.add_binding(id);
                }
            }
            Declaration::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    self.add_binding(id);
                }
            }
            Declaration::TSEnumDeclaration(e) => self.add_binding(&e.id),
            Declaration::TSModuleDeclaration(m) => {
                if let oxc_ast::ast::TSModuleDeclarationName::Identifier(id) = &m.id {
                    let name = id.name.as_str();
                    if name.starts_with('$') && !is_rune(name) && !is_double_dollar_magic(name) {
                        self.names.push(name.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    /// Collect every `var $NAME` hoisted to the function/program scope — descend
    /// blocks / `for` / `if` / `catch` (var hoists past them) but NOT nested
    /// functions / arrows.
    fn scan_var_hoists(&mut self, stmts: &oxc_allocator::Vec<oxc_ast::ast::Statement<'_>>) {
        let mut hoist = VarHoistScan { out: self };
        for stmt in stmts {
            hoist.visit_statement(stmt);
        }
    }
}

/// Collects `$`-prefixed identifiers inside a single binding PATTERN.
struct PatternBindings<'o> {
    out: &'o mut LexicalDollarBindings,
}

impl<'a> Visit<'a> for PatternBindings<'_> {
    fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
        self.out.add_binding(it);
        walk::walk_binding_identifier(self, it);
    }
}

/// Collects `var $NAME` hoists under a function body — descends blocks / control
/// flow but NOT nested functions / arrows (those open their own scopes).
struct VarHoistScan<'o> {
    out: &'o mut LexicalDollarBindings,
}

impl<'a> Visit<'a> for VarHoistScan<'_> {
    fn visit_variable_declaration(&mut self, it: &oxc_ast::ast::VariableDeclaration<'a>) {
        if matches!(it.kind, VariableDeclarationKind::Var) {
            for d in &it.declarations {
                self.out.add_pattern(&d.id);
            }
        }
        // Do NOT descend further — initializers cannot contain hoisting `var`
        // declarations that escape this scope (a `var` inside an initializer's
        // function body belongs to that function).
    }

    // A nested function / arrow / class static block / TS module (namespace)
    // block opens its OWN var scope — its `var`s do not hoist into THIS frame. Do
    // not descend into any of them.
    fn visit_function(&mut self, _it: &Function<'a>, _flags: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {}
    fn visit_static_block(&mut self, _it: &oxc_ast::ast::StaticBlock<'a>) {}
    fn visit_ts_module_block(&mut self, _it: &oxc_ast::ast::TSModuleBlock<'a>) {}
}

/// Classifies `$NAME` references into store-sub READ / WRITE occurrences, with
/// LEXICALLY-SCOPED local-binding suppression. The collector tracks a stack of
/// scope frames (the program scope at the bottom, one per enclosing function /
/// arrow body): a `$NAME` reference is suppressed only when `$NAME` is declared
/// in some currently-active frame (its own scope or an enclosing one) — a binding
/// in an unrelated, already-popped nested scope never suppresses it.
struct StoreSubCollector<'s, 'd> {
    source: &'s str,
    /// The caller-supplied SCRIPT-SCOPE declared `$`-names (the component script's
    /// top-level bindings) — seeded into the outermost frame so a markup fragment
    /// respects a script-scope `let $x`.
    extra_declared: &'d [String],
    /// The active lexical scope frames (bottom = program scope). Each frame holds
    /// the `$`-names declared directly in that scope.
    scopes: Vec<Vec<String>>,
    subs: Vec<StoreSub>,
}

impl<'s, 'd> StoreSubCollector<'s, 'd> {
    /// Whether `name` (a `$NAME` identifier) is a store auto-subscription:
    /// `$`-prefixed, not a rune, not `$$`-magic, and not lexically declared in any
    /// currently-active scope frame (or the script-scope `extra_declared` set).
    fn is_store_sub(&self, name: &str) -> bool {
        name.starts_with('$')
            && !is_rune(name)
            && !is_double_dollar_magic(name)
            && !self.extra_declared.iter().any(|d| d == name)
            && !self
                .scopes
                .iter()
                .any(|frame| frame.iter().any(|d| d == name))
    }
}

impl<'a, 's, 'd> StoreSubCollector<'s, 'd> {
    /// Walk an ASSIGNMENT TARGET (the LHS of `… = rhs` / a `for (… of/in …)`
    /// binding-less left), classifying VALUE sub-positions as reads while NEVER
    /// emitting the READ helper for a target identifier (an `lvalue` `$NAME` is a
    /// WRITE position — a `__verter_store_get(NAME)` there is invalid TSX).
    ///
    /// The split, per target shape:
    /// - a bare target identifier (`$count` in `[$count] = xs`) is a WRITE leaf →
    ///   the `LvalueWrite` projection (`__verter_store_lvalue(count).value`), a
    ///   valid assignment-target member access referencing only `count`. A
    ///   READ helper (`__verter_store_get(count)`) in an lvalue is invalid TSX,
    ///   and SUPPRESSING the leaf would strand a raw `$count` — both wrong.
    /// - a MEMBER-expression target (`$obj.x` / `$obj[k]`) reads its `object`
    ///   (the documented member-write safe-degrade — `$obj` IS a read) and its
    ///   computed key is a value read.
    /// - a TS-cast target (`($x as T) = …`) unwraps to its inner target.
    /// - array / object destructuring targets recurse into their elements; a
    ///   `WithDefault` default initializer and a computed property name are VALUE
    ///   reads (visited normally).
    fn visit_assignment_target_inner(&mut self, target: &AssignmentTarget<'a>) {
        use oxc_ast::ast::AssignmentTarget as AT;
        match target {
            // Bare identifier WRITE leaf — the lvalue-write projection.
            AT::AssignmentTargetIdentifier(id) => {
                self.record_lvalue_write(id.name.as_str(), id.span);
            }
            // Member targets: the object (and computed key) are value reads.
            AT::ComputedMemberExpression(m) => {
                self.visit_expression(&m.object);
                self.visit_expression(&m.expression);
            }
            AT::StaticMemberExpression(m) => self.visit_expression(&m.object),
            AT::PrivateFieldExpression(m) => self.visit_expression(&m.object),
            // TS-cast targets unwrap to the inner SIMPLE target.
            AT::TSAsExpression(e) => self.visit_simple_lvalue_expression(&e.expression),
            AT::TSSatisfiesExpression(e) => self.visit_simple_lvalue_expression(&e.expression),
            AT::TSNonNullExpression(e) => self.visit_simple_lvalue_expression(&e.expression),
            AT::TSTypeAssertion(e) => self.visit_simple_lvalue_expression(&e.expression),
            // Destructuring targets recurse.
            AT::ArrayAssignmentTarget(arr) => {
                for el in arr.elements.iter().flatten() {
                    self.visit_assignment_target_maybe_default(el);
                }
                if let Some(rest) = &arr.rest {
                    self.visit_assignment_target_inner(&rest.target);
                }
            }
            AT::ObjectAssignmentTarget(obj) => {
                for prop in &obj.properties {
                    self.visit_assignment_target_property(prop);
                }
                if let Some(rest) = &obj.rest {
                    self.visit_assignment_target_inner(&rest.target);
                }
            }
        }
    }

    /// A target wrapped behind a TS cast unwraps to a value expression that is
    /// itself a simple assignment target — re-route the common member/identifier
    /// forms; an arbitrary inner expression is visited as a value read.
    fn visit_simple_lvalue_expression(&mut self, expr: &oxc_ast::ast::Expression<'a>) {
        use oxc_ast::ast::Expression as E;
        match expr {
            // A bare identifier inner is still a WRITE leaf — lvalue-write.
            E::Identifier(id) => {
                self.record_lvalue_write(id.name.as_str(), id.span);
            }
            E::StaticMemberExpression(m) => self.visit_expression(&m.object),
            E::ComputedMemberExpression(m) => {
                self.visit_expression(&m.object);
                self.visit_expression(&m.expression);
            }
            E::PrivateFieldExpression(m) => self.visit_expression(&m.object),
            // Any other inner expression is treated as a value read (conservative
            // — it is not a recognised lvalue leaf).
            other => self.visit_expression(other),
        }
    }

    /// A destructuring element (`[a]` / `[a = default]`): the binding is a target,
    /// the default initializer is a VALUE read.
    fn visit_assignment_target_maybe_default(
        &mut self,
        el: &oxc_ast::ast::AssignmentTargetMaybeDefault<'a>,
    ) {
        use oxc_ast::ast::AssignmentTargetMaybeDefault as MD;
        match el {
            MD::AssignmentTargetWithDefault(wd) => {
                self.visit_assignment_target_inner(&wd.binding);
                self.visit_expression(&wd.init); // the default value is a read
            }
            // The inherited `AssignmentTarget` variants — route back through the
            // target walker.
            other => {
                if let Some(target) = other.as_assignment_target() {
                    self.visit_assignment_target_inner(target);
                }
            }
        }
    }

    /// An object destructuring property (`{ a }` / `{ a = d }` / `{ k: t }` /
    /// `{ [c]: t }`): the binding is a target, defaults + computed keys are reads.
    fn visit_assignment_target_property(
        &mut self,
        prop: &oxc_ast::ast::AssignmentTargetProperty<'a>,
    ) {
        use oxc_ast::ast::AssignmentTargetProperty as P;
        match prop {
            P::AssignmentTargetPropertyIdentifier(id) => {
                // `{ $a }` / `{ $a = default }` — the shorthand `binding`
                // identifier is a WRITE leaf needing a synthesised key
                // (shorthand-lvalue-write); the optional default `init` is a read.
                let name = id.binding.name.as_str();
                if self.is_store_sub(name) {
                    self.subs.push(StoreSub {
                        dollar: id.binding.span.start,
                        ident_end: id.binding.span.end,
                        name: name[1..].to_string(),
                        kind: StoreSubKind::ShorthandLvalueWrite,
                    });
                }
                if let Some(init) = &id.init {
                    self.visit_expression(init);
                }
            }
            P::AssignmentTargetPropertyProperty(pp) => {
                // `{ k: target }` / `{ [computed]: target }` — a computed key is a
                // value read; the binding recurses through the maybe-default rule.
                if pp.computed {
                    self.visit_property_key(&pp.name);
                }
                self.visit_assignment_target_maybe_default(&pp.binding);
            }
        }
    }

    /// Record a bare WRITE-leaf identifier (`$NAME` in a destructuring / `for`-of
    /// target) as an [`StoreSubKind::LvalueWrite`] when it classifies as a
    /// store-sub. A non-store-sub identifier (a rune, `$$`-magic, a declared
    /// local, or any non-`$` name) is left untouched.
    fn record_lvalue_write(&mut self, name: &str, span: Span) {
        if self.is_store_sub(name) {
            self.subs.push(StoreSub {
                dollar: span.start,
                ident_end: span.end,
                name: name[1..].to_string(),
                kind: StoreSubKind::LvalueWrite,
            });
        }
    }

    /// The public entry: visit an assignment target, classifying its write-leaf
    /// identifiers (lvalue-write) and its value sub-positions (reads).
    fn visit_assignment_target(&mut self, target: &AssignmentTarget<'a>) {
        self.visit_assignment_target_inner(target);
    }
}

impl<'a, 's, 'd> Visit<'a> for StoreSubCollector<'s, 'd> {
    fn visit_program(&mut self, it: &Program<'a>) {
        // Pre-scan the program (script) scope's DIRECT `$`-bindings so a reference
        // before its hoisted declaration in this scope is suppressed too.
        let frame = top_level_dollar_bindings(it);
        self.scopes.push(frame);
        walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        // A function body opens a fresh lexical scope: pre-scan its params + own
        // id (for a fn EXPRESSION) + body direct/hoisted `$`-bindings, push the
        // frame, walk, pop. Its bindings suppress only references lexically inside.
        let frame = function_own_dollar_bindings(it);
        self.scopes.push(frame);
        walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        let frame = arrow_own_dollar_bindings(it);
        self.scopes.push(frame);
        walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &BlockStatement<'a>) {
        // A `{ … }` block opens its own lexical scope for `let`/`const`/inner
        // function declarations — pre-scan its DIRECT block bindings, push, walk,
        // pop. A block-local `let $x` thus suppresses references inside the block
        // only, not an outer same-name store-sub.
        let frame = block_own_dollar_bindings(it);
        self.scopes.push(frame);
        walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        // `for (let $i …)` introduces a per-loop lexical scope binding the init
        // declarators (block-scoped under `let`/`const`). Pre-scan the init's
        // `let`/`const` declarators into a frame covering the whole loop.
        let mut frame = LexicalDollarBindings::default();
        if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) = &it.init {
            if !matches!(decl.kind, VariableDeclarationKind::Var) {
                for d in &decl.declarations {
                    frame.add_pattern(&d.id);
                }
            }
        }
        self.scopes.push(frame.names);
        walk::walk_for_statement(self, it);
        self.scopes.pop();
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        // `catch ($e) { … }` opens TWO nested lexical scopes: the catch-PARAM
        // scope (binding the param pattern) ENCLOSES the catch-BODY block scope
        // (binding the body's own `let`/`const`). The param's destructuring
        // DEFAULTS (`catch ({ x = $store })` — a value expression) are evaluated
        // in the PARAM scope, which does NOT see the body's declarations: a body
        // `let $store` must NOT over-suppress the param-default `$store` read. So
        // the param frame and its pattern are visited FIRST (excluding the body
        // declarations), then the body frame is pushed for the statements.
        let mut param_frame = LexicalDollarBindings::default();
        if let Some(param) = &it.param {
            param_frame.add_pattern(&param.pattern);
        }
        self.scopes.push(param_frame.names);
        // Visit the param PATTERN under the param-only frame so a destructuring
        // DEFAULT value's store-sub is classified against the param scope (not the
        // body's later declarations), while the param's binding identifiers stay
        // non-references.
        if let Some(param) = &it.param {
            self.visit_binding_pattern(&param.pattern);
        }
        // The body block opens its own nested scope: pre-scan its direct
        // `let`/`const` bindings into a second frame ABOVE the param frame, then
        // visit the body statements. (Visit the statements directly rather than
        // `walk_block_statement`, which would re-push a third block frame.)
        let mut body_frame = LexicalDollarBindings::default();
        body_frame.scan_statements_direct(&it.body.body);
        self.scopes.push(body_frame.names);
        for stmt in &it.body.body {
            self.visit_statement(stmt);
        }
        self.scopes.pop(); // body frame
        self.scopes.pop(); // param frame
    }

    fn visit_switch_statement(&mut self, it: &SwitchStatement<'a>) {
        // A `switch` body is ONE shared block scope across all cases (a `let $x`
        // in one case is visible to later cases, TDZ aside). The discriminant is
        // evaluated in the ENCLOSING scope, so visit it BEFORE pushing the frame.
        self.visit_expression(&it.discriminant);
        let mut frame = LexicalDollarBindings::default();
        for case in &it.cases {
            frame.scan_statements_direct(&case.consequent);
        }
        self.scopes.push(frame.names);
        for case in &it.cases {
            if let Some(test) = &case.test {
                self.visit_expression(test);
            }
            for stmt in &case.consequent {
                self.visit_statement(stmt);
            }
        }
        self.scopes.pop();
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        // A named class EXPRESSION (`const C = class $C { … }`) binds `$C` only
        // inside its OWN body (the class name, like a fn-expression's recursion
        // name) — push a one-name frame for the expression body. A class
        // DECLARATION's id is bound in the ENCLOSING scope (recorded by that
        // scope's direct scan), so it needs no own frame here.
        if matches!(it.r#type, ClassType::ClassExpression) {
            if let Some(id) = &it.id {
                let mut frame = LexicalDollarBindings::default();
                frame.add_binding(id);
                self.scopes.push(frame.names);
                walk::walk_class(self, it);
                self.scopes.pop();
                return;
            }
        }
        walk::walk_class(self, it);
    }

    fn visit_static_block(&mut self, it: &oxc_ast::ast::StaticBlock<'a>) {
        // A class `static { … }` block is its OWN var scope (let/const/var all
        // local to it). Pre-scan its direct lexical + hoisted-var bindings into one
        // frame covering the block.
        let mut frame = LexicalDollarBindings::default();
        frame.scan_statements_direct(&it.body);
        frame.scan_var_hoists(&it.body);
        self.scopes.push(frame.names);
        for stmt in &it.body {
            self.visit_statement(stmt);
        }
        self.scopes.pop();
    }

    fn visit_ts_module_block(&mut self, it: &oxc_ast::ast::TSModuleBlock<'a>) {
        // A TS `namespace`/`module` block is its OWN scope. Pre-scan its direct
        // lexical + hoisted-var bindings into one frame.
        let mut frame = LexicalDollarBindings::default();
        frame.scan_statements_direct(&it.body);
        frame.scan_var_hoists(&it.body);
        self.scopes.push(frame.names);
        for stmt in &it.body {
            self.visit_statement(stmt);
        }
        self.scopes.pop();
    }

    fn visit_object_property(&mut self, it: &oxc_ast::ast::ObjectProperty<'a>) {
        // A SHORTHAND object property `{ $store }` — OXC's `value` is an
        // `IdentifierReference`, so the generic identifier-read rewrite would emit
        // the invalid `{ __verter_store_get(store) }` (a bare call in a property
        // slot with no key). Record a `ShorthandRead` and DON'T walk the value (so
        // it is not double-recorded as a plain Read); the projector inserts the
        // key (`{ $store: __verter_store_get(store) }`).
        if it.shorthand {
            if let oxc_ast::ast::Expression::Identifier(id) = &it.value {
                let name = id.name.as_str();
                if self.is_store_sub(name) {
                    self.subs.push(StoreSub {
                        dollar: id.span.start,
                        ident_end: id.span.end,
                        name: name[1..].to_string(),
                        kind: StoreSubKind::ShorthandRead,
                    });
                    // Walk the key only would re-hit the same span; the key of a
                    // shorthand is the SAME identifier, so skip walking entirely.
                    return;
                }
            }
        }
        walk::walk_object_property(self, it);
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        if let AssignmentTarget::AssignmentTargetIdentifier(target) = &it.left {
            let name = target.name.as_str();
            if self.is_store_sub(name) {
                let target_span = target.span;
                let store_name = name[1..].to_string(); // drop the leading `$`
                if it.operator.as_str() == "=" {
                    // SIMPLE-`=` store WRITE: `$NAME = rhs` → `store.set(rhs)` form.
                    let eq = self.find_op_after(target_span.end, it.right.span().start, "=");
                    self.subs.push(StoreSub {
                        dollar: target_span.start,
                        ident_end: target_span.end,
                        name: store_name,
                        kind: StoreSubKind::SimpleWrite {
                            eq,
                            eq_end: eq + 1,
                            rhs_end: it.right.span().end,
                        },
                    });
                } else if let Some(op_base) = compound_op_base(it.operator.as_str()) {
                    // COMPOUND WRITE (`$NAME OP= rhs`) → a Writable-checked read+set
                    // `__verter_store_set(NAME, __verter_store_get(NAME) OP_BASE (rhs))`.
                    let op_str = it.operator.as_str();
                    let op = self.find_op_after(target_span.end, it.right.span().start, op_str);
                    self.subs.push(StoreSub {
                        dollar: target_span.start,
                        ident_end: target_span.end,
                        name: store_name,
                        kind: StoreSubKind::CompoundWrite {
                            op_base,
                            op,
                            op_end: op + op_str.len() as u32,
                            rhs_end: it.right.span().end,
                        },
                    });
                }
                // Walk ONLY the RHS — the target identifier is the write LHS,
                // already consumed; re-visiting it would double-record it as a
                // READ.
                self.visit_expression(&it.right);
                return;
            }
        }
        // Any OTHER assignment target — a destructuring pattern (`[$count] = xs`
        // / `({ x: $count } = obj)`), a member-expression target (`$obj.x = v`),
        // a TS-cast target — is NOT a bare scalar store write. Its TARGET
        // identifiers are WRITE positions: a `$NAME` in target position must
        // NEVER emit the READ helper (`__verter_store_get(NAME)` in an lvalue is
        // invalid TSX). Route the target through the lvalue walker (suppresses
        // target identifiers, still visits value sub-positions — defaults,
        // computed keys, member objects) and visit the RHS normally.
        self.visit_assignment_target(&it.left);
        self.visit_expression(&it.right);
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        let frame = for_left_lexical_bindings(&it.left);
        self.scopes.push(frame);
        // The RIGHT (iterable) is a value READ — visit normally.
        self.visit_expression(&it.right);
        // The LEFT is either a `let`/`const`/`var` DECLARATION (a binding, never a
        // read — handled by the lexical frame) or an ASSIGNMENT TARGET (`for
        // ($count of xs)`), whose target identifiers are WRITE positions that must
        // NOT emit the READ helper. Route an assignment-target left through the
        // lvalue walker; descend a declaration left's initializers as values.
        match &it.left {
            oxc_ast::ast::ForStatementLeft::VariableDeclaration(decl) => {
                self.visit_variable_declaration(decl);
            }
            _ => {
                if let Some(target) = it.left.as_assignment_target() {
                    self.visit_assignment_target(target);
                }
            }
        }
        self.visit_statement(&it.body);
        self.scopes.pop();
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        let frame = for_left_lexical_bindings(&it.left);
        self.scopes.push(frame);
        self.visit_expression(&it.right);
        match &it.left {
            oxc_ast::ast::ForStatementLeft::VariableDeclaration(decl) => {
                self.visit_variable_declaration(decl);
            }
            _ => {
                if let Some(target) = it.left.as_assignment_target() {
                    self.visit_assignment_target(target);
                }
            }
        }
        self.visit_statement(&it.body);
        self.scopes.pop();
    }

    fn visit_update_expression(&mut self, it: &oxc_ast::ast::UpdateExpression<'a>) {
        // `$count++` / `--$count`: an update of a bare store name → a
        // Writable-checked read+set `__verter_store_set(NAME, __verter_store_get(
        // NAME) ± 1)`. The pre/post distinction is irrelevant in a statement
        // position (the projected value is the new value either way); the
        // projection is the same for both.
        if let oxc_ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(target) =
            &it.argument
        {
            let name = target.name.as_str();
            if self.is_store_sub(name) {
                let prefix = it.prefix;
                let op_base = if it.operator.as_str() == "++" {
                    "+"
                } else {
                    "-"
                };
                // The `++`/`--` operator sits on the side opposite the operand.
                let (op, op_end) = if prefix {
                    (it.span.start, it.span.start + 2)
                } else {
                    (target.span.end, it.span.end)
                };
                self.subs.push(StoreSub {
                    dollar: target.span.start,
                    ident_end: target.span.end,
                    name: name[1..].to_string(),
                    kind: StoreSubKind::Update {
                        op_base,
                        op,
                        op_end,
                        prefix,
                    },
                });
                return;
            }
        }
        walk::walk_update_expression(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        let name = it.name.as_str();
        if self.is_store_sub(name) {
            self.subs.push(StoreSub {
                dollar: it.span.start,
                ident_end: it.span.end,
                name: name[1..].to_string(),
                kind: StoreSubKind::Read,
            });
        }
        walk::walk_identifier_reference(self, it);
    }

    // The store rewrite is VALUE-expression-only. A `$`-prefixed identifier in any
    // TS TYPE position is a TYPE reference, NEVER a store-sub — descending into type
    // syntax would route the type-space `$Foo` through `visit_identifier_reference`
    // and inject an invalid `__verter_store_get(Foo)` into a type position.
    //
    // The LOAD-BEARING interceptor is `visit_ts_type_name`: it is the SOLE bridge
    // from every TS-type-syntax parent to the shared value `visit_identifier_reference`
    // (`walk_ts_type_name` routes a `TSTypeName::IdentifierReference` through it). No-
    // opping it stops EVERY type-space identifier — a type annotation, a type-alias
    // body, an `extends`/`implements` heritage clause, an `as`/`satisfies` annotation,
    // a generic type argument, a type-parameter constraint/default, a `typeof` type
    // query, an indexed-access/`keyof` operand — regardless of which type-bearing
    // parent reached it, including the parents (class `implements`, `typeof` queries)
    // that do NOT pass through `visit_ts_type`. The `visit_ts_type` /
    // `visit_ts_type_annotation` / alias / interface no-ops additionally prune the
    // (large) type subtrees early so the walk never descends them at all.
    fn visit_ts_type_name(&mut self, _it: &oxc_ast::ast::TSTypeName<'a>) {}
    fn visit_ts_type(&mut self, _it: &oxc_ast::ast::TSType<'a>) {}
    fn visit_ts_type_annotation(&mut self, _it: &oxc_ast::ast::TSTypeAnnotation<'a>) {}
    fn visit_ts_type_alias_declaration(&mut self, _it: &oxc_ast::ast::TSTypeAliasDeclaration<'a>) {}
    fn visit_ts_interface_declaration(&mut self, _it: &oxc_ast::ast::TSInterfaceDeclaration<'a>) {}
}

/// The base binary operator for a compound assignment operator, or `None` for an
/// operator that is not a simple arithmetic/bitwise/logical compound (kept
/// conservative — only the operators with a clean `a OP b` desugaring).
fn compound_op_base(op: &str) -> Option<&'static str> {
    Some(match op {
        "+=" => "+",
        "-=" => "-",
        "*=" => "*",
        "/=" => "/",
        "%=" => "%",
        "**=" => "**",
        "&=" => "&",
        "|=" => "|",
        "^=" => "^",
        "<<=" => "<<",
        ">>=" => ">>",
        ">>>=" => ">>>",
        "&&=" => "&&",
        "||=" => "||",
        "??=" => "??",
        _ => return None,
    })
}

impl<'s, 'd> StoreSubCollector<'s, 'd> {
    /// Locate the assignment-operator byte (`op`, e.g. `=` / `+=`) between the
    /// target end (`from`) and the RHS start (`to`). The window is whitespace +
    /// comments + the operator; an `op` occurring INSIDE a `//` line comment or a
    /// `/* */` block comment is skipped so the operator span is structurally
    /// precise (`$count /* = */ = 1` and `$count // = note\n = 1` both find the
    /// real `=`). Returns `from` if not found (the rewrite then degrades to a
    /// zero-width insert at `from` — fail-safe, no out-of-range span).
    fn find_op_after(&self, from: u32, to: u32, op: &str) -> u32 {
        let window = &self.source[from as usize..to as usize];
        let bytes = window.as_bytes();
        let op_bytes = op.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                    // Line comment — skip THROUGH the newline and keep scanning.
                    match window[i + 2..].find('\n') {
                        Some(nl) => {
                            i = i + 2 + nl + 1;
                            continue;
                        }
                        None => break,
                    }
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    // Block comment — skip to the closing `*/`.
                    if let Some(end) = window[i + 2..].find("*/") {
                        i = i + 2 + end + 2;
                        continue;
                    }
                    break;
                }
                _ if bytes[i..].starts_with(op_bytes) => return from + i as u32,
                _ => {}
            }
            i += 1;
        }
        from
    }
}
