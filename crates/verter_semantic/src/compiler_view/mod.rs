//! Read-only views of a component's semantic facts for the compiler.
//!
//! A view is built from the component's own script analysis and nothing
//! else. No constructor here takes a path, a resolver or a host, so a view
//! cannot read another file.

mod script_origins;

pub use script_origins::{
    CalleeRef, DeclarationKind, InitializerFact, OriginStep, ResolvedOrigin, ScriptOriginView,
};
