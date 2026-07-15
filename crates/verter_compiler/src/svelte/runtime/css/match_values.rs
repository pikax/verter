//! The value-enumeration half of the selector-to-template matcher: the
//! official `css/utils.js` `get_possible_values` / `gather_possible_values`
//! ported over the OWNED typed expression projection ([`MatcherExpr`]) that
//! template-expression analysis lowers during its single parse (estree parity
//! via transparent paren peeling at lowering; exact JS `String(value)` forms
//! for number / bigint literals computed there too). UNKNOWN bails to `None`
//! ("may match anything", as upstream); a literal whose JS stringification
//! cannot be reproduced exactly fails closed with a construct description.
//!
//! The matcher WALKS the stored projection — it never parses expression
//! source (no `oxc_parser`, no synth-wrap reparse; pinned by the
//! `svelte_css_matcher_uses_analyzed_expr_ir` architecture guard).

use crate::svelte::runtime::expr::{
    MatcherArrayElement, MatcherExpr, MatcherLiteral, MatcherObjectKey,
};

// ─────────────────────────────────────────────────────────────────────────────
// Expression-value enumeration (css/utils.js `get_possible_values` /
// `gather_possible_values`) over the owned typed matcher projection.
// ─────────────────────────────────────────────────────────────────────────────

/// A gathered JS value — the official set holds stringified literals plus the
/// raw falsy fill-ins (`''` is the empty string; `false`/`NaN`/`0` keep their
/// JS-value identity for the `&&` falsiness filter before the final
/// stringification).
#[derive(Debug, Clone, PartialEq)]
enum JsVal {
    Str(String),
    False,
    Zero,
    NaN,
}

impl JsVal {
    /// JS truthiness (none of the gathered values are nullish).
    fn is_falsy(&self) -> bool {
        match self {
            JsVal::Str(s) => s.is_empty(),
            JsVal::False | JsVal::Zero | JsVal::NaN => true,
        }
    }

    /// The final `String(value)` map.
    fn into_string(self) -> String {
        match self {
            JsVal::Str(s) => s,
            JsVal::False => "false".to_string(),
            JsVal::Zero => "0".to_string(),
            JsVal::NaN => "NaN".to_string(),
        }
    }
}

/// The gathering set — insertion-ordered with JS-`Set` dedup semantics plus
/// the sticky UNKNOWN marker.
#[derive(Default)]
struct JsValSet {
    values: Vec<JsVal>,
    unknown: bool,
}

impl JsValSet {
    fn add(&mut self, value: JsVal) {
        if !self.values.contains(&value) {
            self.values.push(value);
        }
    }

    fn add_unknown(&mut self) {
        self.unknown = true;
    }
}

/// The shape of an official "expression attribute" value — the estree
/// classification `visit_component` keys on.
pub(super) enum ExprAttrShape {
    /// A bare identifier reference (transparent parens peeled, as estree has
    /// no paren nodes).
    Identifier(String),
    /// An estree `Literal` (string / number / boolean / null / bigint /
    /// regexp).
    Literal,
    /// Anything else.
    Other,
}

/// Classify the stored matcher projection's root for the component
/// snippet-resolution rules (the projection was lowered from the
/// transparent-paren-peeled node, so the root is already estree-shaped).
pub(super) fn expression_attr_shape(expr: &MatcherExpr) -> ExprAttrShape {
    match expr {
        MatcherExpr::Identifier(name) => ExprAttrShape::Identifier(name.clone()),
        MatcherExpr::Literal(_) => ExprAttrShape::Literal,
        _ => ExprAttrShape::Other,
    }
}

/// The official `get_possible_values` for an EXPRESSION chunk: `Ok(None)` is
/// the UNKNOWN bail ("may match anything"); `Err` is a value whose exact JS
/// stringification cannot be reproduced (fail closed, never guessed).
pub(super) fn expression_possible_values(
    expr: &MatcherExpr,
    is_class: bool,
) -> Result<Option<Vec<String>>, &'static str> {
    let mut set = JsValSet::default();
    gather_possible_values(expr, is_class, &mut set, false)?;
    if set.unknown {
        return Ok(None);
    }
    Ok(Some(
        set.values.into_iter().map(JsVal::into_string).collect(),
    ))
}

/// The official `gather_possible_values(node, is_class, set, is_nested)`,
/// walking the owned matcher projection.
fn gather_possible_values(
    node: &MatcherExpr,
    is_class: bool,
    set: &mut JsValSet,
    is_nested: bool,
) -> Result<(), &'static str> {
    if set.unknown {
        // No point traversing any further.
        return Ok(());
    }
    match node {
        // The estree `Literal` family — the exact `String(node.value)` was
        // computed at lowering; an unreproducible stringification fails
        // closed with its construct description.
        MatcherExpr::Literal(MatcherLiteral::Value(value)) => set.add(JsVal::Str(value.clone())),
        MatcherExpr::Literal(MatcherLiteral::Refusal(construct)) => return Err(construct),
        MatcherExpr::Conditional {
            consequent,
            alternate,
        } => {
            gather_possible_values(consequent, is_class, set, is_nested)?;
            gather_possible_values(alternate, is_class, set, is_nested)?;
        }
        MatcherExpr::Logical { and, left, right } => {
            if *and {
                // `&&` — the left side is included only when falsy.
                let mut left_set = JsValSet::default();
                gather_possible_values(left, is_class, &mut left_set, is_nested)?;
                if left_set.unknown {
                    // Add all non-nullish falsy values, unless a nested
                    // `class` value (clsx removes falsy entries).
                    if !is_class || !is_nested {
                        set.add(JsVal::Str(String::new()));
                        set.add(JsVal::False);
                        set.add(JsVal::NaN);
                        set.add(JsVal::Zero);
                    }
                } else {
                    for value in &left_set.values {
                        if value.is_falsy() && (!is_class || !is_nested) {
                            set.add(value.clone());
                        }
                    }
                }
                gather_possible_values(right, is_class, set, is_nested)?;
            } else {
                gather_possible_values(left, is_class, set, is_nested)?;
                gather_possible_values(right, is_class, set, is_nested)?;
            }
        }
        MatcherExpr::Array(elements) if is_class => {
            for element in elements {
                match element {
                    MatcherArrayElement::Elision => {}
                    MatcherArrayElement::Spread => set.add_unknown(),
                    MatcherArrayElement::Expr(expr) => {
                        gather_possible_values(expr, is_class, set, true)?;
                    }
                }
            }
        }
        MatcherExpr::Object(keys) if is_class => {
            for key in keys {
                match key {
                    MatcherObjectKey::Value(name) => set.add(JsVal::Str(name.clone())),
                    MatcherObjectKey::Refusal(construct) => return Err(construct),
                    MatcherObjectKey::Unknown => set.add_unknown(),
                }
            }
        }
        _ => set.add_unknown(),
    }
    Ok(())
}
