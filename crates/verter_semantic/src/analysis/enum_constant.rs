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
/// unary `+` `-` `~`, the binary arithmetic and bitwise operators, and a
/// value named by an identifier or a static member path — `E["A"]` read as
/// the path `E.A`, `Infinity` and `NaN` as the paths they spell (see
/// [`global_number_spelling`]). `None` for any other expression (a
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

/// One pending step of the initializer read: an expression still to read,
/// or a step to emit once the operands before it are read.
enum ReadStep<'e, 'a> {
    Read(&'e Expression<'a>),
    Emit(EnumConstantStep),
}

/// Read `expr` into `steps` in evaluation order, from an explicit stack so
/// an initializer of any nesting depth reads without a native frame per
/// level.
fn push_steps(expr: &Expression<'_>, steps: &mut Vec<EnumConstantStep>) -> Option<()> {
    let mut pending = vec![ReadStep::Read(expr)];
    while let Some(step) = pending.pop() {
        let expr = match step {
            ReadStep::Emit(step) => {
                steps.push(step);
                continue;
            }
            ReadStep::Read(expr) => expr,
        };
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
                pending.push(ReadStep::Emit(EnumConstantStep::Template(Arc::from(
                    quasis.into_boxed_slice(),
                ))));
                pending.extend(template.expressions.iter().rev().map(ReadStep::Read));
            }
            Expression::ParenthesizedExpression(paren) => {
                pending.push(ReadStep::Read(&paren.expression));
            }
            Expression::UnaryExpression(unary) => {
                let op = match unary.operator {
                    UnaryOperator::UnaryPlus => EnumConstantUnaryOp::Plus,
                    UnaryOperator::UnaryNegation => EnumConstantUnaryOp::Minus,
                    UnaryOperator::BitwiseNot => EnumConstantUnaryOp::BitNot,
                    _ => return None,
                };
                pending.push(ReadStep::Emit(EnumConstantStep::Unary(op)));
                pending.push(ReadStep::Read(&unary.argument));
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
                pending.push(ReadStep::Emit(EnumConstantStep::Binary(op)));
                pending.push(ReadStep::Read(&binary.right));
                pending.push(ReadStep::Read(&binary.left));
            }
            // `Infinity` and `NaN` are references too: they name the global
            // numbers only when no declaration in scope shadows them, which
            // the reader of the reference decides.
            Expression::Identifier(_)
            | Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_) => {
                let path = member_path(expr)?;
                steps.push(EnumConstantStep::Reference(Arc::from(
                    path.into_boxed_slice(),
                )));
            }
            _ => return None,
        }
    }
    Some(())
}

/// The path a static member chain names — `NS.E.A`, `E["A"]` — rooted at an
/// identifier, read from the outermost access inward.
fn member_path(expr: &Expression<'_>) -> Option<Vec<String>> {
    let mut reversed = Vec::new();
    let mut current = expr;
    loop {
        match current {
            Expression::Identifier(identifier) => {
                reversed.push(identifier.name.to_string());
                break;
            }
            Expression::StaticMemberExpression(member) => {
                reversed.push(member.property.name.to_string());
                current = &member.object;
            }
            Expression::ComputedMemberExpression(member) => match &member.expression {
                Expression::StringLiteral(key) => {
                    reversed.push(key.value.to_string());
                    current = &member.object;
                }
                _ => return None,
            },
            Expression::ParenthesizedExpression(paren) => current = &paren.expression,
            _ => return None,
        }
    }
    reversed.reverse();
    Some(reversed)
}

/// The global number `Infinity` or `NaN` a one-segment path spells: the
/// value the path names when the name resolves to the global declaration,
/// or to no declaration at all. `None` for any other path.
#[must_use]
pub fn global_number_spelling(path: &[String]) -> Option<EnumConstant> {
    match path {
        [name] if name == "Infinity" => Some(EnumConstant::Number(f64::INFINITY)),
        [name] if name == "NaN" => Some(EnumConstant::Number(f64::NAN)),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The program of the initializer of `const x = <initializer>;`, read on
    /// a 1 MiB thread (the smallest stack a host asks for).
    fn program_on_a_small_stack(initializer: String) -> Option<EnumConstantExpr> {
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(move || {
                let allocator = oxc_allocator::Allocator::default();
                let source = format!("const x = {initializer};");
                let parsed = verter_parser::oxc_parse::Parser::new(
                    &allocator,
                    &source,
                    oxc_span::SourceType::ts(),
                )
                .parse();
                let Some(oxc_ast::ast::Statement::VariableDeclaration(declaration)) =
                    parsed.program.body.first()
                else {
                    panic!("the source declares a variable");
                };
                enum_constant_expr(declaration.declarations[0].init.as_ref()?)
            })
            .expect("spawn the reading thread")
            .join()
            .expect("the initializer reads")
    }

    /// An initializer 10,000 operators deep (a left-leaning chain, which the
    /// parser reads without a native frame per operator too), and a member
    /// path 10,000 segments long, read into a program without a native
    /// frame per level.
    #[test]
    fn initializers_10000_deep_read_on_a_small_stack() {
        const DEPTH: usize = 10_000;
        let additions = program_on_a_small_stack(format!("{}1", "1 + ".repeat(DEPTH)))
            .expect("an addition chain is a constant expression");
        assert_eq!(additions.steps.len(), 2 * DEPTH + 1);
        assert_eq!(
            evaluate_enum_constant(&additions, |_| None),
            Some(EnumConstant::Number(10_001.0))
        );
        let path = program_on_a_small_stack(format!("a{}", ".b".repeat(DEPTH)))
            .expect("a member path is a reference");
        match &path.steps[..] {
            [EnumConstantStep::Reference(segments)] => assert_eq!(segments.len(), DEPTH + 1),
            steps => panic!(
                "a member path reads as one reference, read {} steps",
                steps.len()
            ),
        }
    }
}
