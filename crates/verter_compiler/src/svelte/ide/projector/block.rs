//! The Svelte IDE block-construct projection (`{#if}` / `{#each}` / `{#await}` /
//! `{#key}` / `{#snippet}`).
//!
//! Each block lowers to valid TSX (a ternary, a `.map(...)`, an awaited-state
//! IIFE, a void-checked comma, or a hoisted branded snippet declarator), with
//! block-head store-subs rewritten and markup BLOCK BINDINGS (`as $item`, `then
//! $v`, `catch $e`, snippet params) scoped to their subtree so a `$`-named
//! binding is treated as a local, not a store auto-subscription. This module is a
//! continuation of [`super`]'s `TemplateProjector` impl — extracted for file
//! size; it reaches the parent's private projector type and helpers via
//! `use super::*`.

use super::*;

impl TemplateProjector<'_, '_> {
    /// Project a block construct.
    pub(super) fn project_block(&mut self, block: &SvelteBlock) {
        match &block.kind {
            SvelteBlockKind::If => self.project_if(block),
            SvelteBlockKind::Each { item, index, key } => {
                self.project_each(block, *item, *index, *key)
            }
            SvelteBlockKind::Await {
                then_binding,
                catch_binding,
            } => self.project_await(block, *then_binding, *catch_binding),
            SvelteBlockKind::Key => self.project_key(block),
            SvelteBlockKind::Snippet {
                name_text, params, ..
            } => self.project_snippet(block, name_text, *params),
        }
    }

    /// `{#if c}A{:else if d}B{:else}C{/if}` → `{c ? (<>A</>) : d ? (<>B</>) : (<>C</>)}`.
    fn project_if(&mut self, block: &SvelteBlock) {
        let Some(head) = block.head_expr else { return };
        // F11: a store-sub in the `{#if $store}` condition is rewritten.
        self.rewrite_store_subs_in(head);
        // Overwrite `{#if ` (from block start to head start) with `{`.
        self.ct.overwrite(block.span.start, head.start, "{");
        // After the condition: `}` (the close of `{#if c}`) becomes ` ? (<>`.
        // The body run follows. We overwrite the single `}` after head.
        let after_head = head.end;
        // Find the `}` closing the if-open.
        let close = self.find_char_after(after_head, '}');
        if let Some(close_idx) = close {
            self.ct.overwrite(after_head, close_idx + 1, " ? (<>");
        }
        // Project children (the true branch body).
        for child in &block.children {
            self.project_node(child);
        }
        // Handle clauses.
        self.project_if_clauses(block);
        // Close the whole block: overwrite `{/if}` with `</>)}`.
        let end_tag_start = self.find_str_before(block.span.end, "{/if}");
        if let Some(s) = end_tag_start {
            self.ct.overwrite(s, block.span.end, "</>)}");
        }
    }

    fn project_if_clauses(&mut self, block: &SvelteBlock) {
        for clause in &block.clauses {
            match clause.kind {
                SvelteClauseKind::ElseIf => {
                    // `{:else if d}` → `</>) : d ? (<>`. The clause-tag head span
                    // (`{:else if ` through the condition start) is rewritten
                    // from the parser-provided `tag_span`, and the closing `}`
                    // (tag_span.end-1..end) re-opens the body fragment.
                    if let Some(expr) = clause.expr {
                        // F11: a store-sub in the `{:else if $store}` condition.
                        self.rewrite_store_subs_in(expr);
                        self.ct
                            .overwrite(clause.tag_span.start, expr.start, "</>) : ");
                        self.ct.overwrite(expr.end, clause.tag_span.end, " ? (<>");
                    } else {
                        // A malformed `{:else if}` with no condition — rewrite the
                        // whole tag to a falsy ternary arm so no raw `{:…}` leaks.
                        self.ct.overwrite(
                            clause.tag_span.start,
                            clause.tag_span.end,
                            "</>) : false ? (<>",
                        );
                    }
                }
                SvelteClauseKind::Else => {
                    // `{:else}` → `</>) : (<>` — overwrite the WHOLE clause-tag
                    // span (braces included). An empty `{:else}` (no expr, no
                    // children) is still rewritten — the `tag_span` is always
                    // present, so no raw `{:else}` leaks (P1-1).
                    self.ct
                        .overwrite(clause.tag_span.start, clause.tag_span.end, "</>) : (<>");
                }
                _ => {}
            }
            for child in &clause.children {
                self.project_node(child);
            }
        }
    }

