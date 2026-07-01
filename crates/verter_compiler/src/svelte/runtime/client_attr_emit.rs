//! The dynamic-ATTRIBUTE / `class` / `style` `ClientEmitter` write helpers, extracted from
//! `client.rs` to keep the emitter core under the file-size guard.
//!
//! These build the coalesced `$.set_attribute` / property-write / `$.autofocus` /
//! `$.set_class` / `$.set_style` call bodies from the structured op pieces, routing each
//! value through the shared memoizer.

use super::client::ClientEmitter;
use super::client_effect::Memoizer;
use super::client_plan_types::{AttrValue, ClientDynAttrEmit};
use super::ir::NodeId;

impl<'a> ClientEmitter<'a> {
    /// Emit a dynamic plain-attribute write body (`$.set_attribute(node, 'name',
    /// value)` / `node.<prop> = value` / `$.autofocus(node, value)`), resolving the
    /// node var and building the structured value.
    ///
    /// `memoizer` is `Some` on the REACTIVE (in-effect) path: a `has_call` expression
    /// part is hoisted into a `$N` deps-array slot (the official `build_template_chunk`
    /// rule). It is `None` on the INIT path (`$.autofocus` / a non-reactive write),
    /// where the value is read once and is emitted INLINE with no memoization.
    pub(super) fn emit_reactive_attr(
        &self,
        target: NodeId,
        emit: &ClientDynAttrEmit,
        memoizer: &mut Option<&mut Memoizer>,
    ) -> String {
        let var = self.dom_var(target);
        match emit {
            ClientDynAttrEmit::SetAttribute { name, value } => {
                let v = self.build_attr_value(value, memoizer);
                format!("$.set_attribute({var}, '{name}', {v})")
            }
            ClientDynAttrEmit::Property { prop, value } => {
                let v = self.build_attr_value(value, memoizer);
                format!("{var}.{prop} = {v}")
            }
            ClientDynAttrEmit::Autofocus { value } => {
                // Autofocus is init-only — its value is a pre-flattened string (never
                // memoized).
                format!("$.autofocus({var}, {value})")
            }
        }
    }

    /// Memoize a class/style ARGUMENT (the base `value` or the `next` directives
    /// object/array) when it `has_call` and the op is reactive (`memoizer` is `Some`) —
    /// the official `build_set_class` / `build_set_style` rule. On the init path
    /// (`memoizer` is `None`) the argument is emitted inline. A non-`has_call` argument
    /// always stays inline.
    fn memoize_arg(
        &self,
        arg: &str,
        has_call: bool,
        memoizer: &mut Option<&mut Memoizer>,
    ) -> String {
        match memoizer {
            Some(m) => m.add(arg.to_string(), has_call),
            None => arg.to_string(),
        }
    }

    /// Assemble the coalesced `$.set_class(node, 1, value, css_hash, prev, next)` call
    /// body from the structured op pieces, with the real DOM var + accumulator name —
    /// the regular-element (`is_html = 1`) wrapper over [`Self::assemble_set_class`].
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_set_class(
        &self,
        target: NodeId,
        value: &AttrValue,
        css_hash: Option<&str>,
        directives: Option<&str>,
        directives_has_call: bool,
        acc: Option<&str>,
        memoizer: &mut Option<&mut Memoizer>,
    ) -> String {
        let var = self.dom_var(target);
        self.assemble_set_class(
            var,
            true,
            value,
            css_hash,
            directives,
            directives_has_call,
            acc,
            memoizer,
        )
    }

    /// Assemble a coalesced `$.set_class(<host>, <is_html>, value, css_hash, prev,
    /// next)` call body from the structured pieces against an arbitrary HOST expression
    /// — the single `$.set_class` assembly for BOTH the regular-element op (the DOM var,
    /// `is_html = 1`) and the `<svelte:element>` lone-class fast path (the `$$element`
    /// callback param, `is_html = 0`). `prev` is the accumulator name (reactive
    /// directives), `{}` (non-reactive directives), or absent (no directives); a
    /// reactive directive call prefixes the `<acc> = ` assignment. The base `value` is
    /// routed through `build_attr_value` (so a mixed base memoizes each EXPRESSION PART,
    /// a `$.clsx(...)` base memoizes the whole wrap — the official `build_set_class`);
    /// the directives object is memoized as a whole through `memoizer` when it
    /// `has_call`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn assemble_set_class(
        &self,
        host: String,
        is_html: bool,
        value: &AttrValue,
        css_hash: Option<&str>,
        directives: Option<&str>,
        directives_has_call: bool,
        acc: Option<&str>,
        memoizer: &mut Option<&mut Memoizer>,
    ) -> String {
        let value = self.build_attr_value(value, memoizer);
        // `prev`: the accumulator name (reactive) or `{}` (non-reactive); present only
        // when there are directives (the same condition that produced `css_hash`).
        let prev = directives.map(|_| acc.map(str::to_string).unwrap_or_else(|| "{}".to_string()));
        let next = directives.map(|d| self.memoize_arg(d, directives_has_call, memoizer));
        let args = super::client_codegen_helpers::trim_trailing_none(vec![
            Some(host),
            Some(if is_html { "1" } else { "0" }.to_string()),
            Some(value),
            css_hash.map(str::to_string),
            prev,
            next,
        ]);
        let call = format!("$.set_class({})", args.join(", "));
        match acc {
            Some(name) => format!("{name} = {call}"),
            None => call,
        }
    }

    /// Assemble the coalesced `$.set_style(node, value, prev, next)` call body from the
    /// structured op pieces (see [`Self::emit_set_class`]).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_set_style(
        &self,
        target: NodeId,
        value: &AttrValue,
        directives: Option<&str>,
        directives_has_call: bool,
        acc: Option<&str>,
        memoizer: &mut Option<&mut Memoizer>,
    ) -> String {
        let var = self.dom_var(target);
        let value = self.build_attr_value(value, memoizer);
        let prev = directives.map(|_| acc.map(str::to_string).unwrap_or_else(|| "{}".to_string()));
        let next = directives.map(|d| self.memoize_arg(d, directives_has_call, memoizer));
        let args = super::client_codegen_helpers::trim_trailing_none(vec![
            Some(var),
            Some(value),
            prev,
            next,
        ]);
        let call = format!("$.set_style({})", args.join(", "));
        match acc {
            Some(name) => format!("{name} = {call}"),
            None => call,
        }
    }
}
