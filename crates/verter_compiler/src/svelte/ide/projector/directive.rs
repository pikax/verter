//! Attribute and directive projection for Svelte IDE output.

use super::*;

impl TemplateProjector<'_, '_> {
    /// Project the attributes of an element (events verbatim-lowercase, class
    /// object/array via the checker, CSS custom props stripped, bindings,
    /// directives).
    pub(super) fn project_attribute(&mut self, el: &SvelteElement, attr: &SvelteAttribute) {
        match &attr.kind {
            SvelteAttributeKind::Plain {
                name,
                name_span,
                value,
            } => {
                // An empty-name plain attribute carrying an inline-tag inner
                // (`{@attach expr}` / a brace comment used in attribute
                // position) — dispatch on the leading sigil.
                if name.is_empty() {
                    if let Some(SvelteAttributeValue::Expression(inner)) = value {
                        self.project_inline_tag_attribute(attr, *inner);
                        return;
                    }
                }
                if is_css_custom_property(name) {
                    // CSS custom property `--x={expr}`: strip the attribute,
                    // void-check the value.
                    self.strip_custom_property_attr(attr, value.as_ref());
                    return;
                }
                // Attribute-value shorthand `<input {value} />`: the parser
                // sets name == the inner expression text and the attribute span
                // opens with `{`. A bare `{value}` is INVALID in a JSX opening
                // tag — rewrite it to `value={value}` by inserting `name=`
                // before the `{` (the `{value}` expression stays mapped).
                if self.source.as_bytes().get(attr.span.start as usize) == Some(&b'{') {
                    self.ct.prepend_left(attr.span.start, &format!("{name}="));
                    // A `{$store}` shorthand still rewrites the store-sub interior.
                    if let Some(SvelteAttributeValue::Expression(expr)) = value {
                        self.rewrite_store_subs_in(*expr);
                    }
                    let _ = name_span;
                    return;
                }
                // Plain attribute / lowercase event attribute: kept verbatim.
                // `onclick={fn}` stays `onclick` typed by SvelteHTMLElements — but
                // a store-sub in the value expression (`prop={$store}`) is rewritten.
                if let Some(SvelteAttributeValue::Expression(expr)) = value {
                    self.rewrite_store_subs_in(*expr);
                }
                let _ = name_span;
            }
            SvelteAttributeKind::Spread(span) => {
                // `{...rest}` is valid JSX spread — kept. A store-sub in the spread
                // expression (`{...$attrs}`) is rewritten. The recorded span covers
                // the leading `...` (a `...$attrs` does not parse as a standalone
                // expression), so scan only the expression AFTER the `...`.
                let inner = &self.source[span.start as usize..span.end as usize];
                if let Some(rest_off) = inner.find("...").map(|i| i + 3) {
                    let expr_start = span.start + rest_off as u32;
                    self.rewrite_store_subs_in(Span::new(expr_start, span.end));
                }
            }
            SvelteAttributeKind::Directive(dir) => {
                self.project_directive(el, attr, dir);
            }
            // An attribute-position `{@attach expr}` — element-attachment machinery
            // with NO published prop. Projected to a JSX spread that void-checks the
            // attachment expression through `__verter_attach` while contributing no
            // props: `{...(__verter_attach(expr), {})}`. The expression span was
            // captured by the tokenizer (no body re-slicing here).
            SvelteAttributeKind::Attach { expr_span } => {
                // F11: a store-sub in the attachment expression (`{@attach $a}`).
                self.rewrite_store_subs_in(*expr_span);
                // `{@attach ` → `{...(__verter_attach(`
                self.ct
                    .overwrite(attr.span.start, expr_span.start, "{...(__verter_attach(");
                // closing `}` → `), {})}`
                self.ct.overwrite(expr_span.end, attr.span.end, "), {})}");
            }
        }
    }