    /// `{#each xs as x, i (key)}BODY{/each}` → `{xs.map((x, i) => (<>BODY</>))}`.
    fn project_each(
        &mut self,
        block: &SvelteBlock,
        item: Option<Span>,
        index: Option<Span>,
        _key: Option<Span>,
    ) {
        let Some(head) = block.head_expr else { return };
        // F11: a store-sub in the `{#each $store as x}` iterable is rewritten.
        self.rewrite_store_subs_in(head);
        // `{#each ` → `{`
        self.ct.overwrite(block.span.start, head.start, "{");
        // After the list expression, build `.map((item, index) => (<>`.
        // The original ` as x, i (key)}` run (from head.end to the open `}`)
        // is overwritten. The each-open's closing `}` is AFTER every binding
        // span — a DESTRUCTURING `as { x }` / `as [a]` pattern contains its OWN
        // `}`/`]`, so the search must START past the last binding span (item /
        // index / key), else it stops at the pattern's inner `}` and strands the
        // tail (`as { x = $f }` → a malformed `(<>}` projection).
        let search_from = [item, index, _key]
            .into_iter()
            .flatten()
            .map(|s| s.end)
            .chain(std::iter::once(head.end))
            .max()
            .unwrap_or(head.end);
        let open_close = self.find_char_after(search_from, '}');
        if let Some(close_idx) = open_close {
            // The item / index PATTERN text is sliced into the synthesised arrow
            // head, so a store-READ DEFAULT inside it (`as { x = $store }`) is
            // rewritten on the TEXT (`__verter_store_get(store)`) — the bound
            // names stay locals.
            let params = match (item, index) {
                (Some(it), Some(ix)) => format!(
                    "{}, {}",
                    self.rewrite_pattern_text_defaults(self.slice(it)),
                    self.rewrite_pattern_text_defaults(self.slice(ix))
                ),
                (Some(it), None) => self.rewrite_pattern_text_defaults(self.slice(it)),
                (None, _) => "__verter_item".to_string(),
            };
            self.ct
                .overwrite(head.end, close_idx + 1, &format!(".map(({params}) => (<>"));
        }
        // The `as PATTERN, INDEX` bindings (incl. destructured `as {a,b}` /
        // `as [a,b]`) scope to the each BODY — push their `$`-names so a
        // `$`-named item/index binding is NOT mis-rewritten as a store-sub inside
        // the body. The else body sees NO item binding (each-else
        // runs on an empty list), so the frame is popped before the else clause.
        self.push_block_bindings(&[item, index]);
        for child in &block.children {
            self.project_node(child);
        }
        self.pop_block_bindings();
        // `{:else}` (each-else): close the `.map(...)` items expression and open
        // a SEPARATE sibling `{false && (<>ELSE</>)}` — the else body's
        // expressions stay type-checked (and mapped) but render nothing. This
        // is valid TSX (two sibling JSX expressions), unlike a patched `.map`
        // close.
        let has_else = block
            .clauses
            .iter()
            .any(|c| c.kind == SvelteClauseKind::Else);
        for clause in &block.clauses {
            if clause.kind == SvelteClauseKind::Else {
                // Overwrite the WHOLE `{:else}` clause-tag span (braces
                // included) — an empty each-else still rewrites cleanly (P1-1).
                self.ct.overwrite(
                    clause.tag_span.start,
                    clause.tag_span.end,
                    "</>))}\n{false && (<>",
                );
                for child in &clause.children {
                    self.project_node(child);
                }
            }
        }
        // `{/each}` → close the (items map) OR the (else sibling fragment).
        if let Some(s) = self.find_str_before(block.span.end, "{/each}") {
            if has_else {
                // The else sibling fragment closes with `</>)}`.
                self.ct.overwrite(s, block.span.end, "</>)}");
            } else {
                self.ct.overwrite(s, block.span.end, "</>))}");
            }
        }
    }

