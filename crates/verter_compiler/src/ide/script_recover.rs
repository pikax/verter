//! Token-scan recovery for `<script setup>` content that does not parse cleanly.
//!
//! This is the SINGLE recovery surface for the IDE failure path: when OXC fails to
//! parse the original `<script setup>` content (the user is mid-edit — a dangling
//! `a.`, an open delimiter, an unbalanced brace), [`ScriptTokenScanner::recover_plan`]
//! produces a [`ScriptSetupRecoveryPlan`] from a single token scan of the REAL
//! source. There is no synthesize-then-reparse step and no synthetic view is ever
//! parsed. The scanner handles comments, strings, template literals, and bracket
//! matching but does NOT build an AST or parse expressions/types.
//!
//! The plan has two distinct kinds of output:
//!
//! - **Recovered FACTS** — top-level (bracket depth 0) imports, macro calls
//!   (`defineProps`/`defineEmits`/`withDefaults`/`defineModel`/`defineExpose`/
//!   `defineOptions`/`defineSlots`, with the optional `const NAME = …` binding),
//!   variables, and functions. These carry ORIGINAL-source spans and feed hoisting
//!   and binding registration. Facts are gated to top level, mirroring the clean
//!   top-level parser (`block_depth == 0`), so block-local declarations never become
//!   setup bindings/imports.
//! - **OUTPUT-ONLY recovery chunks** — member/expression holes and scope closers
//!   (with empty-delimiter placeholders) detected over the WHOLE source regardless
//!   of nesting depth. These make the generated TSX parse for the language service
//!   and are NEVER turned into bindings, macros, imports, or any other source fact.

use verter_span::Span;

/// Known Vue macro kinds for the token scanner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveredMacroKind {
    DefineProps,
    DefineEmits,
    WithDefaults,
    DefineModel,
    DefineExpose,
    DefineOptions,
    DefineSlots,
}

/// A macro call recovered by the token scanner.
#[derive(Debug)]
pub struct RecoveredMacro<'a> {
    /// What kind of macro was found.
    pub kind: RecoveredMacroKind,
    /// Binding name from `const NAME = macroCall(...)`, if present.
    pub binding_name: Option<&'a str>,
    /// Span of the binding name identifier (used in tests; reserved for future hover annotation).
    #[allow(dead_code)]
    pub binding_span: Option<Span>,
    /// Span of the entire macro call expression (from identifier to closing
    /// paren/bracket). Exercised by the scanner tests; retained for span-precise
    /// recovery uses.
    #[allow(dead_code)]
    pub call_span: Span,
}

/// A function declaration recovered by the token scanner.
#[derive(Debug)]
pub struct RecoveredFunction<'a> {
    /// Function name.
    pub name: &'a str,
    /// Span of the function name identifier (used in tests; reserved for future hover annotation).
    #[allow(dead_code)]
    pub name_span: Span,
    /// Span of the parameter list (used in tests; reserved for future param annotation).
    #[allow(dead_code)]
    pub params_span: Span,
}

/// A variable declaration recovered by the token scanner.
#[derive(Debug)]
pub struct RecoveredVariable<'a> {
    /// Variable name.
    pub name: &'a str,
    /// Span of the variable name identifier (used in tests; reserved for future hover annotation).
    #[allow(dead_code)]
    pub name_span: Span,
    /// Declaration kind (`const`, `let`, or `var`).
    pub kind: RecoveredVarKind,
}

/// Variable declaration keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveredVarKind {
    Const,
    Let,
    Var,
}

/// An `import` statement recovered by the token scanner from the REAL source.
///
/// All spans are SFC-absolute (the `content_start` offset is baked in), exactly
/// like [`RecoveredMacro::call_span`], so the failure-path codegen can hoist the
/// import and rewrite its specifier without any reparse.
#[derive(Debug)]
pub struct RecoveredImport<'a> {
    /// Span of the full `import … '<source>'` statement (SFC-absolute), including
    /// a trailing `;` when one immediately follows. Suitable for `move_with_suffix`.
    pub span: Span,
    /// Module specifier text WITHOUT the surrounding quotes (e.g. `vue`, `./Foo.vue`).
    pub source: &'a str,
    /// Span of the source string literal INCLUDING quotes (SFC-absolute), used to
    /// rewrite `.vue` → `.vue.ts`.
    pub source_span: Span,
    /// Local binding names introduced by this import (default / namespace / named
    /// locals). Empty for side-effect imports (`import './x'`).
    pub binding_names: Vec<&'a str>,
    /// Whether the entire import is type-only (`import type … from …`).
    pub is_type_only: bool,
}

