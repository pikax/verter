//! Enum member constants and their AST-free evaluation.

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
pub fn number_text(value: f64) -> String {
    format!("{value}")
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
pub fn binary(
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
