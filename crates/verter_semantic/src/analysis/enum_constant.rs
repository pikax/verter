//! The checker's constant evaluation of enum member initializers.
//!
//! An initializer is read into an [`EnumConstantExpr`] — literals, the
//! values paths name, and the operators the checker folds — and evaluated
//! by one stack machine with ECMAScript's operator semantics. The caller
//! resolves every referenced path: a declaration's own lowering resolves
//! the earlier members of the same enum, and the session resolves the rest
//! (another enum's member, another declaration of a merged enum, a `const`
//! variable), so the two evaluate the same program the same way.

use std::sync::Arc;

use oxc_ast::ast::{BinaryOperator, Expression, UnaryOperator};
use verter_type_expr::facts::{
    EnumConstantBinaryOp, EnumConstantExpr, EnumConstantStep, EnumConstantUnaryOp, EnumScalar,
};

/// A constant enum member value: a number or a string.
#[derive(Debug, Clone, PartialEq)]
pub enum EnumConstant {
    Number(f64),
    String(String),
}

impl EnumConstant {
    /// The constant a stored member scalar holds; `None` for a computed
    /// member's domain.
    #[must_use]
    pub fn from_scalar(scalar: &EnumScalar) -> Option<Self> {
        match scalar {
            EnumScalar::Number(text) => text.parse::<f64>().ok().map(Self::Number),
            EnumScalar::String(text) => Some(Self::String(text.clone())),
            EnumScalar::Primitive(_) => None,
        }
    }

    /// The stored scalar of this constant.
    #[must_use]
    pub fn to_scalar(&self) -> EnumScalar {
        match self {
            Self::Number(value) => EnumScalar::Number(number_text(*value)),
            Self::String(value) => EnumScalar::String(value.clone()),
        }
    }

    /// The value as the operand of a string concatenation (ECMAScript
    /// `ToString`).
    fn concatenated(&self) -> String {
        match self {
            Self::Number(value) => {
                match verter_type_expr::PropertyKey::<()>::from_js_number(*value) {
                    verter_type_expr::PropertyKey::Number(index) => index.to_string(),
                    verter_type_expr::PropertyKey::String(text) => text.to_string(),
                    verter_type_expr::PropertyKey::UniqueSymbol(()) => {
                        unreachable!("a number spells no symbol")
                    }
                }
            }
            Self::String(value) => value.clone(),
        }
    }
}

/// The canonical `f64` display string a number step and a numeric member
/// scalar store.
fn number_text(value: f64) -> String {
    format!("{value}")
}

/// Read an initializer into the evaluator's program: number and string
/// literals, no-substitution and substituting templates, parentheses, the
/// unary `+` `-` `~`, the binary arithmetic and bitwise operators, `Infinity`
/// and `NaN`, and a value named by an identifier or a static member path —
/// `E["A"]` read as the path `E.A`. `None` for any other expression (a
/// call, a tagged template, an arbitrary element access): the member is
/// then computed.
#[must_use]
pub fn enum_constant_expr(expr: &Expression<'_>) -> Option<EnumConstantExpr> {
    let mut steps = Vec::new();
    push_steps(expr, &mut steps)?;
    Some(EnumConstantExpr {
        steps: Arc::from(steps.into_boxed_slice()),
    })
}

/// The program of the first member without an initializer: zero.
#[must_use]
pub fn enum_first_member_expr() -> EnumConstantExpr {
    EnumConstantExpr {
        steps: Arc::from(vec![EnumConstantStep::Number(number_text(0.0))].into_boxed_slice()),
    }
}

/// The program of a member without an initializer that follows `previous`:
/// the previous member's value plus one.
#[must_use]
pub fn enum_increment_expr(previous: &str) -> EnumConstantExpr {
    EnumConstantExpr {
        steps: Arc::from(
            vec![
                EnumConstantStep::Reference(Arc::from(
                    vec![previous.to_string()].into_boxed_slice(),
                )),
                EnumConstantStep::Increment,
            ]
            .into_boxed_slice(),
        ),
    }
}

