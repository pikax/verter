//! The checker's constant evaluation of enum member initializers.
//!
//! An initializer is read into an [`EnumConstantExpr`] — literals, the
//! values paths name, and the operators the checker folds — and evaluated
//! by one stack machine with ECMAScript's operator semantics. The caller
//! resolves every referenced path: a declaration's own lowering resolves
//! the earlier members of the same enum, and the session resolves the rest
//! (another enum's member, another declaration of a merged enum, a `const`
//! variable), so the two evaluate the same program the same way.
use verter_session_query::enum_constant::number_text;

use std::sync::Arc;

use oxc_ast::ast::{BinaryOperator, Expression, UnaryOperator};
use verter_type_expr::facts::{
    EnumConstantBinaryOp, EnumConstantExpr, EnumConstantStep, EnumConstantUnaryOp,
};

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

#[cfg(test)]
mod tests {
    use super::*;
    use verter_session_query::enum_constant::evaluate_enum_constant;
    use verter_session_query::enum_constant::EnumConstant;

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
