//! The Svelte IDE `bind:` directive projection (F4/F5).
//!
//! The wide `bind:` family and function bindings project to type-checked IDE
//! TSX via the generated bind-contract table (the source of truth) and the
//! prelude's `__verter_bind_*` checker helpers. This module is a continuation
//! of [`super`]'s `TemplateProjector` impl — extracted for file size; it accesses
//! the parent module's private projector type, helpers, and the contract table
//! through `use super::*`.

use super::*;

impl TemplateProjector<'_, '_> {
    /// Project a `bind:` directive (F4/F5).
    ///
    /// The dispatch (in order):
    /// 1. `bind:this` (any host) → the host-instance invariant check (dispatched
    ///    FIRST — never a function binding; its checker owns the whole value).
    /// 2. `bind:group` on an `<input>` → the checkbox/radio array-shape check
    ///    (special only where its contract applies; any other tag falls through).
    /// 3. A FUNCTION binding `bind:x={get, set}` (top-level comma) → the F5
    ///    `__verter_bind_fn` checker (element value type from the bind-contract
    ///    table or the intrinsic attribute table; component value type via
    ///    `InstanceType<typeof C>["$props"][K]`).
    /// 4. A COMPONENT `bind:prop` (non-`this`) → the `$bindable`-prop path: strip
    ///    `bind:`, keep `prop={value}` checked against the component's `$props`.
    /// 5. An intrinsic binding IN the bind-contract table → the directional
    ///    value-type check (read-write / read).
    /// 6. `bind:value`/`bind:checked` (and any other intrinsic name not in the
    ///    table) → strip `bind:`, keep `name={value}` checked against the
    ///    intrinsic attribute table.
    pub(super) fn project_bind(
        &mut self,
        el: &SvelteElement,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        let is_component = matches!(el.kind, SvelteElementKind::Component);

        // The SPECIAL bindings (`bind:this`, `bind:group`) are dispatched FIRST —
        // they are never function bindings, and their dedicated checkers own the
        // whole value (so a stray `bind:this={a, b}` never leaks the `{HOST}`
        // placeholder through the generic F5 path). `bind:group` is special only
        // where its contract applies (an `<input>`); on any other tag it is an
        // unknown binding and falls through to the attribute/error path.
        if dir.local == "this" {
            self.project_bind_this(el, attr, dir, is_component);
            return;
        }
        if dir.local == "group"
            && !is_component
            && lookup_bind_contract("group", &el.name).is_some()
        {
            self.project_bind_group(el, attr, dir);
            return;
        }

        // A FUNCTION binding `bind:x={get, set}` (top-level comma) → the F5
        // checker. Component prop binds and element table binds both flow here.
        if self.is_function_binding(dir) {
            self.project_function_binding(el, attr, dir, is_component);
            return;
        }

        // A component `bind:prop` (non-`this`) is the `$bindable`-prop path —
        // strip the `bind:` prefix and keep `prop={value}` checked against the
        // component's `$props` member (the JSX `ElementAttributesProperty`).
        if is_component {
            self.rewrite_bind_to_attribute(attr, dir);
            return;
        }

        // Intrinsic element in the wide-family table → the directional value-type
        // check.
        if let Some(contract) = lookup_bind_contract(&dir.local, &el.name) {
            self.project_table_bind(attr, dir, contract);
            return;
        }
        // `bind:value`/`bind:checked` and any other intrinsic name not in the
        // wide-family table → a plain checkable JSX attribute (`name={value}`),
        // checked against the intrinsic attribute table.
        self.rewrite_bind_to_attribute(attr, dir);
    }

