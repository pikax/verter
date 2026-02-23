//! Compiler diagnostic types and error codes.
//!
//! Provides Vue-compatible error codes ([`CompilerErrorCode`]) and diagnostic
//! construction helpers. Error codes prefixed with `X_` are template validation
//! errors; others are standard HTML parse errors.

use crate::{common::Span, utils::vue::is_void_tag};

// =============================================================================
// Compiler Error Codes (Vue-compatible)
// =============================================================================

/// Error codes compatible with Vue's `@vue/compiler-core` error system.
///
/// Codes prefixed with `X_` are Vue-specific (template validation).
/// Other codes are standard HTML parse errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompilerErrorCode {
    // -- HTML parse errors --
    DuplicateAttribute,
    EndTagWithAttributes,
    EofBeforeTagName,
    EofInTag,
    MissingAttributeValue,
    MissingEndTagName,
    MissingWhitespaceBetweenAttributes,

    // -- Vue template errors --
    /// Invalid end tag — close tag doesn't match any open element.
    XInvalidEndTag,
    /// Element is missing end tag — unclosed element at end of input.
    XMissingEndTag,
    /// Interpolation end sign was not found.
    XMissingInterpolationEnd,
    /// Legal directive name was expected.
    XMissingDirectiveName,
    /// End bracket for dynamic directive argument not found.
    XMissingDynamicDirectiveArgumentEnd,

    // -- Directive validation errors --
    /// v-if/v-else-if is missing expression.
    XVIfNoExpression,
    /// v-if/else branches must use unique keys.
    XVIfSameKey,
    /// v-else/v-else-if has no adjacent v-if or v-else-if.
    XVElseNoAdjacentIf,
    /// v-for is missing expression.
    XVForNoExpression,
    /// v-for has invalid expression.
    XVForMalformedExpression,
    /// v-bind is missing expression.
    XVBindNoExpression,
    /// v-on is missing expression.
    XVOnNoExpression,
    /// v-slot only works on components or `<template>` tags.
    XVSlotMisplaced,
    /// Duplicate slot names found.
    XVSlotDuplicateSlotNames,
    /// v-model is missing expression.
    XVModelNoExpression,
    /// v-model value must be valid JavaScript member expression.
    XVModelMalformedExpression,

    // -- Script errors --
    /// Duplicate `<script setup>` block.
    DuplicateScriptSetup,
    /// Duplicate `<script>` block.
    DuplicateScript,

    // -- Directive duplication --
    /// Duplicate built-in directive on the same element (v-if, v-for, v-slot, v-once).
    XDuplicateDirective,

    // -- Expression errors --
    /// Error parsing JavaScript expression.
    XInvalidExpression,

    // -- Style errors --
    /// Error parsing or processing CSS in a `<style>` block.
    XCssParseError,
}

impl CompilerErrorCode {
    /// The default human-readable message for this error code (Vue-compatible).
    pub fn message(&self) -> &'static str {
        match self {
            Self::DuplicateAttribute => "Duplicate attribute.",
            Self::EndTagWithAttributes => "End tag cannot have attributes.",
            Self::EofBeforeTagName => "Unexpected EOF in tag.",
            Self::EofInTag => "Unexpected EOF in tag.",
            Self::MissingAttributeValue => "Attribute value was expected.",
            Self::MissingEndTagName => "End tag name was expected.",
            Self::MissingWhitespaceBetweenAttributes => "Whitespace was expected.",
            Self::XInvalidEndTag => "Invalid end tag.",
            Self::XMissingEndTag => "Element is missing end tag.",
            Self::XMissingInterpolationEnd => "Interpolation end sign was not found.",
            Self::XMissingDirectiveName => "Legal directive name was expected.",
            Self::XMissingDynamicDirectiveArgumentEnd => {
                "End bracket for dynamic directive argument not found; spaces disallowed."
            }
            Self::XVIfNoExpression => "v-if/v-else-if is missing expression.",
            Self::XVIfSameKey => "v-if/else branches must use unique keys.",
            Self::XVElseNoAdjacentIf => "v-else/v-else-if has no adjacent v-if or v-else-if.",
            Self::XVForNoExpression => "v-for is missing expression.",
            Self::XVForMalformedExpression => "v-for has invalid expression.",
            Self::XVBindNoExpression => "v-bind is missing expression.",
            Self::XVOnNoExpression => "v-on is missing expression.",
            Self::XVSlotMisplaced => "v-slot only works on components or <template> tags.",
            Self::XVSlotDuplicateSlotNames => "Duplicate slot names found.",
            Self::XVModelNoExpression => "v-model is missing expression.",
            Self::XVModelMalformedExpression => {
                "v-model value must be valid JavaScript member expression."
            }
            Self::DuplicateScriptSetup => {
                "Duplicate <script setup> block — only one <script setup> is allowed per SFC."
            }
            Self::DuplicateScript => {
                "Duplicate <script> block — only one plain <script> is allowed per SFC."
            }
            Self::XDuplicateDirective => "Duplicate built-in directive on the same element.",
            Self::XInvalidExpression => "Error parsing JavaScript expression.",
            Self::XCssParseError => "Error parsing or processing CSS.",
        }
    }
}

