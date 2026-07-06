//! The DOM-hosted `$.bind_*` EMISSION half of the client backend (5c).
//!
//! Extracted from `client.rs` (the file-size guard boundary): these are the
//! [`ClientEmitter`] methods that emit a Svelte `bind:*` directive's runtime call
//! text — the inline `bind:this` interleave, the per-host bind PRELUDE cleanup
//! (`$.remove_input_defaults` / `$.remove_textarea_child` + the `bind:group`
//! per-input value write), the shape-routed `$.bind_*` dispatch, and the
//! data-driven DOM-value/property call formatter. They read the bind's accepted
//! [`ClientBindShape`] + the shared [`RuntimeBindRouting`] routing (never a
//! source-text scan), and the walk-populated DOM var maps.

use super::client::ClientEmitter;
use super::client_effect::Memoizer;
use super::client_plan::{ClientModulePlan, ClientRuntimeOp};
use super::client_plan_types::{AttrValue, AttrValuePart};
use super::client_shapes::{BindGetSetForm, ClientBindShape, GroupBindKey};
use super::client_walk::bind_host_prelude;
use super::ir::{IrNode, NodeId, SpecialKind};
use crate::svelte::bind_contract::{BindPrelude, RuntimeBindRouting};

impl<'a> ClientEmitter<'a> {
    /// Allocate ONE collision-safe `bind:group` accumulator name per DISTINCT group, keyed by
    /// each bind's [`GroupBindKey`] (structural target + scope). Iterates the plan ops in
    /// SOURCE ORDER so the first-seen group is `binding_group`, the next `binding_group_1`, …
    /// (each pushed past a user binding of the same name by the seeded [`alloc_name`](ClientEmitter::alloc_name)).
    /// Populates [`group_binding_names`](ClientEmitter::group_binding_names) (key → name) and
    /// [`group_binding_decls`](ClientEmitter::group_binding_decls) (the names in source order,
    /// for the component-body `const <name> = [];` decls). Two inputs sharing a target hit the
    /// same key and reuse its already-allocated name (one accumulator); two INDEPENDENT groups
    /// get distinct names. Called ONCE from [`ClientEmitter::new`] (a `group_key` is `Some`
    /// only for a `Group` routing, by construction in the bind classifier).
    pub(super) fn plan_group_accumulators(&mut self) {
        // Copy the plan reference so the op walk does not borrow `self` while `alloc_name`
        // mutates `self.used` + the group maps.
        let plan: &ClientModulePlan<'a> = self.plan();
        for op in plan.all_ops() {
            let ClientRuntimeOp::Bind {
                shape:
                    ClientBindShape::DomBind {
                        group_key: Some(key),
                        ..
                    },
                ..
            } = op
            else {
                continue;
            };
            if self.group_binding_names.contains_key(key) {
                continue;
            }
            let name = self.alloc_name(super::client::GROUP_BINDING_NAME);
            self.group_binding_names.insert(key.clone(), name.clone());
            self.group_binding_decls.push(name);
        }
    }