fn push_steps(expr: &Expression<'_>, steps: &mut Vec<EnumConstantStep>) -> Option<()> {
    match expr {
        Expression::NumericLiteral(literal) => {
            steps.push(EnumConstantStep::Number(number_text(literal.value)));
        }
        Expression::StringLiteral(literal) => {
            steps.push(EnumConstantStep::String(literal.value.to_string()));
        }
        Expression::TemplateLiteral(template) => {
            let mut quasis = Vec::with_capacity(template.quasis.len());
            for quasi in &template.quasis {
                quasis.push(quasi.value.cooked.as_ref()?.to_string());
            }
            for expression in &template.expressions {
                push_steps(expression, steps)?;
            }
            steps.push(EnumConstantStep::Template(Arc::from(
                quasis.into_boxed_slice(),
            )));
        }
        Expression::ParenthesizedExpression(paren) => push_steps(&paren.expression, steps)?,
        Expression::UnaryExpression(unary) => {
            let op = match unary.operator {
                UnaryOperator::UnaryPlus => EnumConstantUnaryOp::Plus,
                UnaryOperator::UnaryNegation => EnumConstantUnaryOp::Minus,
                UnaryOperator::BitwiseNot => EnumConstantUnaryOp::BitNot,
                _ => return None,
            };
            push_steps(&unary.argument, steps)?;
            steps.push(EnumConstantStep::Unary(op));
        }
        Expression::BinaryExpression(binary) => {
            let op = match binary.operator {
                BinaryOperator::BitwiseOR => EnumConstantBinaryOp::BitOr,
                BinaryOperator::BitwiseAnd => EnumConstantBinaryOp::BitAnd,
                BinaryOperator::BitwiseXOR => EnumConstantBinaryOp::BitXor,
                BinaryOperator::ShiftLeft => EnumConstantBinaryOp::ShiftLeft,
                BinaryOperator::ShiftRight => EnumConstantBinaryOp::ShiftRight,
                BinaryOperator::ShiftRightZeroFill => EnumConstantBinaryOp::ShiftRightUnsigned,
                BinaryOperator::Multiplication => EnumConstantBinaryOp::Multiply,
                BinaryOperator::Division => EnumConstantBinaryOp::Divide,
                BinaryOperator::Addition => EnumConstantBinaryOp::Add,
                BinaryOperator::Subtraction => EnumConstantBinaryOp::Subtract,
                BinaryOperator::Remainder => EnumConstantBinaryOp::Remainder,
                BinaryOperator::Exponential => EnumConstantBinaryOp::Exponent,
                _ => return None,
            };
            push_steps(&binary.left, steps)?;
            push_steps(&binary.right, steps)?;
            steps.push(EnumConstantStep::Binary(op));
        }
        Expression::Identifier(identifier) => match identifier.name.as_str() {
            "Infinity" => steps.push(EnumConstantStep::Number(number_text(f64::INFINITY))),
            "NaN" => steps.push(EnumConstantStep::Number(number_text(f64::NAN))),
            name => steps.push(EnumConstantStep::Reference(Arc::from(
                vec![name.to_string()].into_boxed_slice(),
            ))),
        },
        Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
            let path = member_path(expr)?;
            steps.push(EnumConstantStep::Reference(Arc::from(
                path.into_boxed_slice(),
            )));
        }
        _ => return None,
    }
    Some(())
}

/// The path a static member chain names — `NS.E.A`, `E["A"]` — rooted at an
/// identifier.
fn member_path(expr: &Expression<'_>) -> Option<Vec<String>> {
    match expr {
        Expression::Identifier(identifier) => Some(vec![identifier.name.to_string()]),
        Expression::StaticMemberExpression(member) => {
            let mut path = member_path(&member.object)?;
            path.push(member.property.name.to_string());
            Some(path)
        }
        Expression::ComputedMemberExpression(member) => match &member.expression {
            Expression::StringLiteral(key) => {
                let mut path = member_path(&member.object)?;
                path.push(key.value.to_string());
                Some(path)
            }
            _ => None,
        },
        Expression::ParenthesizedExpression(paren) => member_path(&paren.expression),
        _ => None,
    }
}

