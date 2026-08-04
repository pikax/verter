//! Stylus lexical policy. Expressions remain opaque and unevaluated.

pub(crate) const DOLLAR_INTERPOLATION_PREFIX: &[u8; 2] = b"${";