    /// Emit any INIT-DOMAIN render op targeting `node` INLINE during the walk, right
    /// after the node's ENTIRE child block (the child-walk descents and the trailing
    /// `$.reset`): the inline `bind:this` binds, the init-domain lifecycle ops
    /// (`$.action` / `$.attach`), and the effect-wrapped legacy events + non-`this`
    /// binds of a `use:` action host, in attribute SOURCE order. The official
    /// compiler emits `$.bind_this(node, …)` / `$.action(…)` / `$.attach(…)` /
    /// `$.effect(() => …)` as RENDER-side setup interleaved into the element walk —
    /// AFTER the element's init-domain attribute writes (`$.autofocus` /
    /// `$.set_class` / `$.set_attribute` / accumulator decls) AND after its child
    /// fragment (a static-children element has an empty child block, so the ops
    /// follow the inits directly), BEFORE the next sibling walk and BEFORE the
    /// grouped `$.template_effect` for sibling reactive text — whereas the bare
    /// non-`this` binds (no `use:`) join the after-update directive batch alongside
    /// `$.transition` / `$.animation` LAST, and modern events emit post-walk (after
    /// the text effect). Emitting them here, and SKIPPING the `This` / `Lifecycle`
    /// arms in [`Self::emit_ops`], matches that order byte-for-byte.
    pub(super) fn emit_inline_render_ops(&mut self, out: &mut String, node: NodeId) {
        // O(1) drain of this node's inline render ops from the per-node index built
        // once in `ClientEmitter::new` (no per-node re-scan of the full op vector).
        // Emitted in stored (plan-op = attribute source) order, so a `bind:this` and
        // an adjacent `use:` / `{@attach}` keep the official interleave.
        let Some(inline_ops) = self.inline_render_ops.remove(&node) else {
            return;
        };
        for op in inline_ops {
            match op {
                super::client_lifecycle::InlineRenderOp::BindThis {
                    getset,
                    getter,
                    setter,
                } => {
                    self.emit_bind(
                        out,
                        node,
                        &ClientBindShape::This { getset },
                        &getter,
                        &setter,
                    );
                }
                super::client_lifecycle::InlineRenderOp::Lifecycle(lifecycle) => {
                    out.push_str(&super::client_lifecycle::render_lifecycle_op(
                        &lifecycle,
                        &self.node_var,
                    ));
                }
                // A LEGACY `on:` event on a `use:` action host — the official
                // `$.effect(() => $.event(…));` wrap at the event's attribute
                // source position (the same arg assembly as the bare form).
                super::client_lifecycle::InlineRenderOp::EffectEvent(emit) => {
                    out.push_str(&super::client_event::render_effect_wrapped_event(
                        &emit,
                        &self.node_var,
                    ));
                }
                // A non-`this` DOM bind on a `use:` action host — the official
                // `$.effect(() => $.bind_*(…));` wrap at the bind's attribute
                // source position (the same arg assembly as the bare statement,
                // through the shared `render_bind_bare`).
                super::client_lifecycle::InlineRenderOp::EffectBind {
                    shape,
                    getter,
                    setter,
                } => {
                    out.push_str(&format!(
                        "\t$.effect(() => {});\n",
                        self.render_bind_bare(node, &shape, &getter, &setter)
                    ));
                }
            }
        }
    }