    /// Project a NON-attach inline-tag attribute (a brace comment or unrecognised
    /// inline tag used in an element open tag) — strip it (no type surface). The
    /// `{@attach}` form is the typed [`SvelteAttributeKind::Attach`] and never
    /// reaches here.
    fn project_inline_tag_attribute(&mut self, attr: &SvelteAttribute, inner: Span) {
        let _ = inner;
        remove_span(self.ct, attr.span);
    }

    /// Strip a CSS custom-property attribute (`--x={expr}`) from the JSX
    /// position and void-check its value. A `--`-prefixed name is not a
    /// valid JSX attribute identifier, so the WHOLE `--name=` attribute name is
    /// removed; the `{expr}` value is rewritten into a JSX spread that
    /// void-checks the expression while contributing NO props:
    /// `{...(__verter_void(expr), {})}`. The expression bytes stay mapped.
    fn strip_custom_property_attr(
        &mut self,
        attr: &SvelteAttribute,
        value: Option<&SvelteAttributeValue>,
    ) {
        if let Some(SvelteAttributeValue::Expression(expr)) = value {
            // F11: a store-sub in the void-checked CSS-custom-property value.
            self.rewrite_store_subs_in(*expr);
            // `--name={` → `{...(__verter_void(` ; the trailing `}` → `), {})}`.
            // The whole prefix (attribute start through the expression start)
            // becomes the spread opener — no `--` residue survives.
            self.ct
                .overwrite(attr.span.start, expr.start, "{...(__verter_void(");
            self.ct.overwrite(expr.end, attr.span.end, "), {})}");
            return;
        }
        // Static or no value — remove the attribute entirely (no type surface).
        remove_span(self.ct, attr.span);
    }

    /// Project a directive attribute.
    fn project_directive(
        &mut self,
        el: &SvelteElement,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        match dir.kind {
            SvelteDirectiveKind::Bind => {
                self.project_bind(el, attr, dir);
            }
            SvelteDirectiveKind::On => {
                // F13: a COMPONENT-kind element routes `on:event={h}` to the
                // checked `__verter_event(Child, "event", h)` helper (the handler
                // is checked against the component's `$events["event"]` payload —
                // an unknown event name / wrong payload FAILS). An INTRINSIC
                // element keeps the verbatim DOM `onevent` rewrite (`on:click` →
                // `onclick`, typed by `SvelteHTMLElements`).
                if matches!(el.kind, SvelteElementKind::Component) {
                    self.rewrite_component_on_event(el, attr, dir);
                } else {
                    self.rewrite_legacy_on(attr, dir);
                }
            }
            SvelteDirectiveKind::Class => {
                // `class:active={cond}` → keep as a checkable boolean attribute
                // by rewriting to `data-class-active={cond}` (SUPPORTED legacy
                // coverage — the condition expression stays void-checked). The
                // VALUELESS shorthand `class:active` (≡ `class:active={active}`)
                // instead projects the implied `active` binding MAPPED, so
                // hover/definition on the identifier resolves to its script
                // declaration.
                if dir.value.is_none() && is_valid_binding_identifier(&dir.local) {
                    self.rewrite_class_directive_shorthand(attr, dir);
                } else {
                    self.rewrite_class_directive_to_data(attr, dir);
                }
            }
            SvelteDirectiveKind::Style => {
                // `style:color={c}` / `style:color|important` (F1) — SUPPORTED.
                // A `style:`-prefixed name is not a valid JSX attribute
                // identifier, so the directive is STRIPPED from the JSX position
                // (mirroring the CSS-custom-property pass-through) and its value
                // is void-checked. The `|important` modifier is presentational.
                // The shorthand `style:color` (no value) projects the implied
                // `color` binding only when the name is a valid binding identifier.
                self.rewrite_style_directive(attr, dir);
            }
            SvelteDirectiveKind::Use => {
                // `use:action` (+ parameter) — SUPPORTED (basic action parameter
                // checking). The action local is a real script identifier
                // (actions are functions), so it stays MAPPED for
                // hover/definition; the parameter keeps its own void-check.
                self.rewrite_use_directive(attr, dir);
            }
            SvelteDirectiveKind::Transition
            | SvelteDirectiveKind::In
            | SvelteDirectiveKind::Out => {
                // `transition:fn={p}` / `in:fn={p}` / `out:fn={p}` (+`|local`/
                // `|global`) (F2) — SUPPORTED. Stripped from the JSX position and
                // spread-merged into a `__verter_transition(node_hint, fn, p)`
                // check (like `__verter_attach`): the transition function `fn`
                // (the directive local) and the params `p` are checked against the
                // host element's instance type. The `|local`/`|global` modifiers
                // are presentational.
                self.rewrite_transition_directive(el, attr, dir);
            }
            SvelteDirectiveKind::Animate => {
                // `animate:fn={p}` (F3) — SUPPORTED. Stripped + spread-merged into
                // `__verter_animate(fn(NODE_HINT, DIRECTIONS, p))`.
                self.rewrite_animate_directive(el, attr, dir);
            }
            SvelteDirectiveKind::Let => {
                // `let:item={alias}` slot-prop binding — its `$`-names are
                // collected as block bindings (scoped to the element's children)
                // by `collect_let_directive_dollar_names`. The directive itself is
                // not a valid JSX attribute, so it is STRIPPED from the JSX
                // position (the binding contributes a child-scope local, not a
                // prop).
                remove_span(self.ct, attr.span);
            }
            SvelteDirectiveKind::Unknown => {
                remove_span(self.ct, attr.span);
            }
        }
    }

