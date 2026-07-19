//! Script-side usage facts for Vue macro-declared members.
//!
//! One typed-AST pass over the `<script setup>` program collects, for the
//! `defineProps` and `defineEmits` return bindings, every literal consumption
//! (member read / literal emit call) and every ESCAPE (any use the analysis
//! cannot bound statically). The unused-declaration diagnostics
//! (`no-unused-props` / `no-unused-emit-declarations` / `no-unused-slots`)
//! consume these facts fail-open: an escape suppresses the whole kind.
//!
//! Soundness model: every `IdentifierReference` to a tracked binding must be
//! consumed by a benign shape recognised at its PARENT node (member read,
//! literal emit call, vue `toRef`/`toRefs` idioms); any reference left
//! unconsumed is an escape. Shadowing a tracked name in an inner scope can
//! only ADD escapes (false suppression), never hide a real use — fail-open.

use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, ComputedMemberExpression, Expression,
    IdentifierReference, Program, PropertyKey, StaticMemberExpression, VariableDeclarator,
};
use oxc_ast_visit::{walk, Visit};
use rustc_hash::FxHashSet;
use verter_span::Span;

/// A literal emit call site: `emit("save", …)`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroUsageCall {
    /// The literal event name.
    pub name: String,
    /// SFC-absolute byte span of the call expression.
    pub span: Span,
}

/// Script-side usage facts for macro-declared members (see module docs).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroUsageFacts {
    /// Literal event names called on the `defineEmits` binding, with spans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emit_literal_calls: Vec<MacroUsageCall>,
    /// The emit binding escaped literal-call analysis (aliased, passed as a
    /// value, called with a dynamic event name, …). Suppresses ALL
    /// unused-event diagnostics for the component.
    pub emit_escapes: bool,
    /// Literal member names read off the `defineProps` binding (`props.x`,
    /// `props["x"]`, `toRef(props, "x")`, immediately-destructured
    /// `toRefs(props)` keys).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props_member_reads: Vec<String>,
    /// The props binding escaped member-read analysis (spread, call argument,
    /// alias, computed access, return, …). Suppresses ALL unused-prop
    /// diagnostics for the component.
    pub props_escapes: bool,
    /// `defineProps`/`withDefaults` was DESTRUCTURED — destructured member
    /// liveness is provider-owned (TS6133); the native unused-prop diagnostic
    /// must not double-report.
    pub props_destructured: bool,
}

/// Collect [`MacroUsageFacts`] from a parsed `<script setup>` program.
///
/// `props_binding` / `emit_binding` are the `defineProps` / `defineEmits`
/// return binding names (when bound to a plain identifier). `vue_value_imports`
/// is the set of local names value-imported from `vue` — `toRef`/`toRefs`
/// special-casing applies only to the real Vue helpers, never to same-named
/// userland functions (which stay escapes).
pub fn collect_macro_usage(
    program: &Program<'_>,
    props_binding: Option<&str>,
    emit_binding: Option<&str>,
    vue_value_imports: &FxHashSet<String>,
) -> MacroUsageFacts {
    let mut visitor = MacroUsageVisitor {
        props_name: props_binding,
        emit_name: emit_binding,
        vue_value_imports,
        facts: MacroUsageFacts::default(),
        consumed: FxHashSet::default(),
    };
    visitor.visit_program(program);
    visitor.facts
}

struct MacroUsageVisitor<'n> {
    props_name: Option<&'n str>,
    emit_name: Option<&'n str>,
    vue_value_imports: &'n FxHashSet<String>,
    facts: MacroUsageFacts,
    /// Span starts of identifier references consumed by a benign parent shape.
    consumed: FxHashSet<u32>,
}

impl MacroUsageVisitor<'_> {
    fn is_props(&self, name: &str) -> bool {
        self.props_name == Some(name)
    }

    fn is_emit(&self, name: &str) -> bool {
        self.emit_name == Some(name)
    }

    fn is_vue_helper(&self, name: &str) -> bool {
        self.vue_value_imports.contains(name)
    }

    /// The literal string value of an argument, if it is one.
    fn literal_arg(arg: &Argument<'_>) -> Option<String> {
        match arg.as_expression()? {
            Expression::StringLiteral(s) => Some(s.value.to_string()),
            Expression::TemplateLiteral(t) if t.expressions.is_empty() => t
                .quasis
                .first()
                .and_then(|q| q.value.cooked.as_ref())
                .map(|c| c.to_string()),
            _ => None,
        }
    }

    fn arg_props_ident_start(&self, arg: &Argument<'_>) -> Option<u32> {
        if let Some(Expression::Identifier(id)) = arg.as_expression() {
            if self.is_props(&id.name) {
                return Some(id.span.start);
            }
        }
        None
    }
}