/// Evaluate a member's program, asking `resolve` for the value each
/// referenced path names. `None` when the program is not a constant: a
/// reference that names no constant, an operator over the wrong kinds of
/// operand.
pub fn evaluate_enum_constant(
    expr: &EnumConstantExpr,
    mut resolve: impl FnMut(&[String]) -> Option<EnumConstant>,
) -> Option<EnumConstant> {
    let mut stack: Vec<EnumConstant> = Vec::new();
    for step in expr.steps.iter() {
        match step {
            EnumConstantStep::Number(text) => stack.push(EnumConstant::Number(text.parse().ok()?)),
            EnumConstantStep::String(text) => stack.push(EnumConstant::String(text.clone())),
            EnumConstantStep::Reference(path) => stack.push(resolve(path)?),
            EnumConstantStep::Unary(op) => {
                let EnumConstant::Number(value) = stack.pop()? else {
                    return None;
                };
                stack.push(EnumConstant::Number(match op {
                    EnumConstantUnaryOp::Plus => value,
                    EnumConstantUnaryOp::Minus => -value,
                    EnumConstantUnaryOp::BitNot => f64::from(!js_to_int32(value)),
                }));
            }
            EnumConstantStep::Binary(op) => {
                let right = stack.pop()?;
                let left = stack.pop()?;
                stack.push(binary(*op, left, right)?);
            }
            EnumConstantStep::Template(quasis) => {
                let values =
                    stack.split_off(stack.len().checked_sub(quasis.len().checked_sub(1)?)?);
                let mut text = String::new();
                for (index, quasi) in quasis.iter().enumerate() {
                    text.push_str(quasi);
                    if let Some(value) = values.get(index) {
                        text.push_str(&value.concatenated());
                    }
                }
                stack.push(EnumConstant::String(text));
            }
            EnumConstantStep::Increment => {
                let EnumConstant::Number(value) = stack.pop()? else {
                    return None;
                };
                stack.push(EnumConstant::Number(value + 1.0));
            }
        }
    }
    let value = stack.pop()?;
    stack.is_empty().then_some(value)
}

/// A binary operator over two constants: arithmetic and bitwise operators
/// over numbers, and `+` over a string and a number or string, which
/// concatenates.
fn binary(
    op: EnumConstantBinaryOp,
    left: EnumConstant,
    right: EnumConstant,
) -> Option<EnumConstant> {
    let (EnumConstant::Number(left), EnumConstant::Number(right)) = (&left, &right) else {
        return (op == EnumConstantBinaryOp::Add).then(|| {
            EnumConstant::String(format!("{}{}", left.concatenated(), right.concatenated()))
        });
    };
    let (left, right) = (*left, *right);
    let shift = js_to_uint32(right) & 31;
    Some(EnumConstant::Number(match op {
        EnumConstantBinaryOp::BitOr => f64::from(js_to_int32(left) | js_to_int32(right)),
        EnumConstantBinaryOp::BitAnd => f64::from(js_to_int32(left) & js_to_int32(right)),
        EnumConstantBinaryOp::BitXor => f64::from(js_to_int32(left) ^ js_to_int32(right)),
        EnumConstantBinaryOp::ShiftLeft => f64::from(js_to_int32(left).wrapping_shl(shift)),
        EnumConstantBinaryOp::ShiftRight => f64::from(js_to_int32(left).wrapping_shr(shift)),
        EnumConstantBinaryOp::ShiftRightUnsigned => {
            f64::from(js_to_uint32(left).wrapping_shr(shift))
        }
        EnumConstantBinaryOp::Multiply => left * right,
        EnumConstantBinaryOp::Divide => left / right,
        EnumConstantBinaryOp::Add => left + right,
        EnumConstantBinaryOp::Subtract => left - right,
        EnumConstantBinaryOp::Remainder => left % right,
        // ECMAScript `**`: an exponent of NaN, or of an infinity over a base
        // of magnitude one, is NaN.
        EnumConstantBinaryOp::Exponent => {
            if right.is_nan() || (right.is_infinite() && left.abs() == 1.0) {
                f64::NAN
            } else {
                left.powf(right)
            }
        }
    }))
}

/// ECMAScript `ToInt32`.
fn js_to_int32(value: f64) -> i32 {
    js_to_uint32(value) as i32
}

/// ECMAScript `ToUint32`.
fn js_to_uint32(value: f64) -> u32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    value.trunc().rem_euclid(4_294_967_296.0) as u32
}