    /// Emit the per-host bind PRELUDE cleanup for an element (the official
    /// `RegularElement.js` default-clearing statement), DATA-DRIVEN from the bind's
    /// runtime routing: `$.remove_input_defaults(var)` for an `<input>` value/checked/
    /// group bind, `$.remove_textarea_child(var)` for a `<textarea bind:value>`,
    /// followed (for a `bind:group` input carrying a `value="X"`) by the per-input
    /// `var.value = var.__value = 'X'` group-value write — the official emission order.
    /// No-op when the element has no bind prelude / group value.
    pub(super) fn emit_bind_prelude(&self, out: &mut String, node: NodeId, region_var: &str) {
        let IrNode::Element(el) = self.ir().node(node) else {
            return;
        };
        match bind_host_prelude(el) {
            Some(BindPrelude::RemoveInputDefaults) => {
                out.push_str(&format!("\t$.remove_input_defaults({region_var});\n"));
            }
            Some(BindPrelude::RemoveTextareaChild) => {
                out.push_str(&format!("\t$.remove_textarea_child({region_var});\n"));
            }
            Some(BindPrelude::None) | None => {}
        }
        // The `bind:group` per-input value write: `var.value = var.__value = 'X'`. The
        // value literal is the classifier's recorded group-value fact for THIS node;
        // it is single-quoted (the official esrap quote form for the static literal).
        if let Some((_, literal)) = self
            .plan()
            .build
            .group_values
            .iter()
            .find(|(n, _)| *n == node)
        {
            out.push_str(&format!(
                "\t{region_var}.value = {region_var}.__value = {};\n",
                super::client_codegen_helpers::js_single_quoted(literal)
            ));
        }
        // The `bind:group` per-input DYNAMIC/mixed value. A node carries EITHER a static
        // literal (above) OR a dynamic value (here), never both. REACTIVE ⇒ declare the
        // `var <var>_value;` change-tracker at this prelude position (the guarded update joins
        // the combined `$.template_effect` post-walk, in [`Self::emit_group_dynamic_value_effect`]);
        // NON-reactive ⇒ a ONE-SHOT inline write here, the same position as the static write.
        // The OUTER `?? ''` coercion is gated on DEFINEDNESS (official `evaluated.is_defined`),
        // NOT single-vs-mixed: a NOT-provably-defined SINGLE value gets `var.value =
        // (var.__value = V) ?? ''`, while a provably-defined single value (and a mixed template
        // literal, already a string) gets `var.value = var.__value = V`.
        if let Some((_, gdv)) = self
            .plan()
            .build
            .group_dynamic_values
            .iter()
            .find(|(n, _)| *n == node)
        {
            if gdv.reactive {
                out.push_str(&format!("\tvar {region_var}_value;\n"));
            } else {
                let v = self.build_attr_value(&gdv.value, &mut None);
                if gdv.is_single_expression() && !gdv.single_value_defined {
                    out.push_str(&format!(
                        "\t{region_var}.value = ({region_var}.__value = {v}) ?? '';\n"
                    ));
                } else {
                    out.push_str(&format!(
                        "\t{region_var}.value = {region_var}.__value = {v};\n"
                    ));
                }
            }
        }
    }

    /// Build the `bind:group` REACTIVE dynamic-value guarded change-detection write for the
    /// combined `$.template_effect` — `if (<var>_value !== (<var>_value = V)) { <var>.value =
    /// (<var>.__value = V) ?? ''; }` (a NOT-provably-defined single value → OUTER `?? ''`) /
    /// `… = V; }` (a provably-defined single, or a mixed string). The outer `?? ''` is gated on
    /// DEFINEDNESS (official `evaluated.is_defined`), the SAME rule as the non-reactive inline
    /// write. The value `V` routes through the SHARED `memoizer`, so a `has_call` value becomes
    /// a `$N` deps slot computed ONCE and reused in the guard + the write. Returns `None` for a
    /// node with no group dynamic value, or a NON-reactive one (its one-shot write is emitted
    /// inline in [`Self::emit_bind_prelude`], not in the effect).
    pub(super) fn emit_group_dynamic_value_effect(
        &self,
        node: NodeId,
        memoizer: &mut super::client_effect::Memoizer,
    ) -> Option<String> {
        let (_, gdv) = self
            .plan()
            .build
            .group_dynamic_values
            .iter()
            .find(|(n, _)| n.0 == node.0)?;
        if !gdv.reactive {
            return None;
        }
        let var = self.dom_var(node);
        let tracker = format!("{var}_value");
        let v = self.build_attr_value(&gdv.value, &mut Some(memoizer));
        let write = if gdv.is_single_expression() && !gdv.single_value_defined {
            format!("{var}.value = ({var}.__value = {v}) ?? ''")
        } else {
            format!("{var}.value = {var}.__value = {v}")
        };
        Some(format!(
            "if ({tracker} !== ({tracker} = {v})) {{\n\t\t\t{write};\n\t\t}}"
        ))
    }

    /// The `bind:group` dynamic-value DEPENDENCY READ for the input's `$.bind_group` getter
    /// (`() => { <dep>; return GET; }`) — the value rendered WITHOUT the memoizer (the full
    /// inline expression, so the getter's reactive read is the un-memoized value, matching
    /// official). Present for BOTH reactive and non-reactive dynamic values; `None` for a
    /// static / absent group value (the getter stays the plain `() => GET` thunk).
    pub(super) fn group_dynamic_value_dep_read(&self, node: NodeId) -> Option<String> {
        let (_, gdv) = self
            .plan()
            .build
            .group_dynamic_values
            .iter()
            .find(|(n, _)| n.0 == node.0)?;
        Some(self.build_attr_value(&gdv.value, &mut None))
    }