    /// `{#await p}P{:then v}T{:catch e}C{/await}` → ternary over a synthetic
    /// promise-state holder. v1: await-expressions are out of scope (D-bg) only
    /// for the EXPRESSION position; the `{#await}` BLOCK itself projects.
    fn project_await(
        &mut self,
        block: &SvelteBlock,
        then_binding: Option<Span>,
        catch_binding: Option<Span>,
    ) {
        let Some(head) = block.head_expr else { return };
        // F11: a store-sub in the `{#await $promise}` head is rewritten.
        self.rewrite_store_subs_in(head);
        // Synthetic holder: `{((__verter_await) => __verter_await.pending ? (<>P</>) : __verter_await.error ? (<>C</>) : (<>T</>))(__verter_state(PROMISE))}`
        // For a tractable, type-clean projection: resolve the promise value
        // type via `Awaited<typeof PROMISE>` and bind it.
        self.ct.overwrite(
            block.span.start,
            head.start,
            "{((__verter_p) => { type __VA = Awaited<typeof __verter_p>; ",
        );
        let open_close = self.find_char_after(head.end, '}');
        if let Some(c) = open_close {
            self.ct.overwrite(head.end, c + 1, "; return (<>");
        }
        // In the INLINE forms `{#await p then $v}` / `{#await p catch $e}` the
        // block has no separate `{:then}`/`{:catch}` clause — `block.children` IS
        // the then-/catch-body, and the binding lives on the block-level
        // `then_binding`/`catch_binding` span. Push the applicable inline binding's
        // `$`-names so a `$`-named inline binding is not mis-rewritten as a
        // store-sub inside that body. (The full clause forms scope
        // their binding per-clause below.)
        let has_then_clause = block
            .clauses
            .iter()
            .any(|c| c.kind == SvelteClauseKind::Then);
        let has_catch_clause = block
            .clauses
            .iter()
            .any(|c| c.kind == SvelteClauseKind::Catch);
        let inline_body_binding = if !has_then_clause && then_binding.is_some() {
            then_binding // `{#await p then $v}` — children are the then-body
        } else if !has_then_clause && !has_catch_clause && catch_binding.is_some() {
            catch_binding // `{#await p catch $e}` — children are the catch-body
        } else {
            None
        };
        self.push_block_bindings(&[inline_body_binding]);
        for child in &block.children {
            self.project_node(child);
        }
        self.pop_block_bindings();
        // Project clauses (`:then`, `:catch`) as fragment continuations.
        for clause in &block.clauses {
            match clause.kind {
                SvelteClauseKind::Then => {
                    // Bind the value as a const of the awaited type. Overwrite the
                    // WHOLE `{:then v}` clause-tag span — an empty `{:then}` (no
                    // binding) still rewrites cleanly with a synthetic name (P1-1).
                    let binding = clause
                        .expr
                        .map(|sp| self.rewrite_pattern_text_defaults(self.slice(sp)))
                        .unwrap_or_else(|| "__verter_v".to_string());
                    self.ct.overwrite(
                        clause.tag_span.start,
                        clause.tag_span.end,
                        &format!("</>); const {binding}: __VA = (null as any); return (<>"),
                    );
                    // The `{:then PATTERN}` binding scopes to the THEN body — push
                    // its `$`-names so a `$`-named then binding is not mis-rewritten
                    // as a store-sub inside the body.
                    self.push_block_bindings(&[clause.expr]);
                    for child in &clause.children {
                        self.project_node(child);
                    }
                    self.pop_block_bindings();
                }
                SvelteClauseKind::Catch => {
                    // Declare the catch binding (`{:catch e}` → a typed
                    // `const e: unknown`) so the catch body's `{e}` resolves.
                    // Overwrite the WHOLE `{:catch e}` clause-tag span — an empty
                    // `{:catch}` still rewrites cleanly (P1-1).
                    let binding = clause
                        .expr
                        .map(|sp| self.rewrite_pattern_text_defaults(self.slice(sp)))
                        .unwrap_or_else(|| "__verter_e".to_string());
                    self.ct.overwrite(
                        clause.tag_span.start,
                        clause.tag_span.end,
                        &format!("</>); const {binding}: unknown = (null as any); return (<>"),
                    );
                    // The `{:catch PATTERN}` binding scopes to the CATCH body —
                    // push its `$`-names.
                    self.push_block_bindings(&[clause.expr]);
                    for child in &clause.children {
                        self.project_node(child);
                    }
                    self.pop_block_bindings();
                }
                _ => {}
            }
        }
        if let Some(s) = self.find_str_before(block.span.end, "{/await}") {
            self.ct
                .overwrite(s, block.span.end, "</>); })(null as any)}");
        }
    }