/// A synthetic, OUTPUT-ONLY insertion the recovery plan emits to make broken
/// `<script setup>` content parse as valid TSX. These chunks exist purely so the
/// TypeScript language service receives a recoverable program; they are NEVER
/// turned into bindings, macros, imports, or any other source fact, and they
/// carry no source-map mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryInsert {
    /// A dangling member access (`a.` / `a?.`): insert a member-name placeholder
    /// immediately after the operator so the dot cannot absorb the following token.
    MemberHole { at: u32 },
    /// A trailing operator / assignment RHS / conditional arm / arrow body: insert
    /// an operand placeholder so the expression is complete.
    ExpressionHole { at: u32 },
}

/// Structured, original-span tolerant recovery plan for a `<script setup>` block
/// whose content does NOT parse cleanly with OXC.
///
/// Every field is derived from a single token scan of the REAL source — there is
/// no synthesize-then-reparse step and no synthetic view is ever parsed. The
/// `inserts` and `scope_closers` describe OUTPUT-ONLY `CodeTransform` chunks; the
/// `imports` / `macros` / `functions` / `variables` carry original-span facts the
/// failure-path codegen reuses for hoisting and binding registration.
#[derive(Debug, Default)]
pub struct ScriptSetupRecoveryPlan<'a> {
    pub imports: Vec<RecoveredImport<'a>>,
    pub macros: Vec<RecoveredMacro<'a>>,
    pub functions: Vec<RecoveredFunction<'a>>,
    pub variables: Vec<RecoveredVariable<'a>>,
    /// OUTPUT-ONLY synthetic insertions (member / expression holes).
    pub inserts: Vec<RecoveryInsert>,
    /// Closers for brackets the user left open, innermost-first, appended at the
    /// recovery boundary (e.g. `})`). A delimiter that requires a non-empty body but
    /// was left empty carries a placeholder operand before its closer (`undefined)`,
    /// `undefined]`). OUTPUT-ONLY.
    pub scope_closers: String,
}

/// The kind of REQUIRED `(...)` header a control/condition keyword introduces.
/// Used to give the keyword's `(` the correct empty-body completion when the user
/// leaves it open. This is TYPED token state derived from the keyword token itself
/// — never a text/substring match on the surrounding source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlKind {
    /// `if` / `while` / `with` — a condition paren whose body is a statement; an
    /// empty discriminant is invalid (`if ()`), so it needs an `undefined` operand
    /// and the trailing `;` completes the (empty) statement body.
    Condition,
    /// `for` — a C-style (`for (a; b; c)`) or iterator (`for (x of y)`) header;
    /// completed by filling the MISSING `;` separators (none for an iterator).
    For,
    /// `switch` / `catch` — a header whose `)` MUST be followed by a block
    /// (`switch (x) {}`, `catch (e) {}`): needs an `undefined` operand if empty
    /// AND a trailing `{}`.
    Block,
}

/// Classification of the last significant token, used to decide whether the body
/// ends mid-expression (needing an expression hole) at EOF, and how to classify a
/// following open delimiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastTok {
    /// Nothing significant seen yet.
    None,
    /// A completed operand (identifier, literal, or a closing bracket).
    Operand,
    /// A member-access dot just consumed (handled via the pending-dot state).
    Dot,
    /// An operator / assignment / conditional arm / arrow that expects an operand.
    Operator,
    /// An opening bracket — closed by `scope_closers`, never an expression hole.
    Open,
    /// A keyword, `;`, or other token that needs no completion.
    Other,
    /// A control/condition keyword (`if`/`while`/`for`/`switch`/`catch`/`with`)
    /// whose following `(` is a REQUIRED header paren — distinct from an operand,
    /// so the `(` is NOT classified as empty-valid call arguments.
    ControlKeyword(ControlKind),
}

/// A bracket the user left open, tracked on the recovery scan's bracket stack so
/// the closer sequence can be emitted innermost-first AND so a delimiter that
/// requires a non-empty body (a grouping/arrow-body paren or a computed-member
/// bracket) can be given a placeholder operand when it is closed empty.
#[derive(Debug, Clone, Copy)]
struct BracketFrame {
    /// The matching close character (`)`, `]`, or `}`).
    close: u8,
    /// Whether closing this delimiter with an empty body would be INVALID TSX
    /// (`const x = ()`, `foo[]`, `() => ()`), determined from the token preceding
    /// the open delimiter. Ignored when `control` is set (the control kind drives
    /// completion instead). Call args, array literals, and blocks/objects are valid
    /// empty and never need a placeholder.
    needs_content: bool,
    /// Whether any significant token has been seen inside this delimiter since it
    /// was opened (a nested bracket counts as content for its parent).
    has_content: bool,
    /// Set when this is a control/condition-keyword header paren; drives the
    /// keyword-specific empty-body completion (condition discriminant, `for`
    /// separators, trailing `switch`/`catch` block) instead of `needs_content`.
    control: Option<ControlKind>,
    /// For a `for` header (`control == Some(ControlKind::For)`): the number of
    /// top-level `;` separators seen so far inside this paren.
    for_semis: u8,
    /// For a `for` header: whether a top-level `of`/`in` was seen (iterator form,
    /// `for (x of y)` / `for (k in o)`, which needs no `;` separators).
    for_iter: bool,
}

