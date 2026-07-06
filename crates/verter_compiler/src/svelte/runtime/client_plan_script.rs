//! The instance-SCRIPT lowering half of the Svelte client plan builder.
//!
//! Extracted from `client_plan.rs` (the file-size guard boundary): these are the
//! [`SupportedClientIr`] methods that lower the TYPED supported instance-script
//! items into the narrow [`ClientScriptItem`] component-FUNCTION-BODY statements
//! ([`build_script_items`](SupportedClientIr::build_script_items)), the
//! `$props()` destructure's single prop-source `$.prop` declaration
//! (`lower_props_destructure`), and each member DEFAULT's `$.prop` initial
//! argument with the official simple/lazy algorithm (`lower_props_default`).
//! Every lowering is a thin per-variant transform over the classifier's finite
//! allowlist; the FALLIBLE expression rewriter is the sole source-derived edit.

use verter_span::Span;

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_codegen_helpers::js_single_quoted;
use super::client_plan::{ClientScriptItem, SupportedClientIr};
use super::expr::ScopeId;
use super::expr_emit;
use super::expr_rewrite::PropRead;

impl<'a> SupportedClientIr<'a> {
    /// Lower the TYPED supported instance-script items into the narrow
    /// [`ClientScriptItem`] component-body statements, IN SOURCE ORDER.
    ///
    /// The instance script is the strict finite [`SupportedInstanceScriptItem`](super::instance_items::SupportedInstanceScriptItem)
    /// allowlist (minted by the default-deny classifier); the lowering is a thin
    /// per-variant transform over that enum — there is NO broad statement-rewrite path. A
    /// `<script module>` and an instance `import` / `export` were already refused at the
    /// classifier (the script-hoisting deferral), so this stage emits NO module-script
    /// imports / hoists; it produces ONLY the component-FUNCTION-BODY statements.
    ///
    /// Every variant except `FunctionDecl` is a rewriter-FREE transform
    /// ([`lower_simple_instance_item`](expr_emit::lower_simple_instance_item)). A
    /// `FunctionDecl` (a named function referenced by a DOM function-pair bind) lowers its
    /// BODY through the FALLIBLE expression rewriter ([`rewrite_source`](Self::rewrite_source))
    /// rooted at the instance-script scope — so a signal read/write inside the body
    /// becomes `$.get`/`$.set` (`function get() { return $.get(value); }`), NEVER verbatim.
    /// FALLIBLE: a function body using an unsupported form refuses.
    pub(super) fn build_script_items(
        &self,
    ) -> Result<(Vec<ClientScriptItem>, Option<String>), UnsupportedSvelteRuntimeSurface> {
        use super::instance_items::SupportedInstanceScriptItem as Item;
        use expr_emit::SimpleItemLowering;
        let root_scope = self.ir.root_scope().scope;
        let mut items = Vec::new();
        // The hoisted `$props.id()` declaration — the scan enforces the single-use
        // rule, so at most one item carries it.
        let mut props_id_hoist = None;
        for item in &self.script_items {
            // A `$props.id()` item yields the BODY-TOP hoist (always a `const`,
            // regardless of the source keyword — official emits `const` for a
            // `let` source declarator); its literal-only SIBLINGS flow through the
            // simple lowering below into the item's source slot.
            if let Item::PropsIdDecl { name, .. } = item {
                props_id_hoist = Some(format!("const {name} = $.props_id();"));
            }
            match expr_emit::lower_simple_instance_item(item) {
                SimpleItemLowering::Statement(code) => {
                    items.push(ClientScriptItem::BodyStatement { code });
                }
                SimpleItemLowering::None => {}
                SimpleItemLowering::NeedsRewriter => {
                    let code = match item {
                        // A `$state` / `$state.raw` declarator: its INIT routes through the
                        // shared rewriter FIRST (a signal read inside a proxiable object init
                        // becomes `$.get`, TS is stripped), THEN the resolved `StateLowering`
                        // wrapper (`$.state` / `$.proxy` / `$.state($.proxy(…))`) is applied.
                        // A no-arg `$state()` has no init to rewrite (the `void 0` form).
                        Item::StatePrimitive { name, init } => {
                            let rewritten = match init {
                                Some(src) => Some(self.rewrite_source(src, root_scope)?),
                                None => None,
                            };
                            expr_emit::lower_state_primitive_item(
                                name,
                                rewritten.as_deref(),
                                &self.ir.analysis.bindings,
                            )
                        }
                        // The `$props()` destructure: its PROP-SOURCE members lower
                        // to ONE `let <local> = $.prop($$props, <key>, <flags>[,
                        // <default>]), …;` declaration (default expressions rewrite
                        // through the shared rewriter); a destructure with no
                        // prop-source member emits nothing.
                        Item::PropsDestructure => {
                            match self.lower_props_destructure(root_scope)? {
                                Some(code) => code,
                                None => continue,
                            }
                        }
                        // A named function-pair function: its body lowers through the shared
                        // rewriter (signal reads/writes rewrite; the `function name(...) {}`
                        // structure is preserved). The rewriter wraps the source as an
                        // expression internally, so a declaration's source round-trips as a
                        // function expression with the body edits applied.
                        Item::FunctionDecl { source, .. } => {
                            self.rewrite_source(source, root_scope)?
                        }
                        // A `$store` SOURCE const: the INIT lowers through the shared
                        // rewriter (a store read/write inside rewrites — `derived(a,
                        // ($a) => $a * 2)` keeps its SHADOWED callback param verbatim);
                        // the `const <name> = ` scaffolding is composed around the
                        // rewritten payload.
                        Item::StoreSourceDecl { name, init } => {
                            let rewritten = self.rewrite_source(init, root_scope)?;
                            format!("const {name} = {rewritten};")
                        }
                        // A `$effect(fn);` / `$effect.pre(fn);` / bare `$effect.root(fn);`
                        // / `$effect.tracking();` statement: the whole call expression
                        // lowers through the shared rewriter in the STATEMENT role (the
                        // top-level call is the expression of a statement — the one
                        // official-legal position for the statement-only user-effect
                        // members). The callee → its registered helper, body signal
                        // reads → `$.get`, nested family calls recurse, an `await`
                        // inside the callback refuses as experimental-async; the
                        // carrier's transparent-wrapper head trivia re-emits inside
                        // the emitted helper call, and its carrier-TAIL trivia
                        // (wrapper interior and unwrapped trailing comments alike)
                        // after the rewritten payload, before the generated `;`.
                        Item::EffectStatement {
                            source,
                            head_trivia,
                            tail_trivia,
                        } => {
                            let rewritten =
                                self.rewrite_statement_source(source, head_trivia, root_scope)?;
                            let sep = if tail_trivia.is_empty() { "" } else { " " };
                            format!("{rewritten}{sep}{tail_trivia};")
                        }
                        // A `$effect.root(fn)` / `$effect.tracking()` declarator init: the
                        // INIT expression lowers through the shared rewriter (the callee →
                        // `$.effect_root` / `$.effect_tracking`, nested effects rewrite
                        // recursively, the carrier's wrapper-head trivia re-emits inside
                        // the emitted helper call, its carrier-TAIL trivia after the
                        // rewritten payload); the declaration keyword + binding
                        // name are typed classification facts composed around the
                        // rewritten payload.
                        Item::EffectRuneInit {
                            const_decl,
                            name,
                            init,
                            head_trivia,
                            tail_trivia,
                        } => {
                            let kw = if *const_decl { "const" } else { "let" };
                            let rewritten =
                                self.rewrite_rune_init_source(init, head_trivia, root_scope)?;
                            let sep = if tail_trivia.is_empty() { "" } else { " " };
                            format!("{kw} {name} = {rewritten}{sep}{tail_trivia};")
                        }
                        // `NeedsRewriter` is produced ONLY for the arms above; any other
                        // item reaching here is a classifier/lowering divergence.
                        _ => unreachable!(
                            "only StatePrimitive, PropsDestructure, FunctionDecl, StoreSourceDecl, EffectStatement, and EffectRuneInit need the rewriter"
                        ),
                    };
                    items.push(ClientScriptItem::BodyStatement { code });
                }
            }
        }
        Ok((items, props_id_hoist))
    }

