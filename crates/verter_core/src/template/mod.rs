//! Template codegen — converts the template AST to render functions.
//!
//! Uses a strategy pattern: a shared tree walker dispatches to a
//! [`code_gen::TemplateCodeGen`](code_gen::TemplateCodeGen) trait, with VDOM
//! and Vapor backends as concrete implementations. OXC is used for expression
//! parsing and binding resolution within template interpolations and directives.

pub mod code_gen;
pub mod oxc;