    /// Project `bind:this={el}` (F4) — a host-instance assignment-compat check.
    ///
    /// The host-instance type is the element's DOM instance type for an intrinsic
    /// (`__VerterHostEl<"tag">`), `InstanceType<typeof Name>` for a component, and
    /// the `Element` bound for a dynamic/special host. The check is INVARIANT and
    /// DISCRIMINATES a wrong element type (a `HTMLDivElement` local on an
    /// `<input>` FAILS, where a one-directional `V extends L` check would pass
    /// since DOM element instance types are largely mutually assignable).
    ///
    /// Two emission shapes, by lvalue kind:
    /// - a TYPE-QUERY-SAFE lvalue (a bare identifier or a dotted member chain,
    ///   e.g. `el` / `refs.first`) is commonly declared WITHOUT an initializer
    ///   (`let el: HTMLInputElement`), so the check must NOT read the local's
    ///   value: `{...((LOCAL = (null! as Host)), __verter_bind_this_assignable<
    ///   Host, typeof LOCAL>(), {})}` — the assignment gives `Host extends typeof
    ///   LOCAL` (plus definite assignment; a `const` target FAILS), the assert's
    ///   `To extends Host` constraint gives `typeof LOCAL extends Host`, together
    ///   invariant, WITHOUT reading the (possibly unassigned) value.
    /// - any OTHER lvalue (an element-access `refs[i]`, a call result, …) is not
    ///   `typeof`-safe (`typeof refs[i]` parses `i` as a type), and such targets
    ///   are always already-initialized slots, so the read-bearing invariant
    ///   `{...((LOCAL = __verter_bind_rw<Host>(LOCAL)), {})}` applies (arg checks
    ///   `LOCAL` → `Host`, the returned `Host` assigns back → invariant).
    ///
    /// A valueless / non-expression `bind:this` is removed (no bound target).
    fn project_bind_this(
        &mut self,
        el: &SvelteElement,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
        is_component: bool,
    ) {
        let Some(SvelteAttributeValue::Expression(expr)) = dir.value else {
            remove_span(self.ct, attr.span);
            return;
        };
        let host_ty = self.bind_this_host_type(el, is_component);
        let local = self.slice(expr).to_string();
        self.ct.overwrite(attr.span.start, expr.start, "{...((");
        if is_type_query_safe_lvalue(&local) {
            // `bind:this={` → `{...((` ; the LOCAL (mapped) is the assignment
            // target ; trailing `}` → ` = (null! as Host)),
            // __verter_bind_this_assignable<Host, typeof LOCAL>(), {})}`.
            self.ct.overwrite(
                expr.end,
                attr.span.end,
                &format!(
                    " = (null! as {host_ty})), \
                     __verter_bind_this_assignable<{host_ty}, typeof {local}>(), {{}})}}"
                ),
            );
        } else {
            // Non-`typeof`-safe lvalue (element access, …) — the read-bearing
            // invariant `LOCAL = __verter_bind_rw<Host>(LOCAL)`.
            self.ct.overwrite(
                expr.end,
                attr.span.end,
                &format!(" = __verter_bind_rw<{host_ty}>({local})), {{}})}}"),
            );
        }
    }

    /// The host-instance type for a `bind:this` target.
    fn bind_this_host_type(&self, el: &SvelteElement, is_component: bool) -> String {
        if is_component && is_valid_component_reference(&el.name) {
            // A component instance: `InstanceType<typeof Name>`.
            format!("InstanceType<typeof {}>", el.name)
        } else if matches!(el.kind, SvelteElementKind::Intrinsic)
            && is_bare_tag_identifier(&el.name)
        {
            format!("__VerterHostEl<\"{}\">", el.name)
        } else {
            // A `<svelte:*>` / dynamic / unverifiable host — the instance type is
            // unknown, fall back to the `Element` bound.
            "Element".to_string()
        }
    }

    /// Project `bind:group` (F4) — the checkbox/radio shared-selection check.
    ///
    /// `bind:group` on a `type="checkbox"` input shares ONE array variable; on a
    /// `type="radio"` input it shares ONE item variable. The projection routes
    /// through `__verter_bind_group_checkbox`/`__verter_bind_group_radio`, which
    /// require the local be an array / non-array (a loose `T | T[]` is rejected by
    /// both). The element `type` attribute selects the checker (default: radio,
    /// the Svelte default for a `bind:group` with no/non-checkbox type). Both
    /// round-trip (group is read-write), so a `const` target also fails.
    fn project_bind_group(
        &mut self,
        el: &SvelteElement,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
    ) {
        let Some(SvelteAttributeValue::Expression(expr)) = dir.value else {
            remove_span(self.ct, attr.span);
            return;
        };
        let checker = if self.element_input_type_is_checkbox(el) {
            "__verter_bind_group_checkbox"
        } else {
            "__verter_bind_group_radio"
        };
        // `bind:group={` → `{...((` ; LOCAL stays mapped ; trailing `}` →
        // ` = CHECKER(LOCAL)), {})}`. The round-trip assignment makes a const
        // target fail too.
        self.ct.overwrite(attr.span.start, expr.start, "{...((");
        let local = self.slice(expr).to_string();
        self.ct.overwrite(
            expr.end,
            attr.span.end,
            &format!(" = {checker}({local})), {{}})}}"),
        );
    }

    /// Whether the element carries a static `type="checkbox"` attribute (the
    /// `bind:group` checkbox-vs-radio selector). A non-static `type={…}` or an
    /// absent/other `type` is treated as radio (the Svelte default).
    fn element_input_type_is_checkbox(&self, el: &SvelteElement) -> bool {
        el.attributes.iter().any(|a| {
            if let SvelteAttributeKind::Plain {
                name,
                value: Some(SvelteAttributeValue::Text(span)),
                ..
            } = &a.kind
            {
                name == "type" && self.slice(*span) == "checkbox"
            } else {
                false
            }
        })
    }