impl std::fmt::Display for CompilerErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

// =============================================================================
// Diagnostics
// =============================================================================

/// Severity level for a diagnostic message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    /// Informational message — does not indicate a problem.
    Info,
    /// Warning — potential issue that doesn't prevent compilation.
    Warning,
    /// Error — compilation problem. May be recoverable (pipeline continues)
    /// or critical (plugin returns `SyntaxResult::Stop`).
    Error,
}

/// A diagnostic message emitted by a plugin during compilation.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    /// Vue-compatible error code.
    pub code: CompilerErrorCode,
    /// Which plugin emitted this diagnostic.
    pub plugin: &'static str,
    /// Human-readable message.
    pub message: String,
    /// Optional source span for the diagnostic.
    pub span: Option<Span>,
}

impl Diagnostic {
    /// Create an error diagnostic with a Vue-compatible error code.
    pub fn error(plugin: &'static str, code: CompilerErrorCode) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            code,
            plugin,
            message: code.message().to_string(),
            span: None,
        }
    }

    /// Create an error with a custom message (overrides the default code message).
    pub fn error_with_message(
        plugin: &'static str,
        code: CompilerErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            code,
            plugin,
            message: message.into(),
            span: None,
        }
    }

    /// Create a warning diagnostic with a Vue-compatible error code.
    pub fn warning(plugin: &'static str, code: CompilerErrorCode) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            code,
            plugin,
            message: code.message().to_string(),
            span: None,
        }
    }

    /// Create an informational diagnostic.
    pub fn info(plugin: &'static str, code: CompilerErrorCode) -> Self {
        Self {
            severity: DiagnosticSeverity::Info,
            code,
            plugin,
            message: code.message().to_string(),
            span: None,
        }
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let severity = match self.severity {
            DiagnosticSeverity::Info => "info",
            DiagnosticSeverity::Warning => "warning",
            DiagnosticSeverity::Error => "error",
        };
        write!(f, "[{}] {}: {}", self.plugin, severity, self.message)
    }
}

// =============================================================================
// Plugin Options and Context
// =============================================================================

/// Predicate for checking whether a tag is a custom element.
pub type IsCustomElementFn = Box<dyn Fn(&[u8]) -> bool>;

pub struct SyntaxPluginOptions {
    // if the tag does not need a closing tag
    pub is_void_tag: fn(&[u8]) -> bool,
    // if the tag is a custom element
    pub is_custom_element: IsCustomElementFn,
}

impl std::fmt::Debug for SyntaxPluginOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyntaxPluginOptions")
            .field("is_void_tag", &"fn(&[u8]) -> bool")
            .field("is_custom_element", &"Box<dyn Fn(&[u8]) -> bool>")
            .finish()
    }
}

impl std::default::Default for SyntaxPluginOptions {
    fn default() -> Self {
        Self {
            is_void_tag,
            is_custom_element: Box::new(|_tag_name: &[u8]| false),
        }
    }
}

pub struct SyntaxPluginContext<'a> {
    pub input: &'a str,
    pub bytes: &'a [u8],
    pub options: &'a SyntaxPluginOptions,
    pub diagnostics: Vec<Diagnostic>,
}

impl<'a> SyntaxPluginContext<'a> {
    /// Report an error diagnostic with a Vue-compatible error code.
    pub fn error(&mut self, plugin: &'static str, code: CompilerErrorCode) {
        self.diagnostics.push(Diagnostic::error(plugin, code));
    }

    /// Report an error with a source span.
    pub fn error_at(&mut self, plugin: &'static str, code: CompilerErrorCode, span: Span) {
        self.diagnostics
            .push(Diagnostic::error(plugin, code).with_span(span));
    }

    /// Report an error with a custom message and source span.
    pub fn error_at_with_message(
        &mut self,
        plugin: &'static str,
        code: CompilerErrorCode,
        message: impl Into<String>,
        span: Span,
    ) {
        self.diagnostics
            .push(Diagnostic::error_with_message(plugin, code, message).with_span(span));
    }

    /// Report a warning diagnostic.
    pub fn warn(&mut self, plugin: &'static str, code: CompilerErrorCode) {
        self.diagnostics.push(Diagnostic::warning(plugin, code));
    }

    /// Report a warning with a source span.
    pub fn warn_at(&mut self, plugin: &'static str, code: CompilerErrorCode, span: Span) {
        self.diagnostics
            .push(Diagnostic::warning(plugin, code).with_span(span));
    }