    /// Build the emitted value expression for a structured [`AttrValue`], routing each
    /// expression part through `memoizer` (when `Some`) so a `has_call` value lands in
    /// the official deps-array form. A `Single` value emits its bare (possibly `$N`)
    /// expression; a `Mixed` value builds the `` `lit${expr ?? ''}lit` `` template with
    /// each expr resolved; a `Const` emits verbatim. Shared by the dynamic-attribute /
    /// class / style emitters and the `bind:group` dynamic-value helpers above.
    pub(super) fn build_attr_value(
        &self,
        value: &AttrValue,
        memoizer: &mut Option<&mut Memoizer>,
    ) -> String {
        match value {
            AttrValue::Const(text) => text.clone(),
            AttrValue::Single {
                rewritten,
                has_call,
            } => match memoizer {
                Some(m) => m.add(rewritten.clone(), *has_call),
                None => rewritten.clone(),
            },
            AttrValue::Mixed(parts) => {
                let mut tmpl = String::from("`");
                for part in parts {
                    match part {
                        AttrValuePart::Literal(text) => {
                            tmpl.push_str(&super::client_codegen_helpers::escape_template_text(
                                text,
                            ));
                        }
                        AttrValuePart::Expr {
                            rewritten,
                            has_call,
                            coalesce,
                        } => {
                            let v = match memoizer {
                                Some(m) => m.add(rewritten.clone(), *has_call),
                                None => rewritten.clone(),
                            };
                            // The `?? ''` coercion the plan computed (official
                            // `build_template_chunk`): a provably-defined part is RAW, an
                            // undecided part gets `?? ''` (parenthesized for a `&&`/`||`
                            // operand). A memoized part is the `$N` identifier slot `v`.
                            use super::reactive_fold::NullishCoalesce;
                            match coalesce {
                                NullishCoalesce::None => tmpl.push_str(&format!("${{{v}}}")),
                                NullishCoalesce::Bare => {
                                    tmpl.push_str(&format!("${{{v} ?? ''}}"));
                                }
                                NullishCoalesce::Parenthesized => {
                                    tmpl.push_str(&format!("${{({v}) ?? ''}}"));
                                }
                            }
                        }
                    }
                }
                tmpl.push('`');
                tmpl
            }
        }
    }

    /// Emit a `bind:*` op from its already-rewritten getter + setter bodies (the
    /// narrow plan op), DATA-DRIVEN from the bind's accepted [`ClientBindShape`]:
    /// `bind:this` emits the dedicated `$.bind_this(host, set, get)`; every DOM-value
    /// bind emits its routed `$.bind_*` / `$.bind_property` call (the helper / arity /
    /// event come from the shared [`RuntimeBindRouting`]). A `group` bind carries its
    /// extra `binding_group` + per-input value args. The body delegates to the
    /// IMMUTABLE [`Self::render_bind_stmt`]; the `&mut self` receiver is kept for the
    /// existing walk/post-walk call sites, which hold the emitter mutably.
    pub(super) fn emit_bind(
        &mut self,
        out: &mut String,
        target: NodeId,
        shape: &ClientBindShape,
        getter: &str,
        setter: &str,
    ) {
        out.push_str(&self.render_bind_stmt(target, shape, getter, setter));
    }