    /// Overwrite the directive tail AFTER the local name (any `|modifier`
    /// suffix / `=…` tail up to the attribute end) with `content`, or APPEND
    /// `content` when the local itself ends the attribute (`overwrite` no-ops
    /// on an empty range, so the suffix must ride the mapped local's end).
    fn overwrite_directive_tail(&mut self, local_end: u32, attr_end: u32, content: &str) {
        if local_end >= attr_end {
            self.ct.append_left(local_end, content);
        } else {
            self.ct.overwrite(local_end, attr_end, content);
        }
    }

    /// Project a `use:action` directive.
    ///
    /// `use:foo={p}` → `{...(__verter_void(foo), __verter_void(p), {})}` and a
    /// valueless `use:foo` → `{...(__verter_void(foo), {})}`. The directive is
    /// stripped from the JSX position, but the action local keeps its AUTHORED
    /// BYTES (mapped) inside a void-check — it is a real script identifier
    /// (actions are functions), so an unknown action name is a type error
    /// (matching svelte-check) and hover/definition on it resolves to the
    /// script declaration. The parameter expression stays mapped + void-checked
    /// as before. A non-identifier local emits no surface (removed outright).
    fn rewrite_use_directive(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        if !is_valid_binding_identifier(&dir.local) {
            remove_span(self.ct, attr.span);
            return;
        }
        // The local directly follows the `use:` prefix in the attribute span.
        let local_start = attr.span.start + "use:".len() as u32;
        let local_end = local_start + dir.local.len() as u32;
        if let Some(SvelteAttributeValue::Expression(expr)) = dir.value {
            // F11: a store-sub in the void-checked value (`use:action={$store}`).
            self.rewrite_store_subs_in(expr);
            self.ct
                .overwrite(attr.span.start, local_start, "{...(__verter_void(");
            self.ct
                .overwrite(local_end, expr.start, "), __verter_void(");
            self.ct.overwrite(expr.end, attr.span.end, "), {})}");
            return;
        }
        self.ct
            .overwrite(attr.span.start, local_start, "{...(__verter_void(");
        self.overwrite_directive_tail(local_end, attr.span.end, "), {})}");
    }

