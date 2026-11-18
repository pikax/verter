//! Attribute strings of element open tags.

use crate::ast::types::{ElementNode, PropFlags};
use crate::code_transform::{AnchorKind, SegmentAnchor};
use crate::template::code_gen::shared::helpers::{SsrHelper, VdomHelper};
use crate::template::oxc::types::OxcParsedElement;
use crate::template::vue_syntax::camelize;

use super::escape::{
    css_to_js_object, escape_js_string, escape_template_literal, html_attr_to_js_key,
    is_boolean_html_attr,
};
use super::props_object::{PropsObject, PropsSlot};
use super::state::{SsrEmitter, SsrOut};

/// The attribute text of an open tag.
pub(super) struct OpenTagAttrs {
    pub(super) text: String,
    /// The authored `class` key inside `text`, when it is carried.
    pub(super) anchor: Option<SegmentAnchor>,
    /// The attributes render through `_ssrRenderAttrs`, which also renders
    /// a textarea's `value`.
    pub(super) through_render_attrs: bool,
}

impl OpenTagAttrs {
    fn plain(text: String) -> Self {
        Self {
            text,
            anchor: None,
            through_render_attrs: false,
        }
    }
}

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// Build SSR attributes string for an element.
    /// For root elements, wraps attrs in `_ssrRenderAttrs()` with `_attrs` merging.
    /// For nested elements without v-bind spread, emits inline per-attribute helpers.
    /// Returns the attrs expression string plus, when the element carries a
    /// plain static `class` attribute merged through the root `_mergeProps`
    /// path (no dynamic `:class`, no `v-bind` spread, no custom directives,
    /// no `_temp0` v-model pattern), the "class" KEY's own authored anchor:
    /// `(offset in the returned string, source byte position of the
    /// `class` attribute's own name)`. `None` in every other
    /// configuration — the caller falls back to an unmapped overwrite.
    pub(super) fn build_attrs_string(
        &mut self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'c>>,
        source: &'c str,
        out: &mut SsrOut<'c>,
        is_root: bool,
    ) -> OpenTagAttrs {
        let tag_name_str = self.tag_name(el, source);
        // When custom directives are present, force the attrs_obj/_mergeProps path
        // even for nested elements (don't use inline per-attr helpers).
        let has_custom_directives = el.prop_flag.has(PropFlags::HasCustomDirective);

        let mut attrs_obj = PropsObject::new();
        let mut has_dynamic_attrs = false;
        let mut has_v_show = false;
        let mut v_show_expr = String::new();
        let mut v_bind_spread_expr = String::new();
        let mut directive_calls: Vec<String> = Vec::new();

        // Pre-scan for v-bind spread so that static class/style processing
        // knows upfront whether to use the mergeProps path (source order
        // matters). Style parts no longer need a v-show pre-scan: static +
        // dynamic + v-show styles ALWAYS fold into the single merged
        // emission.
        let mut has_v_bind_spread = false;
        for p in el.props.iter() {
            if p.is_directive {
                let pn = &source[p.start as usize..p.name_end as usize];
                if pn == "v-bind" && p.arg_start.is_none() {
                    has_v_bind_spread = true;
                }
            }
        }

        // Collect v-model info for this element
        let mut v_model_expr: Option<String> = None;
        let mut v_model_arg: Option<String> = None;
        let mut v_model_prop_idx: usize = 0;
        let mut v_model_is_real = false; // true if from v-model, false if from :value on textarea

        // Per-attribute inline parts (for nested elements without v-bind spread).
        // Stores (prop_index, rendered_attr_string) for source-order interleaving.
        let mut inline_parts: Vec<(usize, String)> = Vec::new();

        // Class merge tracking: when both static `class` and `:class` exist,
        // they are merged into a single `_ssrRenderClass([dynamic, "static"])`.
        let mut static_class_value: Option<String> = None;
        let mut static_class_prop_idx: Option<usize> = None;
        let mut dynamic_class_resolved: Option<String> = None;
        let mut dynamic_class_prop_idx: Option<usize> = None;
        // Source-order position held for the merged `class` entry while its
        // value is still unknown. Exactly one is reserved per element — the
        // first `class`/`:class` occurrence wins the position — so a later
        // occurrence cannot open a second one and duplicate the key.
        let mut class_slot: Option<PropsSlot> = None;

        // Style merge tracking: static style, `:style`, and v-show display
        // always merge into ONE `_ssrRenderStyle(...)` attribute per element
        // (official merges style parts for EVERY element — emitting two
        // `style=` attributes silently drops one, first-wins in browsers).
        let mut dynamic_style_resolved: Option<String> = None;
        let mut dynamic_style_prop_idx: Option<usize> = None;
        let mut static_style_value: Option<String> = None;
        let mut static_style_prop_idx: Option<usize> = None;
        // Source-order position held for the merged `style` entry, reserved
        // once for the same reason as `class_slot`.
        let mut style_slot: Option<PropsSlot> = None;

        // Prepare ref entry for root elements (ref needs source-order insertion)
        let ref_entry: Option<(u32, String)> = if is_root {
            el.v_ref.as_ref().and_then(|v_ref| {
                let (vs, ve) = (v_ref.value_start?, v_ref.value_end?);
                let ref_val = &source[vs as usize..ve as usize];
                let value = if v_ref.is_directive {
                    self.resolve_expr(ref_val, vs, None)
                } else {
                    format!("\"{}\"", escape_js_string(ref_val))
                };
                Some((v_ref.start, value))
            })
        } else {
            None
        };
        let mut ref_emitted = false;

        for (i, prop) in el.props.iter().enumerate() {
            // Emit ref at the correct source position among other props
            if let Some((ref_pos, ref ref_value)) = &ref_entry {
                if !ref_emitted && *ref_pos < prop.start {
                    attrs_obj.push("ref", ref_value.clone());
                    has_dynamic_attrs = true;
                    ref_emitted = true;
                }
            }

            let prop_name = &source[prop.start as usize..prop.name_end as usize];

            if prop.is_directive {
                // Skip events entirely in SSR
                if prop_name.starts_with('@') || prop_name.starts_with("v-on") {
                    continue;
                }

                // v-show
                if prop_name == "v-show" {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_prop = oxc.and_then(|o| o.prop(i));
                        let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                        v_show_expr = self.resolve_expr(expr, vs, oxc_expr);
                        has_v_show = true;
                    }
                    continue;
                }

                // v-bind spread (no argument) — already detected in pre-scan
                if prop_name == "v-bind" && prop.arg_start.is_none() {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_prop = oxc.and_then(|o| o.prop(i));
                        let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                        v_bind_spread_expr = self.resolve_expr(expr, vs, oxc_expr);
                    }
                    continue;
                }

                // v-html and v-text are handled at the content level
                if prop_name == "v-html" || prop_name == "v-text" {
                    continue;
                }

                // v-model: handle based on element type
                if prop_name.starts_with("v-model") {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_prop = oxc.and_then(|o| o.prop(i));
                        let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                        let resolved = self.resolve_expr(expr, vs, oxc_expr);
                        v_model_expr = Some(resolved);
                        v_model_prop_idx = i;
                        v_model_is_real = true;

                        // Check for v-model argument (v-model:title)
                        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                            v_model_arg = Some(source[as_ as usize..ae as usize].to_string());
                        }
                    }
                    continue;
                }

                // v-memo, v-cloak — skip in SSR
                if prop_name == "v-memo" || prop_name == "v-cloak" {
                    continue;
                }

                // Dynamic bindings (:attr or v-bind:attr)
                if prop_name.starts_with(':') || (prop_name == "v-bind" && prop.arg_start.is_some())
                {
                    if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                        let attr_name = &source[as_ as usize..ae as usize];
                        let is_dynamic_name = prop.is_dynamic == Some(true);
                        // :key and :ref are client-only, skip in SSR
                        if attr_name == "key" || attr_name == "ref" {
                            continue;
                        }
                        // :value on <textarea>: skip from attrs, rendered as content
                        if attr_name == "value" && tag_name_str == "textarea" {
                            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                let expr = &source[vs as usize..ve as usize];
                                let oxc_prop = oxc.and_then(|o| o.prop(i));
                                let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                                v_model_expr = Some(self.resolve_expr(expr, vs, oxc_expr));
                                v_model_prop_idx = i;
                            }
                            continue;
                        }
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| o.prop(i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            let resolved = self.resolve_expr(expr, vs, oxc_expr);

                            // Dynamic attribute names (:[expr]) must go through _ssrRenderAttrs
                            // with a computed property key: { [resolvedExpr || ""]: value }
                            if is_dynamic_name {
                                // Strip brackets from arg span to get raw expression
                                let raw_arg = attr_name
                                    .strip_prefix('[')
                                    .unwrap_or(attr_name)
                                    .strip_suffix(']')
                                    .unwrap_or(attr_name);
                                // Resolve the dynamic name expression
                                // as_ points to '[', but raw_arg starts after '[', so offset + 1
                                let oxc_arg = oxc_prop.and_then(|p| p.arg.as_ref());
                                let resolved_name = self.resolve_expr(raw_arg, as_ + 1, oxc_arg);
                                // Build computed key: [resolvedExpr || ""]
                                let computed_key = format!("[{} || \"\"]", resolved_name);
                                attrs_obj.push(computed_key, resolved);
                                has_dynamic_attrs = true;
                                continue;
                            }

                            // For nested elements without v-bind spread or custom directives,
                            // use inline helpers. Custom directives force the _mergeProps path.
                            if !is_root && !has_v_bind_spread && !has_custom_directives {
                                if attr_name == "class" {
                                    // Save for potential merge with static class
                                    dynamic_class_resolved = Some(resolved);
                                    dynamic_class_prop_idx = Some(i);
                                    has_dynamic_attrs = true;
                                    continue;
                                } else if attr_name == "style" {
                                    // ALWAYS defer to the single merged
                                    // _ssrRenderStyle emission (static +
                                    // dynamic + v-show fold into one
                                    // attribute).
                                    dynamic_style_resolved = Some(resolved);
                                    dynamic_style_prop_idx = Some(i);
                                    has_dynamic_attrs = true;
                                    continue;
                                } else if is_boolean_html_attr(attr_name) {
                                    out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
                                    inline_parts.push((
                                        i,
                                        format!(
                                            "${{(_ssrIncludeBooleanAttr({})) ? \" {}\" : \"\"}}",
                                            resolved, attr_name
                                        ),
                                    ));
                                } else {
                                    out.add_ssr_import(SsrHelper::RenderAttr);
                                    inline_parts.push((
                                        i,
                                        format!(
                                            "${{_ssrRenderAttr(\"{}\", {})}}",
                                            attr_name, resolved
                                        ),
                                    ));
                                }
                                has_dynamic_attrs = true;
                                continue;
                            }

                            // Track dynamic class/style for root-path merge
                            if attr_name == "class" {
                                dynamic_class_resolved = Some(resolved);
                                dynamic_class_prop_idx = Some(i);
                                has_dynamic_attrs = true;
                                // Hold this source-order position for the merged value
                                class_slot.get_or_insert_with(|| attrs_obj.reserve());
                            } else if attr_name == "style" {
                                // Defer to static+dynamic style merge (array form,
                                // source order). Emitting a second `style:` key would
                                // drop the static style (last-wins).
                                dynamic_style_resolved = Some(resolved);
                                dynamic_style_prop_idx = Some(i);
                                has_dynamic_attrs = true;
                                style_slot.get_or_insert_with(|| attrs_obj.reserve());
                            } else {
                                attrs_obj.push(html_attr_to_js_key(attr_name), resolved);
                                has_dynamic_attrs = true;
                            }
                        } else {
                            // Same-name shorthand (Vue 3.4+): `:class` ≡ `:class="class"`
                            // The value expression is the arg name itself.
                            // Vue always uses dot notation (_ctx.class) even for keywords.
                            // For hyphenated names (:heading-value), camelize to headingValue
                            // before binding lookup — bindings use camelCase keys and the
                            // raw hyphenated form would produce subtraction in JS output.
                            let camelized = camelize(attr_name);
                            let prefix = self.bindings.resolve_prefix(&camelized);
                            let resolved = if prefix.is_empty() {
                                format!("_ctx.{}", camelized)
                            } else {
                                format!("{}{}", prefix, camelized)
                            };

                            if !is_root && !has_v_bind_spread && !has_custom_directives {
                                if attr_name == "class" {
                                    dynamic_class_resolved = Some(resolved);
                                    dynamic_class_prop_idx = Some(i);
                                    has_dynamic_attrs = true;
                                    continue;
                                } else if attr_name == "style" {
                                    // Defer to the single merged style emission.
                                    dynamic_style_resolved = Some(resolved);
                                    dynamic_style_prop_idx = Some(i);
                                    has_dynamic_attrs = true;
                                    continue;
                                } else if is_boolean_html_attr(attr_name) {
                                    out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
                                    inline_parts.push((
                                        i,
                                        format!(
                                            "${{(_ssrIncludeBooleanAttr({})) ? \" {}\" : \"\"}}",
                                            resolved, attr_name
                                        ),
                                    ));
                                } else {
                                    out.add_ssr_import(SsrHelper::RenderAttr);
                                    inline_parts.push((
                                        i,
                                        format!(
                                            "${{_ssrRenderAttr(\"{}\", {})}}",
                                            attr_name, resolved
                                        ),
                                    ));
                                }
                                has_dynamic_attrs = true;
                                continue;
                            }

                            // Root/spread path
                            if attr_name == "class" {
                                dynamic_class_resolved = Some(resolved);
                                dynamic_class_prop_idx = Some(i);
                                has_dynamic_attrs = true;
                                class_slot.get_or_insert_with(|| attrs_obj.reserve());
                            } else {
                                attrs_obj.push(html_attr_to_js_key(attr_name), resolved);
                                has_dynamic_attrs = true;
                            }
                        }
                    }
                    continue;
                }

                // Other custom directives — resolve and merge via _ssrGetDirectiveProps
                if prop_name.starts_with("v-") {
                    if let Some(call) =
                        self.build_directive_props_call(prop, prop_name, source, oxc, i, out)
                    {
                        directive_calls.push(call);
                        has_dynamic_attrs = true;
                    }
                }
                continue;
            }

            // Static attributes
            let attr_name = prop_name;
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];

                // Track static class for potential merge with :class
                // Trim trailing whitespace (Vue does this)
                if attr_name == "class" {
                    static_class_value = Some(value.trim_end().to_string());
                    static_class_prop_idx = Some(i);
                    if is_root || has_v_bind_spread || has_custom_directives {
                        // Hold this source-order position; the class merge section
                        // below fills it with the final merged value.
                        class_slot.get_or_insert_with(|| attrs_obj.reserve());
                        continue;
                    }
                    // For inline path: let it continue to attrs_obj (harmless, attrs_obj
                    // isn't used when inline path returns early).
                }

                // Track static style for potential merge with :style on root path
                if attr_name == "style" && (is_root || has_v_bind_spread || has_custom_directives) {
                    static_style_value = Some(css_to_js_object(value));
                    static_style_prop_idx = Some(i);
                    style_slot.get_or_insert_with(|| attrs_obj.reserve());
                    continue;
                }

                let js_attr = html_attr_to_js_key(attr_name);
                if attr_name == "style" {
                    // Vue SSR converts static style strings to JS objects in mergeProps
                    attrs_obj.push(js_attr, css_to_js_object(value));
                } else {
                    attrs_obj.push(js_attr, format!("\"{}\"", escape_js_string(value)));
                }
            } else {
                // Boolean attribute (e.g., `disabled`)
                attrs_obj.push(html_attr_to_js_key(attr_name), "\"\"");
            }
        }

        // Merge static + dynamic class into a single expression. On the root
        // path the merged value goes into the position reserved at the first
        // `class`/`:class` occurrence, so source order is preserved.
        match (&dynamic_class_resolved, &static_class_value) {
            (Some(dyn_expr), Some(static_cls)) => {
                if !is_root && !has_v_bind_spread && !has_custom_directives {
                    // Inline path: merged _ssrRenderClass call (Vue order: dynamic first)
                    out.add_ssr_import(SsrHelper::RenderClass);
                    let prop_idx =
                        dynamic_class_prop_idx.unwrap_or(static_class_prop_idx.unwrap_or(0));
                    inline_parts.push((
                        prop_idx,
                        format!(
                            " class=\"${{_ssrRenderClass([{}, \"{}\"])}}\"",
                            dyn_expr,
                            escape_js_string(static_cls),
                        ),
                    ));
                    has_dynamic_attrs = true;
                } else {
                    // Root/attrs_obj path: class: ["static", dynamic] (Vue order: static first)
                    attrs_obj.fill_or_push(
                        class_slot,
                        "class",
                        format!("[\"{}\", {}]", escape_js_string(static_cls), dyn_expr),
                    );
                    has_dynamic_attrs = true;
                }
            }
            (Some(dyn_expr), None) => {
                if !is_root && !has_v_bind_spread && !has_custom_directives {
                    // Inline path: dynamic only
                    out.add_ssr_import(SsrHelper::RenderClass);
                    let prop_idx = dynamic_class_prop_idx.unwrap_or(0);
                    inline_parts.push((
                        prop_idx,
                        format!(" class=\"${{_ssrRenderClass({})}}\"", dyn_expr),
                    ));
                    has_dynamic_attrs = true;
                } else {
                    // Root/attrs_obj path: dynamic class only
                    attrs_obj.fill_or_push(class_slot, "class", dyn_expr.clone());
                    has_dynamic_attrs = true;
                }
            }
            (None, Some(static_cls)) => {
                if is_root || has_v_bind_spread || has_custom_directives {
                    // Root/mergeProps path: put static class into attrs_obj and
                    // mark its key as the anchor. The rendered offset of that
                    // key is reported by `attrs_obj.render()` from the entry's
                    // own position, so no assembled text is searched for it.
                    let value = format!("\"{}\"", escape_js_string(static_cls));
                    match static_class_prop_idx {
                        Some(idx) => attrs_obj.fill_or_push_anchored(
                            class_slot,
                            "class",
                            value,
                            el.props[idx].start,
                        ),
                        None => attrs_obj.fill_or_push(class_slot, "class", value),
                    }
                }
                // Inline path: static class will be rendered as literal HTML
                // in the source-order loop (it wasn't skipped from props).
            }
            (None, None) => {}
        }

        // Merge static + dynamic style into a single expression for the root /
        // mergeProps path (mirrors class merge). Official form, SOURCE order:
        //   style: [{ "margin": "1px" }, _ctx.sty]
        match (&dynamic_style_resolved, &static_style_value) {
            (Some(dyn_expr), Some(stat_obj))
                if is_root || has_v_bind_spread || has_custom_directives =>
            {
                // Authoring order decides the array order (official merges
                // attribute parts in source order).
                let static_first = static_style_prop_idx.unwrap_or(usize::MAX)
                    <= dynamic_style_prop_idx.unwrap_or(usize::MAX);
                let style_value = if static_first {
                    format!("[{}, {}]", stat_obj, dyn_expr)
                } else {
                    format!("[{}, {}]", dyn_expr, stat_obj)
                };
                attrs_obj.fill_or_push(style_slot, "style", style_value);
                has_dynamic_attrs = true;
            }
            (Some(dyn_expr), None) if is_root || has_v_bind_spread || has_custom_directives => {
                attrs_obj.fill_or_push(style_slot, "style", dyn_expr.clone());
                has_dynamic_attrs = true;
            }
            (None, Some(stat_obj)) if is_root || has_v_bind_spread || has_custom_directives => {
                attrs_obj.fill_or_push(style_slot, "style", stat_obj.clone());
            }
            _ => {
                // No style value to emit. A reserved slot that is never filled
                // renders nothing, so nothing has to be cleaned up here.
            }
        }

        // Handle v-model attrs injection
        if let Some(ref model_expr) = v_model_expr {
            let prop_name = v_model_arg.as_deref().unwrap_or("value");
            let input_type = self.get_input_type(el, source);

            match tag_name_str {
                "textarea" => {
                    if v_model_is_real {
                        // Real v-model on textarea: content is always interpolated
                        // (both root and non-root). Do NOT add value to attrs.
                    } else {
                        // :value on textarea: for root path, add value to attrs_obj.
                        // For non-root, content interpolation handles it.
                        if is_root || has_v_bind_spread || has_custom_directives {
                            attrs_obj.push(prop_name, model_expr.clone());
                            has_dynamic_attrs = true;
                        }
                    }
                }
                "select" => {
                    // select: no attr changes needed on <select> itself;
                    // store v-model expr so child <option> elements can inject `selected`
                    self.select_v_model_expr = Some(model_expr.clone());
                }
                "input" => {
                    // For root input elements, skip adding explicit value/checked to
                    // attrs_obj — _ssrGetDynamicModelProps determines the correct prop
                    // at runtime based on the merged type (which may come from _attrs).
                    if is_root {
                        has_dynamic_attrs = true;
                    } else if !has_v_bind_spread && !has_custom_directives {
                        // Non-root inline path
                        match input_type {
                            Some("checkbox") => {
                                out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
                                out.add_ssr_import(SsrHelper::LooseContain);
                                inline_parts.push((
                                    v_model_prop_idx,
                                    format!(
                                        "${{(_ssrIncludeBooleanAttr(\
                                         (Array.isArray({expr})) \
                                         ? _ssrLooseContain({expr}, null) \
                                         : {expr})) \
                                         ? \" checked\" : \"\"}}",
                                        expr = model_expr
                                    ),
                                ));
                                has_dynamic_attrs = true;
                            }
                            Some("radio") => {
                                let radio_value = self.get_option_value(el, oxc, source);
                                out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
                                out.add_ssr_import(SsrHelper::LooseEqual);
                                inline_parts.push((
                                    v_model_prop_idx,
                                    format!(
                                        "${{(_ssrIncludeBooleanAttr(\
                                         _ssrLooseEqual({}, {}))) \
                                         ? \" checked\" : \"\"}}",
                                        model_expr, radio_value
                                    ),
                                ));
                                has_dynamic_attrs = true;
                            }
                            _ => {
                                out.add_ssr_import(SsrHelper::RenderAttr);
                                inline_parts.push((
                                    v_model_prop_idx,
                                    format!(
                                        "${{_ssrRenderAttr(\"{}\", {})}}",
                                        prop_name, model_expr
                                    ),
                                ));
                                has_dynamic_attrs = true;
                            }
                        }
                    } else {
                        // Non-root with v-bind spread or custom directives:
                        // add to attrs_obj (mergeProps path)
                        match input_type {
                            Some("checkbox") => {
                                out.add_ssr_import(SsrHelper::LooseContain);
                                attrs_obj.push(
                                    "checked",
                                    format!(
                                        "(Array.isArray({expr}) \
                                         ? _ssrLooseContain({expr}, null) \
                                         : {expr})",
                                        expr = model_expr
                                    ),
                                );
                                has_dynamic_attrs = true;
                            }
                            Some("radio") => {
                                let radio_value = self.get_option_value(el, oxc, source);
                                out.add_ssr_import(SsrHelper::LooseEqual);
                                attrs_obj.push(
                                    "checked",
                                    format!("_ssrLooseEqual({}, {})", model_expr, radio_value),
                                );
                                has_dynamic_attrs = true;
                            }
                            _ => {
                                attrs_obj.push(prop_name, model_expr.clone());
                                has_dynamic_attrs = true;
                            }
                        }
                    }
                }
                _ => {
                    // Component v-model or other elements
                    attrs_obj.push(prop_name, model_expr.clone());
                    has_dynamic_attrs = true;
                }
            }
        }

        // <option> inside <select v-model>: inject `selected` attribute check
        let option_selected_suffix = if tag_name_str == "option" {
            if let Some(ref model_expr) = self.select_v_model_expr {
                // Determine the option's value expression
                let option_value = self.get_option_value(el, oxc, source);
                out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
                out.add_ssr_import(SsrHelper::LooseContain);
                out.add_ssr_import(SsrHelper::LooseEqual);
                format!(
                    "${{(_ssrIncludeBooleanAttr((Array.isArray({model})) \
                     ? _ssrLooseContain({model}, {val}) \
                     : _ssrLooseEqual({model}, {val}))) ? \" selected\" : \"\"}}",
                    model = model_expr,
                    val = option_value
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // For nested elements using inline mode (has inline_parts, deferred
        // merged style, or v-show, and no v-bind spread). When
        // directive_calls are present, skip inline mode and use the
        // _mergeProps path instead.
        if !is_root
            && !has_v_bind_spread
            && directive_calls.is_empty()
            && (!inline_parts.is_empty()
                || has_v_show
                || dynamic_style_resolved.is_some()
                || (self.is_css_vars_root() && self.has_css_vars()))
        {
            // Build attrs in source order: interleave static and dynamic parts
            let mut dynamic_map: std::collections::HashMap<usize, &str> =
                inline_parts.iter().map(|(i, s)| (*i, s.as_str())).collect();
            // Track which prop indices were consumed by the class merge
            let class_merged_static_idx = if dynamic_class_resolved.is_some() {
                static_class_prop_idx
            } else {
                None
            };
            let mut result = String::new();
            for (i, prop) in el.props.iter().enumerate() {
                if let Some(dynamic_part) = dynamic_map.remove(&i) {
                    result.push_str(dynamic_part);
                } else if Some(i) == class_merged_static_idx {
                    // Static class was merged into the _ssrRenderClass call — skip
                    continue;
                } else if Some(i) == static_style_prop_idx {
                    // Static style was merged into the _ssrRenderStyle call — skip
                    continue;
                } else if !prop.is_directive {
                    // Static attr: render as literal HTML (same as build_literal_attrs)
                    let name = &source[prop.start as usize..prop.name_end as usize];
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let value = &source[vs as usize..ve as usize];
                        if name == "style" {
                            // ALWAYS merged into the single combined
                            // _ssrRenderStyle emission below.
                            static_style_value = Some(css_to_js_object(value));
                            static_style_prop_idx = Some(i);
                            continue;
                        } else {
                            // Vue trims class attribute values (trailing whitespace)
                            let emit_value = if name == "class" {
                                value.trim_end()
                            } else {
                                value
                            };
                            result.push(' ');
                            result.push_str(name);
                            result.push_str("=\"");
                            result.push_str(&escape_template_literal(emit_value));
                            result.push('"');
                        }
                    } else {
                        result.push(' ');
                        result.push_str(name);
                    }
                }
            }
            // Merge all style parts (v-show, :style, static style) into a single attribute
            let v_show_style = if has_v_show {
                Some(format!(
                    "({}) ? null : {{ display: \"none\" }}",
                    v_show_expr
                ))
            } else {
                None
            };
            // Root-level elements of a MULTI-ROOT template (is_root=false,
            // css-vars-root=true) carry the CSS v-bind custom properties as
            // a style part — official injects on every root-level node.
            let css_vars_style = (self.is_css_vars_root() && self.has_css_vars())
                .then(|| "_cssVars.style".to_string());
            let has_any_style = dynamic_style_resolved.is_some()
                || static_style_value.is_some()
                || v_show_style.is_some()
                || css_vars_style.is_some();
            if has_any_style {
                out.add_ssr_import(SsrHelper::RenderStyle);
                // SOURCE order between static style and :style (official
                // merges attribute parts in authoring order); the v-show
                // display entry always comes last.
                let mut ordered: Vec<(usize, String)> = Vec::new();
                if let Some(ref dyn_style) = dynamic_style_resolved {
                    ordered.push((
                        dynamic_style_prop_idx.unwrap_or(usize::MAX),
                        dyn_style.clone(),
                    ));
                }
                if let Some(ref stat_style) = static_style_value {
                    ordered.push((
                        static_style_prop_idx.unwrap_or(usize::MAX),
                        stat_style.clone(),
                    ));
                }
                ordered.sort_by_key(|(idx, _)| *idx);
                let mut style_parts: Vec<String> =
                    ordered.into_iter().map(|(_, part)| part).collect();
                if let Some(vshow) = v_show_style {
                    style_parts.push(vshow);
                }
                if let Some(css_vars) = css_vars_style {
                    style_parts.push(css_vars);
                }
                if style_parts.len() == 1 {
                    result.push_str(&format!(
                        " style=\"${{_ssrRenderStyle({})}}\"",
                        style_parts[0]
                    ));
                } else {
                    result.push_str(&format!(
                        " style=\"${{_ssrRenderStyle([{}])}}\"",
                        style_parts.join(", ")
                    ));
                }
            }
            result.push_str(&option_selected_suffix);
            return OpenTagAttrs::plain(result);
        }

        // Add ref to attrs for root _mergeProps path if not yet emitted during prop loop
        // (ref appears after all other props in source order)
        if let Some((_, ref ref_value)) = &ref_entry {
            if !ref_emitted {
                attrs_obj.push("ref", ref_value.clone());
                has_dynamic_attrs = true;
            }
        }

        // Build the final expression (existing path for root/v-bind-spread)
        if !is_root && !has_dynamic_attrs && !has_v_show && !has_v_bind_spread {
            let mut attrs = self.build_literal_attrs(el, source, out);
            attrs.push_str(&option_selected_suffix);
            return OpenTagAttrs::plain(attrs);
        }

        let mut parts: Vec<String> = Vec::new();

        if has_v_bind_spread {
            parts.push(v_bind_spread_expr);
        }

        // The one emission of the attrs object; the anchor is the "class" key's
        // offset inside the rendered body, reported by the same pass.
        let (attrs_obj_body, class_key_anchor_in_attrs_obj) = attrs_obj.render();
        if !attrs_obj_body.is_empty() {
            parts.push(format!("{{ {} }}", attrs_obj_body));
        }

        if is_root
            && (has_v_show
                || !parts.is_empty()
                || has_v_bind_spread
                || !directive_calls.is_empty()
                || self.has_css_vars())
        {
            parts.push("_attrs".to_string());
        }

        if has_v_show {
            parts.push(format!(
                "{{ style: ({}) ? null : {{ display: \"none\" }} }}",
                v_show_expr
            ));
        }

        // Custom directive props
        for dir_call in &directive_calls {
            parts.push(dir_call.clone());
        }

        // Merge SSR CSS v-bind custom properties onto every ROOT-LEVEL
        // element — official injects on each multi-root sibling and each
        // root branch, not only the single-root case.
        if self.is_css_vars_root() && self.has_css_vars() {
            parts.push("_cssVars".to_string());
        }

        // v-model on root input: use _temp0 pattern for _ssrGetDynamicModelProps.
        // Vue first merges all props + _attrs into _temp0, then passes _temp0 to
        // _ssrGetDynamicModelProps which determines the correct prop at runtime.
        let use_temp0 = is_root && v_model_is_real && tag_name_str == "input";

        // Vue passes the tag name as second arg to _ssrRenderAttrs for textarea
        // so the renderer knows to skip the `value` attribute (rendered as content).
        let tag_arg = if tag_name_str == "textarea" {
            r#", "textarea""#
        } else {
            ""
        };

        // The "class" key's anchor is carried through ONLY for the specific
        // shape this function's own doc comment names: `is_root`, the
        // `_mergeProps` branch below, and `attrs_obj` (carrying the class
        // entry) as `parts[0]` — which requires `!has_v_bind_spread` (a
        // spread expression would occupy `parts[0]` instead). Every other
        // configuration keeps returning `None`, matching this call site's
        // pre-existing (unmapped) behavior exactly.
        let mut attrs_anchor: Option<SegmentAnchor> = None;

        let base = if use_temp0 {
            if let Some(ref model_expr) = v_model_expr {
                self.temp_var_needed = true;
                out.add_ssr_import(SsrHelper::RenderAttrs);
                out.add_ssr_import(SsrHelper::GetDynamicModelProps);
                out.add_vdom_import(VdomHelper::MergeProps);
                // Build: (_temp0 = _mergeProps(parts..., _attrs), _mergeProps(_temp0, _ssrGetDynamicModelProps(_temp0, expr)))
                let first_merge = format!("_mergeProps({})", parts.join(", "));
                format!(
                    "${{_ssrRenderAttrs((_temp0 = {}, _mergeProps(_temp0, _ssrGetDynamicModelProps(_temp0, {}))))}}",
                    first_merge, model_expr
                )
            } else {
                // Shouldn't happen — v_model_is_real implies v_model_expr is Some
                String::new()
            }
        } else if is_root {
            out.add_ssr_import(SsrHelper::RenderAttrs);

            if parts.is_empty() {
                format!("${{_ssrRenderAttrs(_attrs{})}}", tag_arg)
            } else {
                out.add_vdom_import(VdomHelper::MergeProps);
                const PREFIX: &str = "${_ssrRenderAttrs(_mergeProps(";
                if let (Some((offset_in_attrs_obj, source_pos)), false) =
                    (class_key_anchor_in_attrs_obj, has_v_bind_spread)
                {
                    // `attrs_obj` is `parts[0]`, wrapped as `"{ " + attrs_obj + " }"`.
                    attrs_anchor = Some(SegmentAnchor {
                        content_offset: PREFIX.len() as u32 + 2 + offset_in_attrs_obj,
                        length: "class".len() as u32,
                        source_pos,
                        kind: AnchorKind::Exact,
                    });
                }
                format!(
                    "${{_ssrRenderAttrs(_mergeProps({}){})}}",
                    parts.join(", "),
                    tag_arg,
                )
            }
        } else if parts.is_empty() {
            String::new()
        } else if parts.len() == 1 && !has_v_show {
            out.add_ssr_import(SsrHelper::RenderAttrs);
            format!("${{_ssrRenderAttrs({}{})}}", parts[0], tag_arg)
        } else {
            out.add_ssr_import(SsrHelper::RenderAttrs);
            out.add_vdom_import(VdomHelper::MergeProps);
            format!(
                "${{_ssrRenderAttrs(_mergeProps({}){})}}",
                parts.join(", "),
                tag_arg,
            )
        };
        let through_render_attrs = !base.is_empty();
        let text = if option_selected_suffix.is_empty() {
            base
        } else {
            format!("{}{}", base, option_selected_suffix)
        };
        OpenTagAttrs {
            text,
            anchor: attrs_anchor,
            through_render_attrs,
        }
    }

    /// Build literal HTML attributes for a nested (non-root) static element.
    /// Static `style` attributes are wrapped in `_ssrRenderStyle()` to match Vue.
    pub(super) fn build_literal_attrs(
        &self,
        el: &ElementNode,
        source: &str,
        out: &mut SsrOut<'c>,
    ) -> String {
        let mut result = String::new();
        for prop in &el.props {
            if prop.is_directive {
                continue;
            }
            let name = &source[prop.start as usize..prop.name_end as usize];
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                if name == "style" {
                    // Vue wraps static styles in _ssrRenderStyle() with JS object
                    out.add_ssr_import(SsrHelper::RenderStyle);
                    result.push_str(&format!(
                        " style=\"${{_ssrRenderStyle({})}}\"",
                        css_to_js_object(value)
                    ));
                } else {
                    // Vue trims class attribute values (trailing whitespace)
                    let emit_value = if name == "class" {
                        value.trim_end()
                    } else {
                        value
                    };
                    result.push(' ');
                    result.push_str(name);
                    result.push_str("=\"");
                    result.push_str(&escape_template_literal(emit_value));
                    result.push('"');
                }
            } else {
                result.push(' ');
                result.push_str(name);
            }
        }
        result
    }
}