    /// Lower the instance script's `$props()` destructure into its ONE
    /// prop-source declaration statement, or `None` when no member is a prop
    /// source (every prop reads directly off `$$props`).
    ///
    /// Per PROP-SOURCE member (the official `is_prop_source`: a default initial
    /// OR a written local), the declarator is
    /// `<local> = $.prop($$props, '<source-key>', <flags>[, <default>])` with
    /// the official flag algorithm on Verter's runes-only surface:
    /// `IMMUTABLE(1) | RUNES(2)` always, `UPDATED(4)` when the local is written
    /// (reassigned or deep-mutated), `BINDABLE(8)` for a `$bindable(...)`
    /// default, `LAZY_INITIAL(16)` when the default rides a thunk / collapsed
    /// callee. Declarators join ONE `let` declaration in source order (the
    /// official single-declaration shape). FALLIBLE: a default whose expression
    /// refuses the shared rewriter (or a TS-wrapped default) fails closed.
    fn lower_props_destructure(
        &self,
        root_scope: ScopeId,
    ) -> Result<Option<String>, UnsupportedSvelteRuntimeSurface> {
        let Some(instance) = self.ir.analysis.scripts.instance_source else {
            return Ok(None);
        };
        let mut decls = Vec::new();
        // The named prop-source members, off the UNIFIED declarator plan (no
        // re-scan). `is_prop_source` / the `UPDATED` flag consult the shared
        // `prop_updated` set the same plan's default spans helped harvest. A
        // CUSTOM ELEMENT compiles with `accessors` (the official
        // `is_custom_element` force): EVERY member is then a prop source and
        // carries the `UPDATED` flag (the official `analysis.accessors` arm of
        // `get_prop_source`), so the `$$exports` setters have a live setter and
        // external prop writes propagate.
        let ce_accessors = self.ir.component.custom_element.is_some();
        for member in self.decl_plan.iter().flat_map(|plan| plan.members.iter()) {
            if !member.is_prop_source(&self.prop_updated, ce_accessors) {
                continue;
            }
            // IMMUTABLE | RUNES — always set on Verter's runes-only surface.
            let mut flags = 3u8;
            if ce_accessors || self.prop_updated.contains(&member.local) {
                flags |= 4;
            }
            if member.bindable {
                flags |= 8;
            }
            let arg = match &member.default {
                None => None,
                Some(facts) => {
                    Some(self.lower_props_default(instance, member, facts, &mut flags, root_scope)?)
                }
            };
            let key = js_single_quoted(&member.source_key);
            let call = match arg {
                Some(arg) => format!("$.prop($$props, {key}, {flags}, {arg})"),
                None => format!("$.prop($$props, {key}, {flags})"),
            };
            decls.push(format!("{} = {call}", member.local));
        }
        // The `$.rest_props` capture declarator, at the rest pattern's SOURCE
        // position (LAST, after the named prop-source decls): `<local> =
        // $.rest_props($$props, <rest_excludes>)`. For a whole-object capture
        // (`let all = $props()`) there are NO named prop-source decls, so this is
        // the sole declarator (`let all = $.rest_props(…)`).
        if let Some(rest) = &self.rest_props {
            decls.push(format!(
                "{} = $.rest_props($$props, {})",
                rest.local, rest.set_name
            ));
        }
        if decls.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!("let {};", decls.join(", "))))
    }

    /// Lower ONE `$props()` member DEFAULT into its `$.prop` initial argument,
    /// setting `LAZY_INITIAL` (bit 16) when the carrier is a thunk / collapsed
    /// callee — the official `get_prop_source` initial algorithm over the
    /// REWRITTEN expression:
    ///
    /// - a BINDABLE default that `should_proxy` → `() => $.proxy(<rewritten>)`
    ///   (the proxy wrap is bindable-only and always rides the thunk; a
    ///   sequence root parenthesizes the argument — `$.proxy((1, 2))` — so the
    ///   comma expression stays ONE proxy argument);
    /// - a VISITED-simple expression → RAW (no lazy bit): the simple skeleton
    ///   holds AND every identifier leaf stays unrewritten — a function
    ///   literal passes raw even when its BODY carries rewrites (`() =>
    ///   (a = 1)` → `() => (a(1))`, official flags 7), because official runs
    ///   `is_simple_expression` on the initializer AFTER visiting and a body
    ///   rewrite never changes the outer node kind;
    /// - an unrewritten NON-optional zero-arg identifier call → the BARE callee;
    /// - a bare identifier that rewrites to a sibling GETTER call → the BARE
    ///   getter (the same zero-arg collapse over the rewritten node);
    /// - everything else → `() => <rewritten>` (an object or sequence root
    ///   parenthesizes the body: `() => ({ … })` / `() => (1, 2)`).
    fn lower_props_default(
        &self,
        instance: &str,
        plan: &expr_emit::PropsMemberPlan,
        facts: &expr_emit::PropsDefaultFacts,
        flags: &mut u8,
        root_scope: ScopeId,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        if facts.ts_wrapped {
            // A TS-wrapped default is a distinct surface (the official
            // simple/lazy decision runs over the TS node) — fail closed, the
            // same boundary as the `$state()` ts-wrapped init.
            return Err(UnsupportedSvelteRuntimeSurface::AdvancedRune {
                rune: "$props() ts-wrapped default",
                span: Span::new(facts.span.0, facts.span.1),
            });
        }
        let src = instance
            .get(facts.span.0 as usize..facts.span.1 as usize)
            .unwrap_or_default();
        let rewritten = self.rewrite_source(src, root_scope)?;
        let identity = rewritten == src;
        if plan.bindable {
            // The official `should_proxy` runs over the REWRITTEN node: a bare
            // identifier whose read rewrites (a signal / prop getter / props
            // member) becomes a call/member — always proxiable; an unrewritten
            // root keeps the shape-based decision (with the one-hop follow).
            let proxiable = match &facts.bare_ident {
                Some(_) if !identity => true,
                _ => facts.proxiable_by_shape,
            };
            if proxiable {
                *flags |= 16;
                // A sequence root parenthesizes so the comma expression stays
                // ONE `$.proxy` argument.
                if facts.sequence_root {
                    return Ok(format!("() => $.proxy(({rewritten}))"));
                }
                return Ok(format!("() => $.proxy({rewritten})"));
            }
        }
        // The official `is_simple_expression` runs over the VISITED
        // initializer: the skeleton fact carries the identifier leaves, and a
        // leaf rewrites iff the shared rewriter changes its bare text (getter
        // call / `$$props` member / signal read). Rewrites inside a function
        // literal's body never break simplicity — the outer node kind is what
        // the official predicate sees.
        let visited_simple = match &facts.simple_ident_leaves {
            None => false,
            // The whole default text is unrewritten — every leaf trivially is.
            Some(_) if identity => true,
            Some(leaves) => {
                let mut all_unrewritten = true;
                for leaf in leaves {
                    if self.rewrite_source(leaf, root_scope)? != *leaf {
                        all_unrewritten = false;
                        break;
                    }
                }
                all_unrewritten
            }
        };
        if visited_simple {
            return Ok(rewritten);
        }
        *flags |= 16;
        if identity {
            if let Some(callee) = &facts.zero_arg_ident_callee {
                return Ok(callee.clone());
            }
        }
        if let Some(name) = &facts.bare_ident {
            // A bare identifier that rewrote to the sibling GETTER call
            // (`{ b = a }` → `a()`) collapses to the bare getter — the official
            // zero-arg-callee optimization over the rewritten node. (A rewrite
            // to `$$props.x` / `$.get(x)` is not a zero-arg identifier call and
            // rides the thunk below.)
            if !identity && matches!(self.prop_reads.get(name.as_str()), Some(PropRead::Getter)) {
                return Ok(name.clone());
            }
        }
        if facts.object_root || facts.sequence_root {
            // An object body needs the arrow-body parenthesization; a sequence
            // body needs it so the comma expression stays ONE thunk return
            // value (`() => (1, 2)`), never a stray call argument.
            return Ok(format!("() => ({rewritten})"));
        }
        Ok(format!("() => {rewritten}"))
    }
}