    /// Project a `style:` directive (F1).
    ///
    /// `style:color={c}` / `style:color|important` — a `style:`-prefixed name is
    /// not a valid JSX attribute identifier, so the directive is STRIPPED from
    /// the JSX position (mirroring the CSS-custom-property pass-through) and its
    /// value is void-checked: `{...(__verter_void(c), {})}` (the value stays
    /// mapped/checkable, contributes no prop). The `|important` modifier is
    /// presentational. The valueless SHORTHAND `style:color` projects the implied
    /// `color` binding identifier (`{...(__verter_void(color), {})}`) ONLY when
    /// `color` is a valid JS binding identifier; otherwise the attribute is
    /// removed outright (no type surface, no invalid identifier residue).
    fn rewrite_style_directive(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        if let Some(SvelteAttributeValue::Expression(expr)) = dir.value {
            // F11: a store-sub in the void-checked `style:color={$store}` value.
            self.rewrite_store_subs_in(expr);
            // `style:NAME[|mods]={` → `{...(__verter_void(` ; trailing `}` →
            // `), {})}`. The whole directive name+modifiers prefix becomes the
            // spread opener — no `style:` residue survives, the value is mapped.
            self.ct
                .overwrite(attr.span.start, expr.start, "{...(__verter_void(");
            self.ct.overwrite(expr.end, attr.span.end, "), {})}");
            return;
        }
        // Shorthand `style:color` (no `={…}`): project the implied `color`
        // binding when it is a valid identifier so its type errors / hover
        // survive — `{...(__verter_void(color), {})}`. The local keeps its
        // AUTHORED BYTES (mapped) so hover/definition on the identifier
        // resolves to the script declaration. Any `|modifier` suffix rides the
        // trailing overwrite (presentational).
        if is_valid_binding_identifier(&dir.local) {
            let local_start = attr.span.start + "style:".len() as u32;
            let local_end = local_start + dir.local.len() as u32;
            self.ct
                .overwrite(attr.span.start, local_start, "{...(__verter_void(");
            self.overwrite_directive_tail(local_end, attr.span.end, "), {})}");
            return;
        }
        remove_span(self.ct, attr.span);
    }

    /// Project a `transition:` / `in:` / `out:` directive (F2).
    ///
    /// `transition:fn={p}` (+`|local`/`|global`) → a spread-merged
    /// `{...(__verter_transition(fn(NODE_HINT, p)), {})}` — the directive is
    /// stripped from the JSX position and the transition function `fn` (the
    /// directive local, an imported function identifier) is CALLED on the host
    /// element instance (`NODE_HINT`, a typed `null!` cast keyed off the host
    /// tag) with the params `p`. A real call site is the soundest projection:
    /// TSGO checks the host-node type, the params type, the arg count (a
    /// non-function `fn` is not callable, a missing required `params` is an
    /// arg-count error, a wrong `params` is a type error), and the result is
    /// asserted to be a `TransitionConfig` through `__verter_transition` (a thin
    /// result-shape checker). The `|local`/`|global` modifiers are
    /// presentational. A valueless `transition:fn` (no params) calls
    /// `fn(NODE_HINT)`. A non-identifier local emits no call (the attribute is
    /// removed — no invalid identifier residue).
    fn rewrite_transition_directive(
        &mut self,
        el: &SvelteElement,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        if !is_valid_binding_identifier(&dir.local) {
            remove_span(self.ct, attr.span);
            return;
        }
        let node_hint = self.host_element_hint(el);
        let fn_name = dir.local.clone();
        if let Some(SvelteAttributeValue::Expression(expr)) = dir.value {
            // F11: a store-sub in the transition params (`transition:fn={$p}`).
            self.rewrite_store_subs_in(expr);
            // `transition:fn[|mods]={` →
            // `{...(__verter_transition({fn}({hint}, ` ; trailing `}` →
            // `)), {})}`. The params expression `p` stays mapped (its inner type
            // errors + hover survive). A Svelte transition function is invoked at
            // RUNTIME as `fn(node, params, { direction })`, but the PUBLIC TYPES of
            // the built-in transitions (`fly`/`fade`/`slide`/…) and idiomatic
            // userland transitions declare only `(node, params?)` — the trailing
            // `options` is always optional/absent in the type surface. Passing a
            // third arg would therefore break every two-param built-in, so the
            // projected call is `fn(node, params)` (host node + params): a custom
            // transition that DECLARES an `options` param keeps it optional (the
            // `custom_transition_with_optional_options_…` gate fixture pins this).
            self.ct.overwrite(
                attr.span.start,
                expr.start,
                &format!("{{...(__verter_transition({fn_name}({node_hint}, "),
            );
            self.ct.overwrite(expr.end, attr.span.end, ")), {})}");
            return;
        }
        // No params: call `fn(NODE_HINT)`.
        self.ct.overwrite(
            attr.span.start,
            attr.span.end,
            &format!("{{...(__verter_transition({fn_name}({node_hint})), {{}})}}"),
        );
    }

