//! Explicit standalone compilation boundary.
//!
//! [`StandaloneCompiler`] is the internal compiler's sole public
//! request-construction point ("R1", the internal compiler one-shot route):
//! every caller supplies a canonical [`crate::compile_request::CompileRequest`]
//! (built through [`CompileRequest::new`](crate::compile_request::CompileRequest::new),
//! which enforces every construction-time fail-closed rule) plus the
//! ephemeral [`VueExecutionInputs`] carrier for resolved framework facts
//! excluded from request identity. There is no second way to reach
//! [`crate::compile::compile`] — the former `&CodegenOptions` /
//! `&VerterCompileOptions` parameters, both independently caller-
//! constructible second option authorities, are gone.

use std::sync::Arc;

use crate::compile::types::VueExecutionInputs;
use crate::compile::{VerterCompileResult, VueMacroSemanticInput};
use crate::compile_request::{CompileRequest, CompileRequestError};
use crate::parser::types::ParsedSfc;

/// One standalone compile plus the exact parse that produced it.
pub struct StandaloneCompileOutput {
    pub parsed: ParsedSfc,
    pub result: VerterCompileResult,
}

/// Bytes owned by a standalone request, disjoint from registered host sources.
#[derive(Debug, Clone)]
pub struct StandaloneSourceBytes(Arc<str>);

impl StandaloneSourceBytes {
    /// Copy caller bytes into the standalone ownership domain.
    pub fn copied_from(source: &str) -> Self {
        Self(Arc::from(source))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stateless compiler for callers that do not participate in a registered host.
#[derive(Debug, Default, Clone, Copy)]
pub struct StandaloneCompiler;

impl StandaloneCompiler {
    /// Compile copied standalone Vue source. This is the only public raw-source
    /// parser boundary; registered hosts consume their elected artifact.
    ///
    /// Returns the typed refusal for the two `SSR x Vapor` / `inline x
    /// Vapor` cases the canonical request's own construction could not see
    /// (backend resolution needs the parsed source) — every other
    /// fail-closed rule was already enforced when `request` was built.
    pub fn compile_source(
        &self,
        source: &StandaloneSourceBytes,
        request: &CompileRequest,
        execution_inputs: &VueExecutionInputs,
        macro_semantics: &VueMacroSemanticInput,
    ) -> Result<VerterCompileResult, CompileRequestError> {
        let allocator = oxc_allocator::Allocator::new();
        crate::compile::compile(
            source.as_str(),
            request,
            execution_inputs,
            macro_semantics,
            &allocator,
        )
    }

    /// Compile copied standalone Vue source and retain its exact parse for
    /// output-source-space qualification without a second carrier parse.
    pub fn compile_source_with_parsed(
        &self,
        source: &StandaloneSourceBytes,
        request: &CompileRequest,
        execution_inputs: &VueExecutionInputs,
        macro_semantics: &VueMacroSemanticInput,
    ) -> Result<StandaloneCompileOutput, CompileRequestError> {
        let allocator = oxc_allocator::Allocator::new();
        let (parsed, result) = crate::compile::compile_with_parsed(
            source.as_str(),
            request,
            execution_inputs,
            macro_semantics,
            &allocator,
        )?;
        Ok(StandaloneCompileOutput { parsed, result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile_request::{
        CompileProduct, CompileRequestError, FrameworkCompileRequest, RuntimeProductRequest,
        SvelteCompileRequest, VueCompileRequest,
    };

    #[test]
    fn copied_source_is_isolated_from_the_callers_buffer() {
        let mut caller = String::from("<template>before</template>");
        let source = StandaloneSourceBytes::copied_from(&caller);
        caller.replace_range(.., "<template>after</template>");

        let request = CompileRequest::new(
            vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )],
            FrameworkCompileRequest::Vue(VueCompileRequest::default()),
            None,
            None,
            None,
            false,
            false,
        )
        .expect("a single RuntimeClient product must construct");

        let result = StandaloneCompiler
            .compile_source(
                &source,
                &request,
                &VueExecutionInputs::default(),
                &VueMacroSemanticInput::Unavailable,
            )
            .expect("a plain RuntimeClient compile must not be refused");

        assert_eq!(source.as_str(), "<template>before</template>");
        assert!(result.errors.is_empty(), "{:?}", result.errors);
    }

    #[test]
    fn compile_source_refuses_a_svelte_request_instead_of_panicking() {
        let source = StandaloneSourceBytes::copied_from("<template>hi</template>");

        let request = CompileRequest::new(
            vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )],
            FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
            None,
            None,
            None,
            false,
            false,
        )
        .expect("a single RuntimeClient product must construct regardless of framework");

        let error = match StandaloneCompiler.compile_source(
            &source,
            &request,
            &VueExecutionInputs::default(),
            &VueMacroSemanticInput::Unavailable,
        ) {
            Ok(_) => panic!("a Svelte request must not reach the Vue-only carrier driver"),
            Err(error) => error,
        };

        assert!(
            matches!(
                error,
                CompileRequestError::FrameworkMismatch {
                    expected: "Vue",
                    actual: "Svelte",
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn compile_source_with_parsed_refuses_a_svelte_request_instead_of_panicking() {
        let source = StandaloneSourceBytes::copied_from("<template>hi</template>");

        let request = CompileRequest::new(
            vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )],
            FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
            None,
            None,
            None,
            false,
            false,
        )
        .expect("a single RuntimeClient product must construct regardless of framework");

        let error = match StandaloneCompiler.compile_source_with_parsed(
            &source,
            &request,
            &VueExecutionInputs::default(),
            &VueMacroSemanticInput::Unavailable,
        ) {
            Ok(_) => panic!("a Svelte request must not reach the Vue-only carrier driver"),
            Err(error) => error,
        };

        assert!(
            matches!(
                error,
                CompileRequestError::FrameworkMismatch {
                    expected: "Vue",
                    actual: "Svelte",
                }
            ),
            "{error:?}"
        );
    }
}