    /// `{#key e}BODY{/key}` → `{(__verter_void(e), <>BODY</>)}` — the key
    /// expression `e` stays mapped and type-checked (the comma operator's left
    /// operand), the body renders. Valid TSX, no IIFE arity mismatch.
    fn project_key(&mut self, block: &SvelteBlock) {
        let Some(head) = block.head_expr else { return };
        // F11: a store-sub in the `{#key $store}` expression is rewritten.
        self.rewrite_store_subs_in(head);
        // `{#key ` → `{(__verter_void(` — the head `e` stays in place, mapped.
        self.ct
            .overwrite(block.span.start, head.start, "{(__verter_void(");
        // The `}` closing `{#key e}` → `), <>` (close the void call, comma, open
        // the body fragment).
        if let Some(c) = self.find_char_after(head.end, '}') {
            self.ct.overwrite(head.end, c + 1, "), <>");
        }
        for child in &block.children {
            self.project_node(child);
        }
        // `{/key}` → `</>)}`
        if let Some(s) = self.find_str_before(block.span.end, "{/key}") {
            self.ct.overwrite(s, block.span.end, "</>)}");
        }
    }

    /// `{#snippet name(params)}BODY{/snippet}` → hoisted branded declarator.
    fn project_snippet(&mut self, block: &SvelteBlock, name: &str, params: Option<Span>) {
        // Compute the body span: from the end of the `{#snippet ...}` head to
        // the start of `{/snippet}`.
        let head_close = self
            .find_char_after(block.span.start, '}')
            .map(|c| c + 1)
            .unwrap_or(block.span.start);
        let end_tag = self
            .find_str_before(block.span.end, "{/snippet}")
            .unwrap_or(block.span.end);
        let body_span = Span::new(head_close, end_tag);
        // Remove the original `{#snippet ...}` head and `{/snippet}` tail in
        // place (the body is MOVED out, so its source bytes are relocated).
        self.ct.remove(block.span.start, head_close);
        self.ct.remove(end_tag, block.span.end);
        // The snippet PARAMS scope to the snippet body — push their `$`-names so
        // a `$`-named snippet param is not mis-rewritten as a store-sub inside the
        // body. Project the body's children before moving (transforms
        // apply to the moved bytes).
        self.push_block_bindings(&[params]);
        for child in &block.children {
            self.project_node(child);
        }
        self.pop_block_bindings();
        self.snippet_moves.push(SnippetMove {
            block_span: block.span,
            name: name.to_string(),
            params,
            body_span,
        });
    }
}