    /// Project an `animate:` directive (F3).
    ///
    /// `animate:fn={p}` →
    /// `{...(__verter_animate(fn(NODE_HINT, DIRECTIONS, p)), {})}` — the directive
    /// is stripped and the animate function `fn` is CALLED on the host element
    /// with a synthetic from/to-rect `DIRECTIONS` descriptor and the params `p`.
    /// As for transitions, the real call site is the soundest check (host node +
    /// params + arity + non-function), and the result is asserted to be an
    /// `AnimationConfig` through `__verter_animate`. A valueless `animate:fn`
    /// calls `fn(NODE_HINT, DIRECTIONS)`; a non-identifier local emits no call.
    fn rewrite_animate_directive(
        &mut self,
        el: &SvelteElement,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        if !is_valid_binding_identifier(&dir.local) {
            remove_span(self.ct, attr.span);
            return;
        }
        let node_hint = self.host_element_hint(el);
        let fn_name = dir.local.clone();
        let directions = match self.dialect {
            SvelteIdeDialect::TypeScript => "(null! as { from: DOMRect; to: DOMRect })",
            SvelteIdeDialect::JavaScript => {
                "(/** @type {{ from: DOMRect, to: DOMRect }} */ (/** @type {unknown} */ (null)))"
            }
        };
        if let Some(SvelteAttributeValue::Expression(expr)) = dir.value {
            // F11: a store-sub in the animate params (`animate:fn={$p}`).
            self.rewrite_store_subs_in(expr);
            self.ct.overwrite(
                attr.span.start,
                expr.start,
                &format!("{{...(__verter_animate({fn_name}({node_hint}, {directions}, "),
            );
            self.ct.overwrite(expr.end, attr.span.end, ")), {})}");
            return;
        }
        self.ct.overwrite(
            attr.span.start,
            attr.span.end,
            &format!("{{...(__verter_animate({fn_name}({node_hint}, {directions})), {{}})}}"),
        );
    }

    /// The typed node-hint expression for a `transition:`/`animate:`-host element.
    ///
    /// For an INTRINSIC element the hint resolves the precise DOM instance type
    /// via the prelude's `__VerterHostEl<Tag>` (known HTML/SVG tag → its element
    /// type, unknown/custom → `Element`). For a component / `<svelte:*>` /
    /// dynamic host the host element type is unknown, so the hint falls back to
    /// the `Element` bound.
    fn host_element_hint(&self, el: &SvelteElement) -> String {
        match el.kind {
            // `el.name` is interpolated raw into a `__VerterHostEl<"…">` string
            // literal. The parser only classifies a bare tag identifier as
            // `Intrinsic`, so this holds today; the RUNTIME guard (consistent
            // with `bind.rs::bind_this_host_type`) falls back to the `Element`
            // bound if a future producer change admits a `"`/newline into the
            // name — never emitting a broken type literal.
            SvelteElementKind::Intrinsic if is_bare_tag_identifier(&el.name) => {
                match self.dialect {
                    SvelteIdeDialect::TypeScript => {
                        format!("(null! as __VerterHostEl<\"{}\">)", el.name)
                    }
                    SvelteIdeDialect::JavaScript => format!(
                    "(/** @type {{__VerterHostEl<\"{}\">}} */ (/** @type {{unknown}} */ (null)))",
                    el.name
                ),
                }
            }
            _ => match self.dialect {
                SvelteIdeDialect::TypeScript => "(null! as Element)".to_string(),
                SvelteIdeDialect::JavaScript => {
                    "(/** @type {Element} */ (/** @type {unknown} */ (null)))".to_string()
                }
            },
        }
    }