    /// Project a table-driven intrinsic `bind:*` (F4) — the directional
    /// value-type check.
    ///
    /// The bound LOCAL is checked against the contract's value type `V` in the
    /// contract's DIRECTION via the prelude helpers:
    /// - read-write: `{...((LOCAL = __verter_bind_rw<V>(LOCAL)), {})}` (invariant);
    /// - read: `{...((LOCAL = __verter_bind_read<V>()), {})}` (DOM → local; a
    ///   wrong-typed / `const` target FAILS — the readonly write-rejection).
    ///
    /// (The closed Svelte element-binding vocabulary has no write-only direction;
    /// `BindDirection` is the two-arm read / read-write taxonomy.)
    ///
    /// A valueless / non-expression binding is removed (no bound target).
    fn project_table_bind(
        &mut self,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
        contract: &BindContract,
    ) {
        let Some(SvelteAttributeValue::Expression(expr)) = dir.value else {
            remove_span(self.ct, attr.span);
            return;
        };
        let v = contract.value_type;
        match contract.direction {
            BindDirection::ReadWrite => {
                self.ct.overwrite(attr.span.start, expr.start, "{...((");
                let local = self.slice(expr).to_string();
                self.ct.overwrite(
                    expr.end,
                    attr.span.end,
                    &format!(" = __verter_bind_rw<{v}>({local})), {{}})}}"),
                );
            }
            BindDirection::Read => {
                self.ct.overwrite(attr.span.start, expr.start, "{...((");
                self.ct.overwrite(
                    expr.end,
                    attr.span.end,
                    &format!(" = __verter_bind_read<{v}>()), {{}})}}"),
                );
            }
        }
    }

    /// Project an F5 function binding `bind:x={get, set}` (or write-only
    /// `bind:x={null, set}`).
    ///
    /// The two value expressions stay mapped and are checked against the
    /// bind-target type `V` via `__verter_bind_fn<V>(get, set)`: `get` returns
    /// `V` (or `null`), `set` consumes `V`. The target type `V` is:
    /// - a COMPONENT bind: `InstanceType<typeof C>["$props"]["prop"]` (the typing
    ///   is done in the projected TSX via TS — no Rust resolver call);
    /// - an element bind in the contract table: the contract's value type;
    /// - otherwise (an element name not in the table): inferred (`V` is left
    ///   generic, so the checker enforces get/set mutual consistency alone).
    ///
    /// A READONLY element binding (`bind:clientWidth={…}`, etc.) routes to the
    /// readonly checker `__verter_bind_fn_read<V>` whose `get` is `null`-only —
    /// a readonly function binding must be the write-only `{null, set}` form, so
    /// a non-null getter FAILS.
    fn project_function_binding(
        &mut self,
        el: &SvelteElement,
        attr: &SvelteAttribute,
        dir: &crate::svelte::parser::SvelteDirective,
        is_component: bool,
    ) {
        let Some(SvelteAttributeValue::Expression(expr)) = dir.value else {
            // A function binding with no `={…}` is malformed — remove it.
            remove_span(self.ct, attr.span);
            return;
        };
        // The explicit target type + direction, if known. A component bind derives
        // the type in TS from the component's `$props` (always read-write); an
        // element bind reads the contract table (type + direction).
        let (target_ty, readonly) = if is_component && is_valid_component_reference(&el.name) {
            (
                Some(format!(
                    "InstanceType<typeof {}>[\"$props\"][\"{}\"]",
                    el.name, dir.local
                )),
                false,
            )
        } else if !is_component {
            match lookup_bind_contract(&dir.local, &el.name) {
                Some(c) => (
                    Some(c.value_type.to_string()),
                    matches!(c.direction, BindDirection::Read),
                ),
                // An intrinsic binding NOT in the wide-family table (`value`,
                // `checked`, …) derives its target type from the Svelte intrinsic
                // attribute table — `SvelteHTMLElements["tag"]["local"]` — so a
                // DOM-wrong get/set pair (a boolean get/set for `<input
                // bind:value>`) FAILS. Typed entirely in the projected TSX (no
                // Rust resolver). Only when the tag + local are bare identifiers
                // (else leave `V` inferred — get/set consistency only).
                None if is_bare_tag_identifier(&el.name)
                    && is_valid_binding_identifier(&dir.local) =>
                {
                    (
                        Some(format!(
                            "import(\"svelte/elements\").SvelteHTMLElements[\"{}\"][\"{}\"]",
                            el.name, dir.local
                        )),
                        false,
                    )
                }
                None => (None, false),
            }
        } else {
            (None, false)
        };
        let checker = if readonly {
            "__verter_bind_fn_read"
        } else {
            "__verter_bind_fn"
        };
        let type_arg = target_ty.map(|t| format!("<{t}>")).unwrap_or_default();
        // `bind:x={` → `{...(CHECKER<V>(` ; the `get, set` expression pair stays
        // mapped ; trailing `}` → `), {})}`.
        self.ct.overwrite(
            attr.span.start,
            expr.start,
            &format!("{{...({checker}{type_arg}("),
        );
        self.ct.overwrite(expr.end, attr.span.end, "), {})}");
    }
}