impl<'a> Visit<'a> for MacroUsageVisitor<'_> {
    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        if let Some(Expression::CallExpression(call)) = &decl.init {
            if let Expression::Identifier(callee) = &call.callee {
                // Destructured defineProps/withDefaults — provider-owned TS6133.
                if matches!(callee.name.as_str(), "defineProps" | "withDefaults")
                    && matches!(&decl.id, BindingPattern::ObjectPattern(_))
                {
                    self.facts.props_destructured = true;
                }
                // `const { a, b } = toRefs(props)` — per-member reads.
                if callee.name == "toRefs" && self.is_vue_helper("toRefs") {
                    if let (Some(props_start), BindingPattern::ObjectPattern(pattern)) = (
                        call.arguments
                            .first()
                            .and_then(|a| self.arg_props_ident_start(a)),
                        &decl.id,
                    ) {
                        let mut all_static = pattern.rest.is_none();
                        let mut keys = Vec::new();
                        for prop in &pattern.properties {
                            match &prop.key {
                                PropertyKey::StaticIdentifier(id) => keys.push(id.name.to_string()),
                                PropertyKey::StringLiteral(s) => keys.push(s.value.to_string()),
                                _ => all_static = false,
                            }
                        }
                        if all_static {
                            self.consumed.insert(props_start);
                            self.facts.props_member_reads.extend(keys);
                        }
                        // Non-static keys / rest: props ident stays unconsumed
                        // and the identifier-reference fallback records the
                        // escape.
                    }
                }
            }
        }
        walk::walk_variable_declarator(self, decl);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Expression::Identifier(callee) = &call.callee {
            if self.is_emit(&callee.name) {
                // The callee position itself is a benign consumption; the
                // OUTCOME depends on the first argument.
                self.consumed.insert(callee.span.start);
                match call.arguments.first().and_then(Self::literal_arg) {
                    Some(name) => self.facts.emit_literal_calls.push(MacroUsageCall {
                        name,
                        span: Span::new(call.span.start, call.span.end),
                    }),
                    None => self.facts.emit_escapes = true,
                }
            } else if callee.name == "toRef" && self.is_vue_helper("toRef") {
                if let Some(props_start) = call
                    .arguments
                    .first()
                    .and_then(|a| self.arg_props_ident_start(a))
                {
                    self.consumed.insert(props_start);
                    match call.arguments.get(1).and_then(Self::literal_arg) {
                        Some(member) => self.facts.props_member_reads.push(member),
                        // `toRef(props)` whole-object ref — escapes.
                        None => self.facts.props_escapes = true,
                    }
                }
            }
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        if let Expression::Identifier(object) = &member.object {
            if self.is_props(&object.name) {
                self.consumed.insert(object.span.start);
                self.facts
                    .props_member_reads
                    .push(member.property.name.to_string());
            }
        }
        walk::walk_static_member_expression(self, member);
    }

    fn visit_computed_member_expression(&mut self, member: &ComputedMemberExpression<'a>) {
        if let Expression::Identifier(object) = &member.object {
            if self.is_props(&object.name) {
                self.consumed.insert(object.span.start);
                match &member.expression {
                    Expression::StringLiteral(s) => {
                        self.facts.props_member_reads.push(s.value.to_string());
                    }
                    // `props[key]` — cannot bound the member set.
                    _ => self.facts.props_escapes = true,
                }
            }
        }
        walk::walk_computed_member_expression(self, member);
    }

    fn visit_identifier_reference(&mut self, id: &IdentifierReference<'a>) {
        if !self.consumed.contains(&id.span.start) {
            if self.is_props(&id.name) {
                self.facts.props_escapes = true;
            }
            if self.is_emit(&id.name) {
                self.facts.emit_escapes = true;
            }
        }
        walk::walk_identifier_reference(self, id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(source: &str, props: Option<&str>, emit: Option<&str>) -> MacroUsageFacts {
        let allocator = oxc_allocator::Allocator::new();
        let parsed =
            oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
        assert!(
            parsed.errors.is_empty(),
            "fixture must parse: {:?}",
            parsed.errors
        );
        let vue: FxHashSet<String> = ["toRef", "toRefs", "useSlots"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        collect_macro_usage(&parsed.program, props, emit, &vue)
    }

    #[test]
    fn literal_emit_calls_are_collected_with_spans() {
        let f = facts(
            "const emit = defineEmits<{ save: []; close: [] }>();\nemit('save');\nfunction f() { emit(`close`); }",
            None,
            Some("emit"),
        );
        let names: Vec<&str> = f
            .emit_literal_calls
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["save", "close"]);
        assert!(!f.emit_escapes);
        assert!(f.emit_literal_calls[0].span.start > 0, "call span recorded");
    }

    #[test]
    fn dynamic_event_name_escapes() {
        let f = facts(
            "const emit = defineEmits();\nemit(name);",
            None,
            Some("emit"),
        );
        assert!(f.emit_escapes);
        assert!(f.emit_literal_calls.is_empty());
    }

    #[test]
    fn emit_passed_as_value_escapes() {
        for source in [
            "const emit = defineEmits();\nconst e = emit;",
            "const emit = defineEmits();\nregister(emit);",
            "const emit = defineEmits();\nexport function use() { return emit; }",
        ] {
            let f = facts(source, None, Some("emit"));
            assert!(f.emit_escapes, "must escape: {source}");
        }
    }

    #[test]
    fn props_member_reads_are_collected() {
        let f = facts(
            "const props = defineProps<{ a: number; b: string }>();\nconst x = props.a;\nconst y = props['b'];",
            Some("props"),
            None,
        );
        assert_eq!(f.props_member_reads, ["a", "b"]);
        assert!(!f.props_escapes);
    }

    #[test]
    fn props_whole_object_uses_escape() {
        for source in [
            "const props = defineProps();\nwatch(props, () => {});",
            "const props = defineProps();\nconst all = { ...props };",
            "const props = defineProps();\nconst alias = props;",
            "const props = defineProps();\nconst v = props[key];",
        ] {
            let f = facts(source, Some("props"), None);
            assert!(f.props_escapes, "must escape: {source}");
        }
    }

    #[test]
    fn vue_to_ref_literal_is_a_member_read_not_an_escape() {
        let f = facts(
            "import { toRef } from 'vue';\nconst props = defineProps();\nconst a = toRef(props, 'a');",
            Some("props"),
            None,
        );
        assert_eq!(f.props_member_reads, ["a"]);
        assert!(!f.props_escapes);
    }

    #[test]
    fn to_refs_immediately_destructured_reads_members() {
        let f = facts(
            "import { toRefs } from 'vue';\nconst props = defineProps();\nconst { a, b } = toRefs(props);",
            Some("props"),
            None,
        );
        assert_eq!(f.props_member_reads, ["a", "b"]);
        assert!(!f.props_escapes);
    }

    #[test]
    fn to_refs_stored_whole_escapes() {
        let f = facts(
            "import { toRefs } from 'vue';\nconst props = defineProps();\nconst refs = toRefs(props);",
            Some("props"),
            None,
        );
        assert!(f.props_escapes);
    }

    #[test]
    fn userland_to_ref_is_an_escape_not_a_member_read() {
        // NOT imported from vue — a same-named userland helper receives the
        // whole props object; treating it as a member read would be unsound.
        let allocator = oxc_allocator::Allocator::new();
        let source = "const props = defineProps();\nconst a = toRef(props, 'a');";
        let parsed =
            oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
        let f = collect_macro_usage(&parsed.program, Some("props"), None, &FxHashSet::default());
        assert!(f.props_escapes);
        assert!(f.props_member_reads.is_empty());
    }

    #[test]
    fn destructured_define_props_is_flagged_provider_owned() {
        let f = facts("const { a } = defineProps<{ a: number }>();", None, None);
        assert!(f.props_destructured);
        let f = facts(
            "const { a } = withDefaults(defineProps<{ a?: number }>(), { a: 1 });",
            None,
            None,
        );
        assert!(f.props_destructured);
        let f = facts(
            "const props = defineProps<{ a: number }>();",
            Some("props"),
            None,
        );
        assert!(!f.props_destructured);
    }
}
