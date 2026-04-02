//! Debug and display helpers for solver internals.
//!
//! These are for tracing and test diagnostics, not for public output.

use super::arena::{MappedModifierKind, Node, NodeId, PrimitiveKind, QueryArena, SolverLiteral};

// ---------------------------------------------------------------------------
// Node display
// ---------------------------------------------------------------------------

/// Display a node in human-readable form for debugging.
pub fn display_node(arena: &QueryArena, id: NodeId) -> String {
    if id.is_unresolved() {
        return "UNRESOLVED".to_string();
    }

    let mut buf = String::new();
    display_node_inner(arena, id, &mut buf, 0, &mut Vec::new());
    buf
}

fn display_node_inner(
    arena: &QueryArena,
    id: NodeId,
    buf: &mut String,
    depth: usize,
    visited: &mut Vec<NodeId>,
) {
    // Cycle guard
    if depth > 20 || visited.contains(&id) {
        buf.push_str("...");
        return;
    }
    visited.push(id);

    match arena.get(id) {
        Node::Primitive(kind) => buf.push_str(primitive_name(*kind)),
        Node::Literal(lit) => match lit {
            SolverLiteral::String(s) => {
                buf.push('"');
                buf.push_str(s);
                buf.push('"');
            }
            SolverLiteral::Number(n) => {
                buf.push_str(&n.to_string());
            }
            SolverLiteral::Boolean(b) => {
                buf.push_str(if *b { "true" } else { "false" });
            }
            SolverLiteral::BigInt(s) => {
                buf.push_str(s);
                buf.push('n');
            }
        },
        Node::Union(members) => {
            for (i, &m) in members.iter().enumerate() {
                if i > 0 {
                    buf.push_str(" | ");
                }
                display_node_inner(arena, m, buf, depth + 1, visited);
            }
        }
        Node::Intersection(members) => {
            for (i, &m) in members.iter().enumerate() {
                if i > 0 {
                    buf.push_str(" & ");
                }
                display_node_inner(arena, m, buf, depth + 1, visited);
            }
        }
        Node::Array { element, readonly } => {
            if *readonly {
                buf.push_str("readonly ");
            }
            display_node_inner(arena, *element, buf, depth + 1, visited);
            buf.push_str("[]");
        }
        Node::Tuple { elements, readonly } => {
            if *readonly {
                buf.push_str("readonly ");
            }
            buf.push('[');
            for (i, el) in elements.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                if el.rest {
                    buf.push_str("...");
                }
                if let Some(ref label) = el.label {
                    buf.push_str(label);
                    buf.push_str(": ");
                }
                display_node_inner(arena, el.ty, buf, depth + 1, visited);
                if el.optional {
                    buf.push('?');
                }
            }
            buf.push(']');
        }
        Node::Object(obj) => {
            buf.push_str("{ ");
            for (i, prop) in obj.properties.iter().enumerate() {
                if i > 0 {
                    buf.push_str("; ");
                }
                if prop.readonly {
                    buf.push_str("readonly ");
                }
                buf.push_str(&prop.name);
                if prop.optional {
                    buf.push('?');
                }
                buf.push_str(": ");
                display_node_inner(arena, prop.ty, buf, depth + 1, visited);
            }
            if !obj.properties.is_empty() && !obj.index_signatures.is_empty() {
                buf.push_str("; ");
            }
            for (i, idx) in obj.index_signatures.iter().enumerate() {
                if i > 0 {
                    buf.push_str("; ");
                }
                if idx.readonly {
                    buf.push_str("readonly ");
                }
                buf.push_str("[key: ");
                display_node_inner(arena, idx.key_type, buf, depth + 1, visited);
                buf.push_str("]: ");
                display_node_inner(arena, idx.value_type, buf, depth + 1, visited);
            }
            buf.push_str(" }");
        }
        Node::Function(func) => {
            if let Some(sig) = func.signatures.first() {
                buf.push('(');
                for (i, param) in sig.parameters.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    if param.rest {
                        buf.push_str("...");
                    }
                    if let Some(ref name) = param.name {
                        buf.push_str(name);
                        buf.push_str(": ");
                    }
                    display_node_inner(arena, param.ty, buf, depth + 1, visited);
                    if param.optional {
                        buf.push('?');
                    }
                }
                buf.push_str(") => ");
                display_node_inner(arena, sig.return_type, buf, depth + 1, visited);
            } else {
                buf.push_str("() => void");
            }
        }
        Node::Ref {
            name,
            type_arguments,
        } => {
            buf.push_str(name);
            if !type_arguments.is_empty() {
                buf.push('<');
                for (i, &arg) in type_arguments.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    display_node_inner(arena, arg, buf, depth + 1, visited);
                }
                buf.push('>');
            }
        }
        Node::Applied { identity, args } => {
            buf.push_str(&identity.symbol_name);
            if !args.is_empty() {
                buf.push('<');
                for (i, &arg) in args.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    display_node_inner(arena, arg, buf, depth + 1, visited);
                }
                buf.push('>');
            }
        }
        Node::TypeParam { name, .. } => {
            buf.push_str(name);
        }
        Node::KeyOf(operand) => {
            buf.push_str("keyof ");
            display_node_inner(arena, *operand, buf, depth + 1, visited);
        }
        Node::TypeOf { path } => {
            buf.push_str("typeof ");
            buf.push_str(&path.join("."));
        }
        Node::IndexedAccess { object, index } => {
            display_node_inner(arena, *object, buf, depth + 1, visited);
            buf.push('[');
            display_node_inner(arena, *index, buf, depth + 1, visited);
            buf.push(']');
        }
        Node::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            ..
        } => {
            display_node_inner(arena, *check, buf, depth + 1, visited);
            buf.push_str(" extends ");
            display_node_inner(arena, *extends, buf, depth + 1, visited);
            buf.push_str(" ? ");
            display_node_inner(arena, *true_branch, buf, depth + 1, visited);
            buf.push_str(" : ");
            display_node_inner(arena, *false_branch, buf, depth + 1, visited);
        }
        Node::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            buf.push_str("{ ");
            match readonly {
                MappedModifierKind::Add => buf.push_str("+readonly "),
                MappedModifierKind::Remove => buf.push_str("-readonly "),
                MappedModifierKind::Unchanged => {}
            }
            buf.push('[');
            buf.push_str(parameter);
            buf.push_str(" in ");
            display_node_inner(arena, *source, buf, depth + 1, visited);
            if let Some(nt) = name_type {
                buf.push_str(" as ");
                display_node_inner(arena, *nt, buf, depth + 1, visited);
            }
            buf.push(']');
            match optional {
                MappedModifierKind::Add => buf.push_str("+?"),
                MappedModifierKind::Remove => buf.push_str("-?"),
                MappedModifierKind::Unchanged => {}
            }
            buf.push_str(": ");
            display_node_inner(arena, *value, buf, depth + 1, visited);
            buf.push_str(" }");
        }
        Node::TemplateLiteral {
            quasis,
            expressions,
        } => {
            buf.push('`');
            for (i, quasi) in quasis.iter().enumerate() {
                buf.push_str(quasi);
                if let Some(&expr) = expressions.get(i) {
                    buf.push_str("${");
                    display_node_inner(arena, expr, buf, depth + 1, visited);
                    buf.push('}');
                }
            }
            buf.push('`');
        }
        Node::Infer { name } => {
            buf.push_str("infer ");
            buf.push_str(name);
        }
        Node::Rest(inner) => {
            buf.push_str("...");
            display_node_inner(arena, *inner, buf, depth + 1, visited);
        }
        Node::RecursiveRef { target } => {
            buf.push_str("@rec(");
            buf.push_str(&target.to_string());
            buf.push(')');
        }
        Node::Error { description } => {
            buf.push_str("error(");
            buf.push_str(description);
            buf.push(')');
        }
    }

    visited.pop();
}

