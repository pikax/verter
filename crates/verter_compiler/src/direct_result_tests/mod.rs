//! Regression coverage that inspects the pre-assembly, per-block
//! [`super::compile::VerterCompileResult`] directly (raw script/template/
//! style/TSX blocks, before any [`crate::assembly`] composition) — a shape
//! with no equivalent on the public one-shot
//! [`crate::standalone::StandaloneCompiler::compile`] atomic contract, which
//! publishes only the fully composed [`crate::assembly::ArtifactSet`]. These
//! tests drive [`super::compile::compile`] directly and so must live inside
//! the crate (that entry is `pub(crate)`) rather than as `tests/cases/`
//! integration tests.

mod repro_member_access_ide_codegen;
mod style_planner;
mod vdom_ssr_root_prefix_comment_absorption;
