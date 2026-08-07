//! Explicit standalone compilation boundary.

use std::sync::Arc;

use crate::compile::{
    CodegenOptions, VerterCompileOptions, VerterCompileResult, VueMacroSemanticInput,
};
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
    pub fn compile_source(
        &self,
        source: &StandaloneSourceBytes,
        options: &CodegenOptions,
        verter_options: &VerterCompileOptions,
        macro_semantics: &VueMacroSemanticInput,
    ) -> VerterCompileResult {
        let allocator = oxc_allocator::Allocator::new();
        crate::compile::compile(
            source.as_str(),
            options,
            verter_options,
            macro_semantics,
            &allocator,
        )
    }

    /// Compile copied standalone Vue source and retain its exact parse for
    /// output-source-space qualification without a second carrier parse.
    pub fn compile_source_with_parsed(
        &self,
        source: &StandaloneSourceBytes,
        options: &CodegenOptions,
        verter_options: &VerterCompileOptions,
        macro_semantics: &VueMacroSemanticInput,
    ) -> StandaloneCompileOutput {
        let allocator = oxc_allocator::Allocator::new();
        let (parsed, result) = crate::compile::compile_with_parsed(
            source.as_str(),
            options,
            verter_options,
            macro_semantics,
            &allocator,
        );
        StandaloneCompileOutput { parsed, result }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::CompileTarget;

    #[test]
    fn copied_source_is_isolated_from_the_callers_buffer() {
        let mut caller = String::from("<template>before</template>");
        let source = StandaloneSourceBytes::copied_from(&caller);
        caller.replace_range(.., "<template>after</template>");

        let result = StandaloneCompiler.compile_source(
            &source,
            &CodegenOptions {
                target: CompileTarget::BUNDLER,
                ..CodegenOptions::default()
            },
            &VerterCompileOptions::default(),
            &VueMacroSemanticInput::Unavailable,
        );

        assert_eq!(source.as_str(), "<template>before</template>");
        assert!(result.errors.is_empty(), "{:?}", result.errors);
    }
}