/// Lightweight token scanner for recovering structural info from broken scripts.
///
/// Single-pass, handles comments, strings, template literals, bracket matching.
/// Does NOT build an AST or parse expressions/types.
pub struct ScriptTokenScanner<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    /// SFC-absolute offset added to all span positions.
    content_start: u32,
}

const MACRO_NAMES: &[(&str, RecoveredMacroKind)] = &[
    ("defineProps", RecoveredMacroKind::DefineProps),
    ("defineEmits", RecoveredMacroKind::DefineEmits),
    ("withDefaults", RecoveredMacroKind::WithDefaults),
    ("defineModel", RecoveredMacroKind::DefineModel),
    ("defineExpose", RecoveredMacroKind::DefineExpose),
    ("defineOptions", RecoveredMacroKind::DefineOptions),
    ("defineSlots", RecoveredMacroKind::DefineSlots),
];

impl<'a> ScriptTokenScanner<'a> {
    pub fn new(source: &'a str, content_start: u32) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            content_start,
        }
    }

    /// Run the full structured recovery scan over the REAL source, producing a
    /// [`ScriptSetupRecoveryPlan`]: original-span import / macro / function /
    /// variable facts plus OUTPUT-ONLY member / expression holes and scope
    /// closers for a `<script setup>` whose content does not parse cleanly.
    ///
    /// Single pass; never builds an AST and never reparses a synthetic view.
    /// Member-access holes are detected structurally: a `.` / `?.` is dangling
    /// when EOF or a newline follows the operator before the property name, or
    /// when the next token cannot be a property name — this never false-positives
    /// on well-formed multi-line chains (`foo\n  .bar`), where the newline sits
    /// BEFORE the dot.
    ///
    /// Holes and scope closers are detected over the WHOLE source regardless of
    /// nesting depth, but recovered FACTS (imports / macros / variables / functions)
    /// are emitted ONLY at top level (bracket depth 0) — mirroring the clean
    /// top-level parser (`block_depth == 0`), so block-local declarations never
    /// become setup bindings/imports.
    pub fn recover_plan(mut self) -> ScriptSetupRecoveryPlan<'a> {
        let mut plan = ScriptSetupRecoveryPlan::default();
        // Stack of brackets the user left open (close char + content-requirement +
        // content-seen), used for scope closers and empty-delimiter placeholders.
        let mut bracket_stack: Vec<BracketFrame> = Vec::new();
        // SFC-absolute position just after a `.` / `?.` whose property is not yet seen.
        let mut pending_dot: Option<u32> = None;
        // Whether a newline has been crossed since `pending_dot` was set.
        let mut pending_dot_newline = false;
        let mut last_tok = LastTok::None;

        loop {
            let saw_newline = self.skip_trivia();
            if pending_dot.is_some() && saw_newline {
                pending_dot_newline = true;
            }
            if self.pos >= self.bytes.len() {
                break;
            }
            let b = self.bytes[self.pos];

            // Resolve a pending dangling-dot against the upcoming significant token.
            if let Some(at) = pending_dot {
                let next_is_property = is_ident_start(b) || b == b'#';
                if pending_dot_newline || !next_is_property {
                    plan.inserts.push(RecoveryInsert::MemberHole { at });
                    // The recovered `a.valueOf` IS a completed operand, so reset the
                    // token state from `Dot`: the current token begins a NEW statement
                    // (the dangling member ended the previous one). Without this, a
                    // dangling member followed by a control keyword (`a.\nif (`) would
                    // keep `last_tok == Dot`, the member-name guard below would
                    // suppress the keyword's required-header classification, and `if (`
                    // would close as empty call args → invalid `a.valueOf\nif ();`. A
                    // REAL same-line member (`p.catch(`) pushes NO hole, so its
                    // `last_tok == Dot` survives and the keyword stays a method name.
                    last_tok = LastTok::Operand;
                }
                pending_dot = None;
                pending_dot_newline = false;
                // Fall through and process the current token normally (it is either
                // the property name on the same line, or the token after the hole).
            }

            // Any non-closing token is content for the enclosing bracket. For an
            // OPEN bracket this marks the PARENT before the child frame is pushed;
            // for a CLOSE bracket we skip, so an empty pair stays empty. This drives
            // placeholder injection for delimiters that require a non-empty body.
            if !matches!(b, b')' | b']' | b'}') {
                if let Some(top) = bracket_stack.last_mut() {
                    top.has_content = true;
                }
            }

            // String / template literals are complete operands.
            if b == b'"' || b == b'\'' {
                self.skip_string(b);
                last_tok = LastTok::Operand;
                continue;
            }
            if b == b'`' {
                self.skip_template_literal();
                last_tok = LastTok::Operand;
                continue;
            }

            // Brackets — track balance for scope closers + content requirement.
            if b == b'(' || b == b'[' || b == b'{' {
                // Classify the delimiter from the TYPED preceding-token state so an
                // empty body recovers to VALID TSX:
                //   control/condition keyword + `(` → header paren (`if (…)`, `for
                //     (…;…;…)`, `switch (…) {}`), completed per its `ControlKind`;
                //   operand + `(`                  → call args, valid empty (`f()`);
                //   operator / start + `(`         → grouping / arrow body, needs an
                //     operand (`(undefined)`);
                //   operand + `[`                  → computed member, needs a key
                //     (`foo[undefined]`);
                //   operator / start + `[`         → array literal, valid empty (`[]`);
                //   `{`                            → block / object, valid empty (`{}`).
                let (close, needs_content, control) = match b {
                    b'(' => match last_tok {
                        LastTok::ControlKeyword(kind) => (b')', true, Some(kind)),
                        LastTok::Operand => (b')', false, None),
                        _ => (b')', true, None),
                    },
                    b'[' => (b']', last_tok == LastTok::Operand, None),
                    _ => (b'}', false, None),
                };
                bracket_stack.push(BracketFrame {
                    close,
                    needs_content,
                    has_content: false,
                    control,
                    for_semis: 0,
                    for_iter: false,
                });
                self.pos += 1;
                last_tok = LastTok::Open;
                continue;
            }
            if b == b')' || b == b']' || b == b'}' {
                if bracket_stack.last().map(|f| f.close) == Some(b) {
                    bracket_stack.pop();
                }
                self.pos += 1;
                last_tok = LastTok::Operand;
                continue;
            }

            // Member access / optional chain / spread.
            if b == b'.' {
                if self.looking_at(b"...") {
                    self.pos += 3;
                    last_tok = LastTok::Operator; // spread expects an operand
                    continue;
                }
                self.pos += 1;
                pending_dot = Some(self.content_start + self.pos as u32);
                pending_dot_newline = false;
                last_tok = LastTok::Dot;
                continue;
            }
            if b == b'?' {
                if self.looking_at(b"?.") {
                    self.pos += 2;
                    pending_dot = Some(self.content_start + self.pos as u32);
                    pending_dot_newline = false;
                    last_tok = LastTok::Dot;
                    continue;
                }
                // `??` (nullish) and `?` (ternary) both expect an operand next.
                self.pos += if self.looking_at(b"??") { 2 } else { 1 };
                last_tok = LastTok::Operator;
                continue;
            }

            // Identifiers / keywords — replicates the historical detection exactly,
            // plus `import` recovery. FACTS (imports / functions / variables / macros)
            // are recovered ONLY at top level (bracket depth 0); a keyword nested in a
            // block is treated as a plain identifier so the body keeps walking (its
            // holes/closers are still tracked) but no block-local binding is fabricated.
            if is_ident_start(b) {
                let at_top_level = bracket_stack.is_empty();
                let ident_start = self.pos;
                let ident = self.read_ident();

                if ident == "import" {
                    if at_top_level {
                        if let Some(imp) = self.try_recover_import(ident_start) {
                            plan.imports.push(imp);
                        }
                        last_tok = LastTok::Other;
                        continue;
                    }
                    // Nested `import` (e.g. a dynamic import inside a block) is not a
                    // top-level statement — fall through to plain-identifier handling.
                    last_tok = LastTok::Operand;
                    continue;
                }

                if ident == "function" {
                    if at_top_level {
                        if let Some(func) = self.try_recover_function() {
                            plan.functions.push(func);
                        }
                    }
                    last_tok = LastTok::Operand;
                    continue;
                }

                let var_kind = match ident {
                    "const" => Some(RecoveredVarKind::Const),
                    "let" => Some(RecoveredVarKind::Let),
                    "var" => Some(RecoveredVarKind::Var),
                    _ => None,
                };
                if let Some(kind) = var_kind {
                    // Top-level only, and at a word boundary (not part of a larger
                    // identifier). Block-local declarations are not setup bindings.
                    if at_top_level && !self.is_ident_at(self.pos) {
                        if let Some(var) = self.try_recover_variable(kind) {
                            plan.variables.push(var);
                        }
                    }
                    last_tok = LastTok::Other;
                    continue;
                }

                if at_top_level {
                    if let Some(&(_, kind)) = MACRO_NAMES.iter().find(|&&(name, _)| name == ident) {
                        if let Some(call_end) = self.try_match_macro_call() {
                            let call_span = Span::new(
                                self.content_start + ident_start as u32,
                                self.content_start + call_end as u32,
                            );
                            let (binding_name, binding_span) =
                                self.scan_backward_for_binding(ident_start);
                            plan.macros.push(RecoveredMacro {
                                kind,
                                binding_name,
                                binding_span,
                                call_span,
                            });
                            last_tok = LastTok::Operand;
                            continue;
                        }
                    }
                }

                // A keyword that follows a `.` / `?.` is a MEMBER NAME, not a
                // statement keyword (`promise.catch(`, `arr.for(`, `obj.if(` are
                // method calls), so skip control-keyword / iterator classification.
                if last_tok != LastTok::Dot {
                    // Control/condition keywords introduce a REQUIRED header paren;
                    // a following `(` must be treated as a condition/header paren
                    // (needs content) rather than empty-valid call arguments.
                    // Classified from the keyword token itself — no text match — and
                    // NOT gated to top level: the requirement holds at any depth.
                    if let Some(kind) = control_paren_kind(ident) {
                        last_tok = LastTok::ControlKeyword(kind);
                        continue;
                    }

                    // Inside a `for (` header, a top-level `of`/`in` marks the
                    // iterator form (`for (x of y)` / `for (k in o)`), which needs
                    // no `;` separators when the header is completed.
                    if (ident == "of" || ident == "in")
                        && matches!(
                            bracket_stack.last(),
                            Some(f) if f.control == Some(ControlKind::For)
                        )
                    {
                        if let Some(top) = bracket_stack.last_mut() {
                            top.for_iter = true;
                        }
                        last_tok = LastTok::Operand;
                        continue;
                    }
                }

                // Plain identifier or keyword — completes an expression position.
                last_tok = LastTok::Operand;
                continue;
            }

            // Operators / punctuation. Only the forms that genuinely expect a
            // following operand mark the body as ending mid-expression.
            match b {
                b'=' => {
                    if self.looking_at(b"=>") {
                        self.pos += 2; // arrow — expects a body
                    } else if self.looking_at(b"===") {
                        self.pos += 3;
                    } else if self.looking_at(b"==") {
                        self.pos += 2;
                    } else {
                        self.pos += 1; // assignment
                    }
                    last_tok = LastTok::Operator;
                }
                b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b':' | b',' => {
                    self.pos += 1;
                    last_tok = LastTok::Operator;
                }
                b';' => {
                    // A top-level `;` directly inside a `for (` header is a clause
                    // separator — count it so completion fills only the MISSING
                    // separators (`for (a; b;)` → one more, `for (a; b; c)` → none).
                    if let Some(top) = bracket_stack.last_mut() {
                        if top.control == Some(ControlKind::For) {
                            top.for_semis = top.for_semis.saturating_add(1);
                        }
                    }
                    self.pos += 1;
                    last_tok = LastTok::Other;
                }
                _ => {
                    // `<` `>` `!` etc.: no operand hole, no bracket tracking.
                    self.pos += 1;
                    last_tok = LastTok::Other;
                }
            }
        }

        // EOF: a still-pending dangling dot is a member hole.
        if let Some(at) = pending_dot {
            plan.inserts.push(RecoveryInsert::MemberHole { at });
        }
        // EOF: a trailing operator / assignment / conditional arm / arrow needs an operand.
        else if last_tok == LastTok::Operator {
            plan.inserts.push(RecoveryInsert::ExpressionHole {
                at: self.content_start + self.bytes.len() as u32,
            });
        }

        // Remaining open brackets → emit closers innermost-first, each completed to
        // VALID TSX. A grouping / computed-member delimiter left empty gets an
        // `undefined` operand (`const x = (undefined)`, `foo[undefined]`); a
        // control-keyword header is completed per its kind (`if (undefined);`,
        // `for (;;)`, `for (a; b;)`, `switch (undefined) {}`, `catch (e) {}`).
        while let Some(frame) = bracket_stack.pop() {
            match frame.control {
                Some(ControlKind::Condition) => {
                    if !frame.has_content {
                        plan.scope_closers.push_str("undefined");
                    }
                    plan.scope_closers.push(frame.close as char);
                }
                Some(ControlKind::For) => {
                    // Fill only the MISSING `;` separators: a C-style header needs
                    // two (`for (;;)`, `for (a; b;)`); an iterator header (`of`/`in`)
                    // needs none (`for (x of y)`).
                    if !(frame.for_iter && frame.for_semis == 0) {
                        for _ in 0..2u8.saturating_sub(frame.for_semis) {
                            plan.scope_closers.push(';');
                        }
                    }
                    plan.scope_closers.push(frame.close as char);
                }
                Some(ControlKind::Block) => {
                    if !frame.has_content {
                        plan.scope_closers.push_str("undefined");
                    }
                    plan.scope_closers.push(frame.close as char);
                    // A `switch`/`catch` header MUST be followed by a block.
                    plan.scope_closers.push_str(" {}");
                }
                None => {
                    if frame.needs_content && !frame.has_content {
                        plan.scope_closers.push_str("undefined");
                    }
                    plan.scope_closers.push(frame.close as char);
                }
            }
        }

        plan
    }

    // ── Helpers ──────────────────────────────────────────────────────

    fn looking_at(&self, needle: &[u8]) -> bool {
        self.bytes[self.pos..].starts_with(needle)
    }

    fn skip_line_comment(&mut self) {
        self.pos += 2; // skip //
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
            self.pos += 1;
        }
    }

    fn skip_block_comment(&mut self) {
        self.pos += 2; // skip /*
        while self.pos + 1 < self.bytes.len() {
            if self.bytes[self.pos] == b'*' && self.bytes[self.pos + 1] == b'/' {
                self.pos += 2;
                return;
            }
            self.pos += 1;
        }
        self.pos = self.bytes.len(); // unclosed
    }

    fn skip_string(&mut self, quote: u8) {
        self.pos += 1; // skip opening quote
        while self.pos < self.bytes.len() {
            if self.bytes[self.pos] == b'\\' {
                self.pos += 2; // skip escape
                continue;
            }
            if self.bytes[self.pos] == quote {
                self.pos += 1;
                return;
            }
            self.pos += 1;
        }
        // unclosed string — pos is at end
    }

    fn skip_template_literal(&mut self) {
        self.pos += 1; // skip opening backtick
        while self.pos < self.bytes.len() {
            if self.bytes[self.pos] == b'\\' {
                self.pos += 2;
                continue;
            }
            if self.bytes[self.pos] == b'`' {
                self.pos += 1;
                return;
            }
            if self.bytes[self.pos] == b'$'
                && self.pos + 1 < self.bytes.len()
                && self.bytes[self.pos + 1] == b'{'
            {
                self.pos += 2; // skip ${
                self.skip_bracket_matched(b'{', b'}');
                continue;
            }
            self.pos += 1;
        }
        // unclosed template literal
    }

    fn read_ident(&mut self) -> &'a str {
        let start = self.pos;
        while self.pos < self.bytes.len() && is_ident_continue(self.bytes[self.pos]) {
            self.pos += 1;
        }
        &self.source[start..self.pos]
    }

    /// Skip whitespace and comments, returning true if any were skipped.
    fn skip_ws_and_comments(&mut self) -> bool {
        let start = self.pos;
        loop {
            if self.pos >= self.bytes.len() {
                break;
            }
            if self.bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
                continue;
            }
            if self.looking_at(b"//") {
                self.skip_line_comment();
                continue;
            }
            if self.looking_at(b"/*") {
                self.skip_block_comment();
                continue;
            }
            break;
        }
        self.pos > start
    }

    /// Skip whitespace and comments, returning true if a newline was crossed.
    /// Used by the structured plan scan to detect dangling member-access dots
    /// (a `.` followed by a newline before its property name).
    fn skip_trivia(&mut self) -> bool {
        let mut saw_newline = false;
        loop {
            if self.pos >= self.bytes.len() {
                break;
            }
            let b = self.bytes[self.pos];
            if b.is_ascii_whitespace() {
                if b == b'\n' {
                    saw_newline = true;
                }
                self.pos += 1;
                continue;
            }
            if self.looking_at(b"//") {
                self.skip_line_comment();
                continue;
            }
            if self.looking_at(b"/*") {
                let before = self.pos;
                self.skip_block_comment();
                if self.source.as_bytes()[before..self.pos].contains(&b'\n') {
                    saw_newline = true;
                }
                continue;
            }
            break;
        }
        saw_newline
    }

    /// Whether the identifier at the current position is exactly `kw` (word-boundary checked).
    fn peek_ident_is(&self, kw: &str) -> bool {
        self.bytes[self.pos..].starts_with(kw.as_bytes())
            && self
                .bytes
                .get(self.pos + kw.len())
                .is_none_or(|&c| !is_ident_continue(c))
    }

    /// Try to match a macro call after the macro identifier has been consumed.
    /// Matches optional `<...>` type params followed by `(...)`.
    /// Returns the end position of the call if matched, None otherwise.
    fn try_match_macro_call(&mut self) -> Option<usize> {
        let saved = self.pos;
        self.skip_ws_and_comments();

        // Optional generic type params: <...>
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'<' {
            self.pos += 1;
            if !self.skip_bracket_matched(b'<', b'>') {
                // Unclosed generic — still try to match what we can
                self.pos = saved;
                self.skip_ws_and_comments();
            } else {
                self.skip_ws_and_comments();
            }
        }

        // Required call parens: (...)
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'(' {
            self.pos += 1;
            if self.skip_bracket_matched(b'(', b')') {
                return Some(self.pos);
            }
        }

        self.pos = saved;
        None
    }

    /// Skip forward matching brackets, handling nesting and string/comment skipping.
    /// Starts AFTER the opening bracket has been consumed.
    /// Returns true if the closing bracket was found.
    fn skip_bracket_matched(&mut self, open: u8, close: u8) -> bool {
        let mut depth = 1u32;
        while self.pos < self.bytes.len() && depth > 0 {
            let b = self.bytes[self.pos];

            // Skip strings
            if b == b'"' || b == b'\'' {
                self.skip_string(b);
                continue;
            }
            if b == b'`' {
                self.skip_template_literal();
                continue;
            }

            // Skip comments
            if self.looking_at(b"//") {
                self.skip_line_comment();
                continue;
            }
            if self.looking_at(b"/*") {
                self.skip_block_comment();
                continue;
            }

            if b == open {
                depth += 1;
            } else if b == close {
                depth -= 1;
                if depth == 0 {
                    self.pos += 1; // consume closing bracket
                    return true;
                }
            }
            self.pos += 1;
        }
        false // unclosed
    }

    /// Scan backward from `macro_start` looking for `const NAME =` or `let NAME =` pattern.
    fn scan_backward_for_binding(&self, macro_start: usize) -> (Option<&'a str>, Option<Span>) {
        // Walk backward skipping whitespace
        let mut p = macro_start;
        p = self.skip_back_ws(p);

        // Expect '='
        if p == 0 || self.bytes[p - 1] != b'=' {
            return (None, None);
        }
        p -= 1;
        p = self.skip_back_ws(p);

        // Read identifier backward
        if p == 0 || !is_ident_continue(self.bytes[p - 1]) {
            return (None, None);
        }
        let ident_end = p;
        while p > 0 && is_ident_continue(self.bytes[p - 1]) {
            p -= 1;
        }
        let ident_start = p;
        let name = &self.source[ident_start..ident_end];

        // Verify it's preceded by const/let/var (with word boundary check)
        p = self.skip_back_ws(p);
        let make_span = || {
            Span::new(
                self.content_start + ident_start as u32,
                self.content_start + ident_end as u32,
            )
        };

        // Helper: check the keyword doesn't have an ident char before it
        let is_word_boundary = |before_kw: usize| -> bool {
            before_kw == 0 || !is_ident_continue(self.bytes[before_kw - 1])
        };

        // `.get(..)` is char-boundary-safe: when `p` lands mid-codepoint (the
        // backward ident walk stops on a UTF-8 boundary because a multibyte byte
        // is not an ident char), the fixed-width slice returns `None` instead of
        // panicking, and the keyword simply does not match.
        if p >= 3 && self.source.get(p - 3..p) == Some("var") && is_word_boundary(p - 3) {
            return (Some(name), Some(make_span()));
        }
        if p >= 3 && self.source.get(p - 3..p) == Some("let") && is_word_boundary(p - 3) {
            return (Some(name), Some(make_span()));
        }
        if p >= 5 && self.source.get(p - 5..p) == Some("const") && is_word_boundary(p - 5) {
            return (Some(name), Some(make_span()));
        }

        (None, None)
    }

    fn skip_back_ws(&self, mut p: usize) -> usize {
        while p > 0 && self.bytes[p - 1].is_ascii_whitespace() {
            p -= 1;
        }
        p
    }

    /// Check if there's an identifier character at position `pos`.
    /// Returns false if pos is out of bounds.
    fn is_ident_at(&self, pos: usize) -> bool {
        // Handle underflow: if the subtraction would go below 0
        if pos >= self.bytes.len() {
            return false;
        }
        is_ident_continue(self.bytes[pos])
    }

    /// Try to recover a variable declaration after `const`/`let`/`var` keyword has been consumed.
    ///
    /// Reads the binding name identifier. Handles simple identifiers only (not destructuring).
    fn try_recover_variable(&mut self, kind: RecoveredVarKind) -> Option<RecoveredVariable<'a>> {
        self.skip_ws_and_comments();

        if self.pos >= self.bytes.len() || !is_ident_start(self.bytes[self.pos]) {
            return None;
        }
        let name_start = self.pos;
        let name = self.read_ident();
        let name_end = self.pos;

        Some(RecoveredVariable {
            name,
            name_span: Span::new(
                self.content_start + name_start as u32,
                self.content_start + name_end as u32,
            ),
            kind,
        })
    }

    /// Try to recover a function declaration after `function` keyword has been consumed.
    fn try_recover_function(&mut self) -> Option<RecoveredFunction<'a>> {
        self.skip_ws_and_comments();

        // Read function name
        if self.pos >= self.bytes.len() || !is_ident_start(self.bytes[self.pos]) {
            return None;
        }
        let name_start = self.pos;
        let name = self.read_ident();
        let name_end = self.pos;

        self.skip_ws_and_comments();

        // Optional type params <...>
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'<' {
            self.pos += 1;
            if !self.skip_bracket_matched(b'<', b'>') {
                return None; // unclosed generic
            }
            self.skip_ws_and_comments();
        }

        // Params (...)
        if self.pos >= self.bytes.len() || self.bytes[self.pos] != b'(' {
            return None;
        }
        let params_start = self.pos;
        self.pos += 1; // skip (
        if !self.skip_bracket_matched(b'(', b')') {
            return None; // unclosed params
        }
        let params_end = self.pos;

        Some(RecoveredFunction {
            name,
            name_span: Span::new(
                self.content_start + name_start as u32,
                self.content_start + name_end as u32,
            ),
            params_span: Span::new(
                self.content_start + params_start as u32,
                self.content_start + params_end as u32,
            ),
        })
    }

    /// Try to recover an `import` statement after the `import` keyword has been
    /// consumed (`self.pos` is just past `import`). Scans forward to the module
    /// source string literal, collecting local binding names along the way.
    ///
    /// Returns the structured import (SFC-absolute spans) on success and leaves
    /// `self.pos` past the statement. On failure (no module source before a `;`
    /// or EOF — a half-typed import) it RESTORES `self.pos` to just after the
    /// `import` keyword and returns `None`, so the surrounding scan continues
    /// without losing later tokens.
    fn try_recover_import(&mut self, import_kw_start: usize) -> Option<RecoveredImport<'a>> {
        let save = self.pos;
        let mut binding_names: Vec<&'a str> = Vec::new();
        let mut is_type_only = false;
        let mut depth: i32 = 0;

        // Optional leading `type` (whole-import type-only) — but NOT when `type`
        // is itself the default binding name (`import type from '…'`).
        self.skip_ws_and_comments();
        if self.peek_ident_is("type") {
            let save_t = self.pos;
            let _ = self.read_ident();
            self.skip_ws_and_comments();
            let next = self.bytes.get(self.pos).copied();
            let is_clause = matches!(next, Some(b'{') | Some(b'*'))
                || (next.is_some_and(is_ident_start) && !self.peek_ident_is("from"));
            if is_clause {
                is_type_only = true;
            } else {
                self.pos = save_t;
            }
        }

        loop {
            self.skip_ws_and_comments();
            let b = match self.bytes.get(self.pos).copied() {
                Some(b) => b,
                None => {
                    self.pos = save;
                    return None;
                }
            };
            match b {
                b'"' | b'\'' if depth == 0 => {
                    let src_start = self.pos;
                    self.skip_string(b);
                    let src_end = self.pos;
                    if src_end <= src_start + 1 {
                        self.pos = save;
                        return None;
                    }
                    let source = &self.source[src_start + 1..src_end - 1];
                    let mut end = src_end;
                    if self.bytes.get(end).copied() == Some(b';') {
                        end += 1;
                        self.pos = end;
                    }
                    return Some(RecoveredImport {
                        span: Span::new(
                            self.content_start + import_kw_start as u32,
                            self.content_start + end as u32,
                        ),
                        source,
                        source_span: Span::new(
                            self.content_start + src_start as u32,
                            self.content_start + src_end as u32,
                        ),
                        binding_names,
                        is_type_only,
                    });
                }
                b'{' | b'(' | b'[' => {
                    depth += 1;
                    self.pos += 1;
                }
                b'}' | b')' | b']' => {
                    depth -= 1;
                    self.pos += 1;
                }
                b';' if depth == 0 => {
                    self.pos = save;
                    return None;
                }
                _ if is_ident_start(b) => {
                    let name = self.read_ident();
                    match name {
                        "from" | "type" => {}
                        // `X as Y`: the previously collected `X` is the imported
                        // name, not the local — drop it; the next ident is local.
                        "as" => {
                            binding_names.pop();
                        }
                        _ => binding_names.push(name),
                    }
                }
                _ => {
                    self.pos += 1;
                }
            }
        }
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// Maps a control/condition keyword to the kind of REQUIRED `(...)` header it
/// introduces, or `None` for any other identifier/keyword. Token-level only — the
/// caller has already read the identifier; this never inspects raw source text.
fn control_paren_kind(ident: &str) -> Option<ControlKind> {
    match ident {
        "if" | "while" | "with" => Some(ControlKind::Condition),
        "for" => Some(ControlKind::For),
        "switch" | "catch" => Some(ControlKind::Block),
        _ => None,
    }
}

#[cfg(test)]
#[path = "script_recover_tests.rs"]
mod tests;