    /// The full `\t$.bind_*(…);\n` STATEMENT form of [`Self::render_bind_bare`] —
    /// the shape every bare (unwrapped) bind registration emits: the walk-inline
    /// `bind:this`, the post-walk special/global-host binds, and the after-update
    /// directive-batch binds. Immutable (`&self`) so the directive-batch render
    /// loop, which holds `&ClientRuntimeOp` borrows of the plan, can call it.
    pub(super) fn render_bind_stmt(
        &self,
        target: NodeId,
        shape: &ClientBindShape,
        getter: &str,
        setter: &str,
    ) -> String {
        format!(
            "\t{};\n",
            self.render_bind_bare(target, shape, getter, setter)
        )
    }

    /// Render the BARE `$.bind_*(…)` call expression — no leading indent, no
    /// terminating `;` — the single arg-assembly authority BOTH the statement form
    /// ([`Self::render_bind_stmt`]) and the `use:`-host effect wrap
    /// (`$.effect(() => $.bind_*(…))`, the [`InlineRenderOp::EffectBind`] arm of
    /// [`Self::emit_inline_render_ops`]) share, so the helper / arity / group logic
    /// can never diverge between the two carriers (mirrors `render_event_call`).
    ///
    /// [`InlineRenderOp::EffectBind`]: super::client_lifecycle::InlineRenderOp::EffectBind
    pub(super) fn render_bind_bare(
        &self,
        target: NodeId,
        shape: &ClientBindShape,
        getter: &str,
        setter: &str,
    ) -> String {
        let var = self.bind_host_expr(target);
        match shape {
            ClientBindShape::This { getset } => {
                // The set/get tokens: an identifier target synthesizes the thunks
                // (`($$value) => SET` / `() => GET`); a function-pair passes the
                // user-supplied (signal-rewritten) set/get DIRECTLY. The arg order is
                // (setter-slot, getter-slot) in BOTH forms — official `$.bind_this(el, set,
                // get)`.
                let (set_tok, get_tok) = match getset {
                    BindGetSetForm::TargetLvalue => {
                        (format!("($$value) => {setter}"), format!("() => {getter}"))
                    }
                    // A function-pair / store-accessor bind passes the plan's
                    // COMPLETE get/set expressions directly (no thunk wrapper).
                    BindGetSetForm::FunctionPair | BindGetSetForm::StoreAccessor => {
                        (setter.to_string(), getter.to_string())
                    }
                };
                format!("$.bind_this({var}, {set_tok}, {get_tok})")
            }
            ClientBindShape::DomBind {
                name,
                routing,
                getset,
                group_key,
            } => self.format_dom_bind(
                name,
                *routing,
                *getset,
                group_key.as_ref(),
                target,
                &var,
                getter,
                setter,
            ),
        }
    }