fn primitive_name(kind: PrimitiveKind) -> &'static str {
    match kind {
        PrimitiveKind::String => "string",
        PrimitiveKind::Number => "number",
        PrimitiveKind::Boolean => "boolean",
        PrimitiveKind::Symbol => "symbol",
        PrimitiveKind::BigInt => "bigint",
        PrimitiveKind::Any => "any",
        PrimitiveKind::Unknown => "unknown",
        PrimitiveKind::Void => "void",
        PrimitiveKind::Never => "never",
        PrimitiveKind::Null => "null",
        PrimitiveKind::Undefined => "undefined",
        PrimitiveKind::Object => "object",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::arena::*;
    use super::*;

    #[test]
    fn display_primitive() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        assert_eq!(display_node(&arena, s), "string");
    }

    #[test]
    fn display_literal() {
        let mut arena = QueryArena::new();
        let lit = arena.string_literal("hello");
        assert_eq!(display_node(&arena, lit), "\"hello\"");
    }

    #[test]
    fn display_union() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let n = arena.primitive(PrimitiveKind::Number);
        let u = arena.union(vec![s, n]);
        assert_eq!(display_node(&arena, u), "string | number");
    }

    #[test]
    fn display_intersection() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let n = arena.primitive(PrimitiveKind::Number);
        let i = arena.intersection(vec![s, n]);
        assert_eq!(display_node(&arena, i), "string & number");
    }

    #[test]
    fn display_array() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let a = arena.array(s, false);
        assert_eq!(display_node(&arena, a), "string[]");
    }

    #[test]
    fn display_readonly_array() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let a = arena.array(s, true);
        assert_eq!(display_node(&arena, a), "readonly string[]");
    }

    #[test]
    fn display_object() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let obj = arena.object(ObjectNode {
            properties: vec![PropertyNode {
                name: "x".into(),
                ty: s,
                optional: false,
                readonly: false,
                is_method: false,
            }],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });
        assert_eq!(display_node(&arena, obj), "{ x: string }");
    }

    #[test]
    fn display_ref_with_args() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let r = arena.type_ref("Partial", vec![s]);
        assert_eq!(display_node(&arena, r), "Partial<string>");
    }

    #[test]
    fn display_conditional() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let n = arena.primitive(PrimitiveKind::Number);
        let b = arena.primitive(PrimitiveKind::Boolean);
        let never = arena.primitive(PrimitiveKind::Never);
        let c = arena.conditional(s, n, b, never, false);
        assert_eq!(
            display_node(&arena, c),
            "string extends number ? boolean : never"
        );
    }

    #[test]
    fn display_keyof() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let k = arena.key_of(s);
        assert_eq!(display_node(&arena, k), "keyof string");
    }

    #[test]
    fn display_indexed_access() {
        let mut arena = QueryArena::new();
        let s = arena.primitive(PrimitiveKind::String);
        let n = arena.primitive(PrimitiveKind::Number);
        let ia = arena.indexed_access(s, n);
        assert_eq!(display_node(&arena, ia), "string[number]");
    }

    #[test]
    fn display_unresolved() {
        let arena = QueryArena::new();
        assert_eq!(display_node(&arena, NodeId::UNRESOLVED), "UNRESOLVED");
    }
}