    /// Check if any error-level diagnostics have been reported.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error)
    }

    /// Get all diagnostics.
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_error_creation() {
        let d = Diagnostic::error("test-plugin", CompilerErrorCode::XInvalidEndTag);
        assert_eq!(d.severity, DiagnosticSeverity::Error);
        assert_eq!(d.plugin, "test-plugin");
        assert_eq!(d.message, "Invalid end tag.");
        assert_eq!(d.code, CompilerErrorCode::XInvalidEndTag);
        assert!(d.span.is_none());
    }

    #[test]
    fn test_diagnostic_error_with_custom_message() {
        let d = Diagnostic::error_with_message(
            "test",
            CompilerErrorCode::XMissingEndTag,
            "Element <div> is missing end tag.",
        );
        assert_eq!(d.code, CompilerErrorCode::XMissingEndTag);
        assert_eq!(d.message, "Element <div> is missing end tag.");
    }

    #[test]
    fn test_diagnostic_warning_creation() {
        let d = Diagnostic::warning("parser", CompilerErrorCode::DuplicateAttribute);
        assert_eq!(d.severity, DiagnosticSeverity::Warning);
        assert_eq!(d.plugin, "parser");
        assert_eq!(d.message, "Duplicate attribute.");
    }

    #[test]
    fn test_diagnostic_with_span() {
        let d = Diagnostic::error("test", CompilerErrorCode::XInvalidEndTag)
            .with_span(Span { start: 10, end: 20 });
        assert_eq!(d.span, Some(Span { start: 10, end: 20 }));
    }

    #[test]
    fn test_diagnostic_display() {
        let d = Diagnostic::error("script", CompilerErrorCode::XInvalidExpression);
        assert_eq!(
            d.to_string(),
            "[script] error: Error parsing JavaScript expression."
        );

        let d = Diagnostic::warning("template", CompilerErrorCode::DuplicateAttribute);
        assert_eq!(d.to_string(), "[template] warning: Duplicate attribute.");
    }

    #[test]
    fn test_diagnostic_severity_ordering() {
        assert!(DiagnosticSeverity::Info < DiagnosticSeverity::Warning);
        assert!(DiagnosticSeverity::Warning < DiagnosticSeverity::Error);
    }

    #[test]
    fn test_error_code_messages() {
        assert_eq!(
            CompilerErrorCode::XMissingEndTag.message(),
            "Element is missing end tag."
        );
        assert_eq!(
            CompilerErrorCode::XInvalidEndTag.message(),
            "Invalid end tag."
        );
        assert_eq!(
            CompilerErrorCode::XVIfNoExpression.message(),
            "v-if/v-else-if is missing expression."
        );
    }

    #[test]
    fn test_context_error_helpers() {
        let opts = SyntaxPluginOptions::default();
        let input = "";
        let mut ctx = SyntaxPluginContext {
            input,
            bytes: input.as_bytes(),
            options: &opts,
            diagnostics: Vec::new(),
        };

        assert!(!ctx.has_errors());

        ctx.error("plugin-a", CompilerErrorCode::XInvalidEndTag);
        assert!(ctx.has_errors());
        assert_eq!(ctx.diagnostics.len(), 1);

        ctx.warn("plugin-a", CompilerErrorCode::DuplicateAttribute);
        assert_eq!(ctx.diagnostics.len(), 2);
    }

    #[test]
    fn test_context_error_at_with_span() {
        let opts = SyntaxPluginOptions::default();
        let input = "<div>bad</div>";
        let mut ctx = SyntaxPluginContext {
            input,
            bytes: input.as_bytes(),
            options: &opts,
            diagnostics: Vec::new(),
        };

        ctx.error_at(
            "parser",
            CompilerErrorCode::XInvalidEndTag,
            Span { start: 0, end: 5 },
        );
        assert_eq!(ctx.diagnostics.len(), 1);
        assert_eq!(ctx.diagnostics[0].span, Some(Span { start: 0, end: 5 }));
        assert_eq!(ctx.diagnostics[0].code, CompilerErrorCode::XInvalidEndTag);
    }

    #[test]
    fn test_context_take_diagnostics() {
        let opts = SyntaxPluginOptions::default();
        let input = "";
        let mut ctx = SyntaxPluginContext {
            input,
            bytes: input.as_bytes(),
            options: &opts,
            diagnostics: Vec::new(),
        };

        ctx.error("a", CompilerErrorCode::XInvalidEndTag);
        ctx.warn("b", CompilerErrorCode::DuplicateAttribute);

        let diags = ctx.take_diagnostics();
        assert_eq!(diags.len(), 2);
        assert!(ctx.diagnostics.is_empty(), "take_diagnostics should drain");
    }

    #[test]
    fn test_has_errors_ignores_warnings() {
        let opts = SyntaxPluginOptions::default();
        let input = "";
        let mut ctx = SyntaxPluginContext {
            input,
            bytes: input.as_bytes(),
            options: &opts,
            diagnostics: Vec::new(),
        };

        ctx.warn("a", CompilerErrorCode::DuplicateAttribute);
        assert!(!ctx.has_errors(), "warnings should not count as errors");

        ctx.error("c", CompilerErrorCode::XMissingEndTag);
        assert!(ctx.has_errors());
    }
}