    /// Format ONE DOM-value/property bind call from its typed routing — the
    /// DATA-DRIVEN emit (no per-name match arm pile), as a BARE call expression
    /// (no indent, no terminating `;` — the [`Self::render_bind_bare`] carrier
    /// contract). The shape follows the pinned `svelte@5.56.3` forms exactly:
    /// - get/set helpers: `$.bind_value(el, () => GET, ($$value) => SET)`,
    ///   `$.bind_select_value` / `$.bind_checked` / `$.bind_current_time` /
    ///   `$.bind_paused` / `$.bind_content_editable('name', el, get, set)`;
    /// - setter-only helpers: `$.bind_played(el, ($$value) => SET)`,
    ///   `$.bind_element_size(el, 'name', ($$value) => SET)`;
    /// - the generic property form: `$.bind_property('name', 'event', el, set [, get])`
    ///   (the getter present iff the routing direction is read-write);
    /// - the group form: `$.bind_group(binding_group, [], el, get, set)`.
    ///
    /// The `getset` form decides the get/set TOKENS: a [`TargetLvalue`](BindGetSetForm::TargetLvalue)
    /// bind WRAPS the plan's getter/setter bodies in `() => GET` / `($$value) => SET`
    /// thunks; a [`FunctionPair`](BindGetSetForm::FunctionPair) bind (`bind:value={get,
    /// set}`) passes the plan's (already signal-rewritten) get/set expressions DIRECTLY,
    /// matching official (`$.bind_value(el, get, set)`). The per-helper ARGUMENT
    /// STRUCTURE (which slot carries get vs set, the string-literal name args, the
    /// getter-iff-read-write rule) is IDENTICAL across both forms — only the wrapper
    /// differs. A `group` bind additionally carries its `group_key`, which the `Group` arm
    /// resolves to its per-group accumulator name (see [`Self::plan_group_accumulators`]).
    #[allow(clippy::too_many_arguments)]
    fn format_dom_bind(
        &self,
        bind_name: &str,
        routing: RuntimeBindRouting,
        getset: BindGetSetForm,
        group_key: Option<&GroupBindKey>,
        node: NodeId,
        var: &str,
        getter: &str,
        setter: &str,
    ) -> String {
        use crate::svelte::bind_contract::{BindDirection, HelperArity, RuntimeHelper};
        // The get/set tokens: a target-lvalue bind synthesizes the thunks; a
        // function-pair passes the user-supplied (signal-rewritten) get/set directly.
        let (get_thunk, set_thunk) = match getset {
            BindGetSetForm::TargetLvalue => {
                (format!("() => {getter}"), format!("($$value) => {setter}"))
            }
            // A function-pair / store-accessor bind passes the plan's COMPLETE
            // get/set expressions directly (no thunk wrapper): the store getter
            // is the bare accessor thunk `$c`, the setter the complete
            // `($$value) => $.store_set(c, $$value)` closure.
            BindGetSetForm::FunctionPair | BindGetSetForm::StoreAccessor => {
                (getter.to_string(), setter.to_string())
            }
        };
        match routing.helper {
            RuntimeHelper::Value => {
                format!("$.bind_value({var}, {get_thunk}, {set_thunk})")
            }
            RuntimeHelper::SelectValue => {
                format!("$.bind_select_value({var}, {get_thunk}, {set_thunk})")
            }
            RuntimeHelper::Checked => {
                format!("$.bind_checked({var}, {get_thunk}, {set_thunk})")
            }
            RuntimeHelper::CurrentTime => {
                format!("$.bind_current_time({var}, {get_thunk}, {set_thunk})")
            }
            RuntimeHelper::Paused => {
                format!("$.bind_paused({var}, {get_thunk}, {set_thunk})")
            }
            RuntimeHelper::Played => format!("$.bind_played({var}, {set_thunk})"),
            RuntimeHelper::ElementSize => {
                // `$.bind_element_size(el, 'name', ($$value) => SET)`.
                format!("$.bind_element_size({var}, '{bind_name}', {set_thunk})")
            }
            RuntimeHelper::ContentEditable => {
                // `$.bind_content_editable('name', el, () => GET, ($$value) => SET)`.
                format!("$.bind_content_editable('{bind_name}', {var}, {get_thunk}, {set_thunk})")
            }
            RuntimeHelper::Property => {
                // `$.bind_property('name', 'event', el, set [, get])` — the getter is
                // present ONLY for a read-write property (a read-only property is
                // setter-only). Arg order is property-form-specific (set BEFORE get).
                let _ = routing.arity; // arity is Property by construction here.
                if routing.direction == BindDirection::ReadWrite {
                    format!(
                        "$.bind_property('{bind_name}', '{event}', {var}, {set_thunk}, {get_thunk})",
                        event = routing.prop_event,
                    )
                } else {
                    format!(
                        "$.bind_property('{bind_name}', '{event}', {var}, {set_thunk})",
                        event = routing.prop_event,
                    )
                }
            }
            RuntimeHelper::Group => {
                // `$.bind_group(binding_group, [], el, () => GET, ($$value) => SET)`.
                // The accumulator is the PER-DISTINCT-GROUP name the emitter allocated in
                // `ClientEmitter::new` (one collision-safe `binding_group[_N]` per distinct
                // group key, in source order) — looked up by THIS bind's `group_key` so two
                // independent groups reference DISTINCT accumulators (never a single
                // component-wide name), while two inputs sharing a target share one.
                let _ = HelperArity::GetSet;
                let group = group_key
                    .and_then(|key| self.group_binding_names.get(key))
                    .map(String::as_str)
                    .expect(
                        "a Group routing carries a group_key whose accumulator name was \
                         allocated per distinct group in ClientEmitter::new",
                    );
                // A DYNAMIC/mixed group value makes the getter read the value DEPENDENCY first,
                // then return the bound target (`() => { V; return GET; }`), so the group
                // re-evaluates when the value changes (official order). The dep-read is the
                // value rendered WITHOUT memoization (the full inline expression). A static /
                // absent value keeps the plain `() => GET` thunk.
                let get_tok = self
                    .group_dynamic_value_dep_read(node)
                    .map(|dep| format!("() => {{\n\t\t{dep};\n\n\t\treturn {getter};\n\t}}"))
                    .unwrap_or(get_thunk);
                format!("$.bind_group({group}, [], {var}, {get_tok}, {set_thunk})")
            }
            RuntimeHelper::WindowSize => {
                // `$.bind_window_size('<name>', set)` — the dimension NAME is the first
                // string-literal arg, NO host expr, setter-only.
                format!("$.bind_window_size('{bind_name}', {set_thunk})")
            }
            RuntimeHelper::WindowScroll => {
                // `$.bind_window_scroll('x'|'y', get, set)` — the runtime axis name is
                // REMAPPED from the bind name (`scrollX` → `'x'`, `scrollY` → `'y'`) and the
                // helper is READ-WRITE (get+set). The bind name is a typed contract-row name
                // (one of exactly `scrollX` / `scrollY`), not source text.
                let axis = match bind_name {
                    "scrollX" => "x",
                    "scrollY" => "y",
                    other => unreachable!(
                        "a WindowScroll routing carries only scrollX/scrollY, got `{other}`"
                    ),
                };
                format!("$.bind_window_scroll('{axis}', {get_thunk}, {set_thunk})")
            }
            RuntimeHelper::Online => {
                // `$.bind_online(set)` — setter-only, NO name, NO host expr.
                format!("$.bind_online({set_thunk})")
            }
            RuntimeHelper::Focused => {
                // `$.bind_focused(host, set)` — host expr + setter-only. The host is the
                // element var on a regular element and `$.window` on the window host.
                format!("$.bind_focused({var}, {set_thunk})")
            }
            RuntimeHelper::ActiveElement => {
                // `$.bind_active_element(set)` — the dedicated setter-only helper, NO name,
                // NO host expr (NOT the generic `$.bind_property`).
                format!("$.bind_active_element({set_thunk})")
            }
            RuntimeHelper::This => {
                // `this` never reaches the DOM router: the bind classifier maps the
                // `this` name exclusively to the `This` shape (emitted by the
                // `bind_this` arm of `emit_bind`), so a `DomBind` routing carrying the
                // `This` helper is never produced.
                unreachable!("a `this` bind routes through the This shape, never format_dom_bind")
            }
        }
    }

    /// The HOST EXPRESSION a `bind:` call targets — a regular element's walked DOM var, or
    /// the global host for a special-element bind (`<svelte:window>` ⇒ `$.window`,
    /// `<svelte:document>` ⇒ `$.document`, `<svelte:body>` ⇒ `$.document.body`,
    /// `<svelte:element>` ⇒ the `$$element` callback param). Structural over the typed IR
    /// node, never a name scan.
    fn bind_host_expr(&self, target: NodeId) -> String {
        if let IrNode::Special(s) = self.ir().node(target) {
            match s.kind {
                SpecialKind::Window => return "$.window".to_string(),
                SpecialKind::Document => return "$.document".to_string(),
                SpecialKind::Body => return "$.document.body".to_string(),
                SpecialKind::Element => return "$$element".to_string(),
                _ => {}
            }
        }
        self.dom_var(target)
    }
}
