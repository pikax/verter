//! The typed assignment/update TARGET-LVALUE classification of the fallible Svelte
//! client expression rewriter — the [`ClientLvalue`] arms every write target lowers
//! through (a signal ident, a prop setter, a plain ident, an IMPORTED binding
//! (non-reassignable — the official `constant_assignment` reject), a member deep
//! write, a TS-wrapped reactive target, a destructuring target). A SECOND `impl`
//! block of [`BindingOccurrenceCollector`](super::plan::BindingOccurrenceCollector),
//! extracted from `plan.rs` (the file-size guard boundary); the occurrence
//! recording + edit planning stay there.

use oxc_ast::ast::{AssignmentTarget, Expression, SimpleAssignmentTarget};

use super::super::expr::BindingRuntimeKind;
use super::plan::BindingOccurrenceCollector;
use super::plan_render::expression_contains_ts_only_syntax;
use super::{ClientLvalue, PropRead};

impl BindingOccurrenceCollector<'_> {
    /// Whether `name` resolves (scope-awarely) to an IMPORTED binding (a component
    /// default or any other imported value) — a NON-reassignable lvalue root.
    fn is_import_root(&self, name: &str) -> bool {
        matches!(self.signal_kind(name), Some(k) if super::super::expr::is_import_binding(k))
    }

    /// The typed PROP lvalue of a bare-identifier target resolving to a `$props()`
    /// prop binding: a PROP-SOURCE local (a `Getter` read — declared via
    /// `$.prop`) writes through the setter (`PropSetter`); a NON-source prop
    /// target is a projection divergence (a write makes the prop a source, so
    /// its recorded read form must already be the getter) and fails closed
    /// defensively. `None` for a non-prop binding.
    fn prop_lvalue(&self, name: &str) -> Option<ClientLvalue> {
        if !matches!(
            self.signal_kind(name),
            Some(BindingRuntimeKind::Prop | BindingRuntimeKind::BindableProp)
        ) {
            return None;
        }
        Some(match self.ctx.prop_reads.get(name) {
            Some(PropRead::Getter) => ClientLvalue::PropSetter {
                name: name.to_string(),
            },
            _ => ClientLvalue::UnsupportedReactiveTarget,
        })
    }

    /// Whether a MEMBER write target — the WHOLE lvalue: its object chain AND
    /// every computed KEY along it (`computed_key` is the target's own
    /// outermost key, which sits outside the object spine) — BOTH carries a
    /// TS-only wrapper (`v!` / `v as T` / `v satisfies T` / `<T>v` / `v<T>`,
    /// at any depth inside a key) anywhere on the walk down to its root
    /// identifier AND roots at a `$props()` prop (plain or bindable). Such a
    /// write must FAIL CLOSED: official svelte@5.56.3 parse-rejects TypeScript
    /// syntax in a plain-`<script>` component's template (`expected_token`),
    /// and accepting it would emit a corrupted prop write (the
    /// TypeScript-strip pass cannot strip under an update target, and the
    /// bindable mutation-wrap walk does not peel TS wrappers — `v!.a++` would
    /// emit the invalid `v()!.a++` with no `v(…, true)` wrap; a TS-wrapped
    /// KEY leaks the same way, `v[k!]++` → the invalid `v()[k()!]++`). A
    /// chain rooting at a NON-prop binding (a `$state` proxy, a raw signal, a
    /// plain local) keeps its existing `Member` classification — this
    /// predicate scopes the fail-close to the prop write surfaces, never
    /// widening it. Structural over the parsed OXC nodes; shadow-aware via
    /// [`Self::signal_kind`]; the key inspection is the recursive
    /// [`expression_contains_ts_only_syntax`] walk.
    fn member_lvalue_is_ts_wrapped_prop_chain(
        &self,
        mut object: &Expression<'_>,
        computed_key: Option<&Expression<'_>>,
    ) -> bool {
        let mut saw_ts = computed_key.is_some_and(expression_contains_ts_only_syntax);
        loop {
            match object {
                Expression::ParenthesizedExpression(p) => object = &p.expression,
                Expression::StaticMemberExpression(m) => object = &m.object,
                Expression::ComputedMemberExpression(m) => {
                    saw_ts = saw_ts || expression_contains_ts_only_syntax(&m.expression);
                    object = &m.object;
                }
                Expression::PrivateFieldExpression(m) => object = &m.object,
                Expression::TSNonNullExpression(e) => {
                    saw_ts = true;
                    object = &e.expression;
                }
                Expression::TSAsExpression(e) => {
                    saw_ts = true;
                    object = &e.expression;
                }
                Expression::TSSatisfiesExpression(e) => {
                    saw_ts = true;
                    object = &e.expression;
                }
                Expression::TSTypeAssertion(e) => {
                    saw_ts = true;
                    object = &e.expression;
                }
                Expression::TSInstantiationExpression(e) => {
                    saw_ts = true;
                    object = &e.expression;
                }
                Expression::Identifier(id) => {
                    return saw_ts
                        && matches!(
                            self.signal_kind(id.name.as_str()),
                            Some(BindingRuntimeKind::Prop | BindingRuntimeKind::BindableProp)
                        );
                }
                _ => return false,
            }
        }
    }

    /// The typed lvalue of a MEMBER write target (`o.a = …` / `o[i]++` /
    /// `o.#x = …`), classified from the whole lvalue — object chain plus
    /// computed keys (`computed_key` is a computed target's own outermost
    /// key) — the SINGLE funnel every member assignment AND update target
    /// passes through (both [`Self::classify_target`] and
    /// [`Self::classify_simple_target`] route their member arms here). A
    /// TS-wrapped chain rooting at a `$props()` prop — the wrapper on the
    /// spine OR anywhere inside a computed key — is the TS-wrapped
    /// REACTIVE-write class and fails closed
    /// ([`ClientLvalue::UnsupportedReactiveTarget`], same as a TS-wrapped
    /// bare-identifier target); every other member target stays the plain
    /// deep-write [`ClientLvalue::Member`].
    fn member_write_lvalue(
        &self,
        object: &Expression<'_>,
        computed_key: Option<&Expression<'_>>,
    ) -> ClientLvalue {
        if self.member_lvalue_is_ts_wrapped_prop_chain(object, computed_key) {
            ClientLvalue::UnsupportedReactiveTarget
        } else {
            ClientLvalue::Member
        }
    }

    /// Classify an assignment / update TARGET into its typed [`ClientLvalue`]. The
    /// classification is STRUCTURAL (the parsed OXC node), so a TS-wrapped
    /// (`(x as T)` / `x!`), private-field (`o.#x`), computed-member, or destructuring
    /// (`{ x }` / `[x]`) target is handled by its own arm — never silently dropped.
    pub(super) fn classify_target(&self, target: &AssignmentTarget<'_>) -> ClientLvalue {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(id) => {
                let name = id.name.as_str();
                if self.is_signal(name) {
                    ClientLvalue::SignalIdent {
                        name: name.to_string(),
                    }
                } else if let Some(prop) = self.prop_lvalue(name) {
                    prop
                } else if self.is_import_root(name) {
                    // An import binding is NOT reassignable (official
                    // `constant_assignment`) — never a writable plain ident.
                    ClientLvalue::ImportedBinding
                } else {
                    ClientLvalue::PlainIdent
                }
            }
            // Member targets (`o.a` / `o[i]` / `o.#x`) are deep writes,
            // classified through the single member-write funnel: plain member
            // access (a `BareProxy` / `StateProxy` member write stays plain),
            // EXCEPT a TS-wrapped chain (spine OR computed key) rooting at a
            // `$props()` prop, which fails closed. A computed target hands the
            // funnel its own outermost key alongside the object spine.
            AssignmentTarget::StaticMemberExpression(m) => {
                self.member_write_lvalue(&m.object, None)
            }
            AssignmentTarget::ComputedMemberExpression(m) => {
                self.member_write_lvalue(&m.object, Some(&m.expression))
            }
            AssignmentTarget::PrivateFieldExpression(m) => {
                self.member_write_lvalue(&m.object, None)
            }
            // A TS-wrapped target (`(x as T) = …` / `x! = …` / `<T>x = …`) could
            // denote a reactive write but is outside the supported plain-identifier
            // subset — fail closed rather than drop the rewrite.
            AssignmentTarget::TSAsExpression(_)
            | AssignmentTarget::TSSatisfiesExpression(_)
            | AssignmentTarget::TSNonNullExpression(_)
            | AssignmentTarget::TSTypeAssertion(_) => ClientLvalue::UnsupportedReactiveTarget,
            // A destructuring assignment target (`{ x } = …` / `[x] = …`) — the
            // official compiler lowers it through a destructure closure; fail closed.
            AssignmentTarget::ArrayAssignmentTarget(_)
            | AssignmentTarget::ObjectAssignmentTarget(_) => ClientLvalue::UnsupportedTarget,
        }
    }

    /// Classify a SIMPLE assignment / update target (the `UpdateExpression`
    /// argument, which excludes destructuring patterns) into its typed
    /// [`ClientLvalue`].
    pub(super) fn classify_simple_target(
        &self,
        target: &SimpleAssignmentTarget<'_>,
    ) -> ClientLvalue {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                let name = id.name.as_str();
                if self.is_signal(name) {
                    ClientLvalue::SignalIdent {
                        name: name.to_string(),
                    }
                } else if let Some(prop) = self.prop_lvalue(name) {
                    prop
                } else if self.is_import_root(name) {
                    // An import binding is NOT reassignable (official
                    // `constant_assignment`) — never a writable plain ident.
                    ClientLvalue::ImportedBinding
                } else {
                    ClientLvalue::PlainIdent
                }
            }
            // Member update targets route through the SAME single member-write
            // funnel as assignment member targets (a TS-wrapped chain — spine
            // OR computed key — rooting at a `$props()` prop fails closed).
            SimpleAssignmentTarget::StaticMemberExpression(m) => {
                self.member_write_lvalue(&m.object, None)
            }
            SimpleAssignmentTarget::ComputedMemberExpression(m) => {
                self.member_write_lvalue(&m.object, Some(&m.expression))
            }
            SimpleAssignmentTarget::PrivateFieldExpression(m) => {
                self.member_write_lvalue(&m.object, None)
            }
            SimpleAssignmentTarget::TSAsExpression(_)
            | SimpleAssignmentTarget::TSSatisfiesExpression(_)
            | SimpleAssignmentTarget::TSNonNullExpression(_)
            | SimpleAssignmentTarget::TSTypeAssertion(_) => ClientLvalue::UnsupportedReactiveTarget,
        }
    }
}