    /// Whether a `bind:` directive value is a function binding
    /// (`bind:x={get, set}` — a top-level comma in the value expression).
    pub(super) fn is_function_binding(&self, dir: &crate::svelte::parser::SvelteDirective) -> bool {
        let Some(SvelteAttributeValue::Expression(span)) = dir.value else {
            return false;
        };
        let body = self.slice(span);
        // A top-level comma (depth 0, OUTSIDE string/template/char literals)
        // marks the `get, set` function-binding form. The scanner skips literal
        // bodies so a comma inside a string (`bind:x={() => f("a,b"), set}`) does
        // not false-positive — and an escaped quote inside a literal is honoured.
        let mut depth = 0i32;
        let mut chars = body.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\'' | '"' | '`' => skip_string_literal(&mut chars, ch),
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => return true,
                _ => {}
            }
        }
        false
    }

    /// Project a `bind:` directive to a checkable JSX attribute pair (the
    /// component `$bindable`-prop path + `bind:value`/`bind:checked`).
    ///
    /// `bind:value={v}` → `value={v}` (strip the `bind:` prefix, keep the
    /// `={v}` value mapped). The valueless SHORTHAND `bind:value` (no `={…}`)
    /// binds the same-named local, so it becomes `value={value}` — the whole
    /// `bind:local` run is overwritten with `local={local}` (a bare `value`
    /// attribute would be a boolean `true`, not the bound value).
    pub(super) fn rewrite_bind_to_attribute(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        if let Some(SvelteAttributeValue::Expression(expr)) = dir.value {
            // Strip `bind:` (prefix + colon), keeping `local={value}`. The value
            // expression stays a mapped chunk, so a store-sub in it (`bind:value=
            // {$store}`) is rewritten through the same `$`-span overwrite (F11,
            // P1-1) — it composes with the prefix strip.
            self.rewrite_store_subs_in(expr);
            let prefix_len = "bind:".len() as u32;
            self.ct
                .overwrite(attr.span.start, attr.span.start + prefix_len, "");
        } else if dir.value.is_some() {
            // A non-expression value (static/quoted) — strip the prefix only.
            let prefix_len = "bind:".len() as u32;
            self.ct
                .overwrite(attr.span.start, attr.span.start + prefix_len, "");
        } else {
            // Valueless shorthand `bind:local` → `local={local}`.
            let local = &dir.local;
            self.ct.overwrite(
                attr.span.start,
                attr.span.end,
                &format!("{local}={{{local}}}"),
            );
        }
    }

    /// Rewrite an INTRINSIC element's legacy `on:event` to `onevent` (verbatim
    /// lowercase, typed by `SvelteHTMLElements`).
    fn rewrite_legacy_on(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        // `on:click` → `onclick`. Overwrite `on:` with `on` (drop the colon),
        // keeping the event local + value.
        let Some(SvelteAttributeValue::Expression(expr)) = dir.value else {
            // Event forwarding and malformed static handlers have no authored
            // callback expression to type-check. Leaving a valueless `onclick`
            // would instead fabricate a boolean-handler TS2322.
            remove_span(self.ct, attr.span);
            return;
        };

        let start = attr.span.start;
        let prefix_len = "on:".len() as u32;
        self.ct.overwrite(start, start + prefix_len, "on");
        // Modifiers affect runtime listener behavior; they are never valid TSX
        // attribute syntax. Keep the event local and handler chunks mapped while
        // replacing only the directive punctuation/modifier span.
        let local_end = start + prefix_len + dir.local.len() as u32;
        self.ct.overwrite(local_end, expr.start, "={");
        // F11: a store-sub in the kept handler value (`on:click={$handler}`).
        self.rewrite_store_subs_in(expr);
    }

    /// Rewrite a COMPONENT element's legacy `on:event={handler}` to the checked
    /// `__verter_event` helper (F13).
    ///
    /// `<Child on:select={h}>` → `{...(__verter_event(Child, "select", h), {})}` —
    /// a no-prop JSX spread that CALLS `__verter_event(component, name, handler)`.
    /// The helper's `name` parameter is constrained to `keyof $events & string`
    /// (an unknown event name FAILS) and its `handler` parameter is typed
    /// `$events[name]` (a wrong payload type FAILS). The component reference is the
    /// element's name; a non-identifier component reference (a dynamic
    /// `<svelte:component>` routes through F8, not here) is not reachable on a
    /// `Component`-kind element. A `<Child on:select>` with NO handler value is a
    /// legacy event FORWARD (re-dispatch to the parent) — there is no local
    /// handler to check, so the directive is stripped (no type surface).
    fn rewrite_component_on_event(
        &mut self,
        el: &SvelteElement,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        let Some(SvelteAttributeValue::Expression(expr)) = dir.value else {
            // Event forwarding (`on:select` with no value) — strip, no surface.
            remove_span(self.ct, attr.span);
            return;
        };
        // A component reference must be a valid identifier to index its `$events`.
        if !is_valid_component_reference(&el.name) {
            remove_span(self.ct, attr.span);
            return;
        }
        // F11: a store-sub in the handler value (`on:select={$handler}`) is
        // rewritten before the surrounding bytes are overwritten.
        self.rewrite_store_subs_in(expr);
        // `on:select={` → `{...(__verter_event(Child, "select", ` ; trailing `}` →
        // `), {})}`. The handler bytes stay mapped between the two overwrites.
        self.ct.overwrite(
            attr.span.start,
            expr.start,
            &format!("{{...(__verter_event({}, \"{}\", ", el.name, dir.local),
        );
        self.ct.overwrite(expr.end, attr.span.end, "), {})}");
    }

    /// Project a VALUELESS `class:` shorthand (`class:active` ≡
    /// `class:active={active}`) to the implied binding void-check —
    /// `{...(__verter_void(active), {})}`. The local is BOTH the class name and
    /// the implied condition binding identifier; keeping its AUTHORED BYTES
    /// mapped lets hover/definition on the shorthand resolve to the script
    /// declaration, and an unknown shorthand name is a type error (matching
    /// svelte-check). Only called for a valid-identifier local; the valued form
    /// and non-identifier locals keep the `data-class-*` rewrite.
    fn rewrite_class_directive_shorthand(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        let local_start = attr.span.start + "class:".len() as u32;
        let local_end = local_start + dir.local.len() as u32;
        self.ct
            .overwrite(attr.span.start, local_start, "{...(__verter_void(");
        self.overwrite_directive_tail(local_end, attr.span.end, "), {})}");
    }

    /// Rewrite a `class:` directive to a `data-class-*` attribute keeping the
    /// condition value mapped.
    fn rewrite_class_directive_to_data(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        // Replace `class:active` (name part) with `data-class-active`. We
        // overwrite from the attribute start to the end of the local name.
        let name_end = attr.span.start + ("class:".len() + dir.local.len()) as u32;
        let replacement = format!("data-class-{}", dir.local);
        self.ct.overwrite(attr.span.start, name_end, &replacement);
        // F11: a store-sub in the kept `={$store}` condition value is rewritten.
        if let Some(SvelteAttributeValue::Expression(expr)) = dir.value {
            self.rewrite_store_subs_in(expr);
        }
    }
}
