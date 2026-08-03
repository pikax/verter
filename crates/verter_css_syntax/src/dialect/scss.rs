//! SCSS lexical extensions. This module intentionally performs no Sass evaluation.

pub(crate) const VARIABLE_PREFIX: u8 = b'$';
pub(crate) const INTERPOLATION_PREFIX: &[u8; 2] = b"#{";
