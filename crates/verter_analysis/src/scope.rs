//! Bitwise flags controlling which analysis passes run.
//!
//! Replaces the coarse `AnalysisLevel` enum (Full/Essential/None) with fine-grained
//! flags so different consumers (build, LSP, linter) request exactly the analysis
//! they need.

bitflags::bitflags! {
    /// Controls which analysis passes run during file upsert.
    ///
    /// Different consumers need different analysis depth:
    /// - **Build**: Script + direct type deps for macro resolution (minimal overhead)
    /// - **LSP**: Full analysis for completions, hover, diagnostics
    /// - **Linter**: Script + template bindings + Vue API validation
    ///
    /// Use the preset constants ([`BUILD`](Self::BUILD), [`LSP`](Self::LSP), etc.)
    /// or combine individual flags with bitwise OR.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AnalysisScope: u32 {
        // ── Script (bits 0–7) ──

        /// Import declarations.
        const IMPORTS           = 1 << 0;
        /// Variable/function/class declarations.
        const BINDINGS          = 1 << 1;
        /// Ref/reactive/computed classification.
        const REACTIVITY        = 1 << 2;
        /// defineProps/Emits/Model/Slots/Expose.
        const MACROS            = 1 << 3;
        /// Cross-file type references in macros.
        const MACRO_TYPE_DEPS   = 1 << 4;
        /// Track provide/inject/lifecycle/watcher calls.
        const VUE_API_USAGE     = 1 << 5;
        /// Per-export hashes for smart invalidation.
        const EXPORT_SIGNATURES = 1 << 6;
        /// Analyze function return reactivity (for composables).
        const FUNC_RETURNS      = 1 << 7;

        // ── Template (bits 8–15) ──

        /// Component usages + prop expressions.
        const TPL_COMPONENTS    = 1 << 8;
        /// Which script bindings are used in template.
        const TPL_BINDINGS      = 1 << 9;
        /// Slot definitions + usages.
        const TPL_SLOTS         = 1 << 10;
        /// Template ref attributes.
        const TPL_REFS          = 1 << 11;
        /// Event handler bindings.
        const TPL_EVENTS        = 1 << 12;
        /// Prop constness classification.
        const TPL_CONSTNESS     = 1 << 13;

        // ── Style (bits 16–19) ──

        /// Full CSS analysis (selectors, classes, IDs).
        const STYLE_CSS         = 1 << 16;
        /// v-bind() in styles.
        const STYLE_VBIND       = 1 << 17;
        /// Scoped/module metadata.
        const STYLE_SCOPED      = 1 << 18;
        /// :deep/:global/:slotted.
        const STYLE_PSEUDOS     = 1 << 19;

        // ── Cross-file (bits 24–26) ──

        /// Build render tree from template analysis.
        const CROSS_RENDER_TREE = 1 << 24;
        /// Provide/inject chain validation.
        const CROSS_PROVIDE     = 1 << 25;
        /// Prop constness optimization.
        const CROSS_PROP_CONST  = 1 << 26;
    }
}

impl serde::Serialize for AnalysisScope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.bits())
    }
}

impl<'de> serde::Deserialize<'de> for AnalysisScope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bits = u32::deserialize(deserializer)?;
        Ok(Self::from_bits_truncate(bits))
    }
}

impl AnalysisScope {
    // ── Presets ──

    /// Build mode: script analysis + direct type deps for macro resolution.
    /// Minimal overhead, enough for compilation + smart invalidation.
    pub const BUILD: Self = Self::from_bits_truncate(
        Self::IMPORTS.bits()
            | Self::BINDINGS.bits()
            | Self::MACROS.bits()
            | Self::MACRO_TYPE_DEPS.bits()
            | Self::EXPORT_SIGNATURES.bits()
            | Self::STYLE_VBIND.bits()
            | Self::STYLE_SCOPED.bits(),
    );

    /// Build + cross-file optimization: adds template analysis + constness.
    pub const BUILD_OPTIMIZED: Self = Self::from_bits_truncate(
        Self::BUILD.bits()
            | Self::REACTIVITY.bits()
            | Self::VUE_API_USAGE.bits()
            | Self::TPL_COMPONENTS.bits()
            | Self::TPL_BINDINGS.bits()
            | Self::TPL_CONSTNESS.bits()
            | Self::CROSS_RENDER_TREE.bits()
            | Self::CROSS_PROVIDE.bits()
            | Self::CROSS_PROP_CONST.bits(),
    );

    /// LSP mode: full analysis for completions, hover, diagnostics, etc.
    pub const LSP: Self = Self::all();

    /// Linter mode: script + template bindings + Vue API validation.
    pub const LINTER: Self = Self::from_bits_truncate(
        Self::IMPORTS.bits()
            | Self::BINDINGS.bits()
            | Self::REACTIVITY.bits()
            | Self::MACROS.bits()
            | Self::VUE_API_USAGE.bits()
            | Self::TPL_COMPONENTS.bits()
            | Self::TPL_BINDINGS.bits()
            | Self::TPL_SLOTS.bits()
            | Self::TPL_REFS.bits()
            | Self::TPL_EVENTS.bits(),
    );

    /// Equivalent to the old `AnalysisLevel::Essential`.
    /// Script analysis only; skips style and template analysis.
    pub const ESSENTIAL: Self = Self::from_bits_truncate(
        Self::IMPORTS.bits()
            | Self::BINDINGS.bits()
            | Self::MACROS.bits()
            | Self::MACRO_TYPE_DEPS.bits()
            | Self::EXPORT_SIGNATURES.bits(),
    );

    /// Equivalent to the old `AnalysisLevel::None`.
    /// Only SFC tokenization and hashing for compilation.
    pub const NONE: Self = Self::empty();

    /// Returns `true` if script analysis (imports, bindings, macros) should run.
    pub fn needs_script_analysis(self) -> bool {
        self.intersects(
            Self::IMPORTS
                .union(Self::BINDINGS)
                .union(Self::MACROS)
                .union(Self::REACTIVITY)
                .union(Self::VUE_API_USAGE)
                .union(Self::FUNC_RETURNS),
        )
    }

    /// Returns `true` if style analysis (CSS parsing) should run.
    pub fn needs_style_analysis(self) -> bool {
        self.intersects(
            Self::STYLE_CSS
                .union(Self::STYLE_VBIND)
                .union(Self::STYLE_SCOPED)
                .union(Self::STYLE_PSEUDOS),
        )
    }

    /// Returns `true` if full CSS analysis (scanner-based) should run.
    /// When only `STYLE_VBIND` or `STYLE_SCOPED` is set, we can skip
    /// the CSS scanner and use lightweight extraction.
    pub fn needs_full_css_analysis(self) -> bool {
        self.contains(Self::STYLE_CSS)
    }

    /// Returns `true` if template analysis should run.
    pub fn needs_template_analysis(self) -> bool {
        self.intersects(
            Self::TPL_COMPONENTS
                .union(Self::TPL_BINDINGS)
                .union(Self::TPL_SLOTS)
                .union(Self::TPL_REFS)
                .union(Self::TPL_EVENTS)
                .union(Self::TPL_CONSTNESS),
        )
    }

    /// Returns `true` if cross-file analysis should run.
    pub fn needs_cross_file_analysis(self) -> bool {
        self.intersects(
            Self::CROSS_RENDER_TREE
                .union(Self::CROSS_PROVIDE)
                .union(Self::CROSS_PROP_CONST),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @ai-generated - BUILD preset has expected flags
    #[test]
    fn build_preset_has_expected_flags() {
        let scope = AnalysisScope::BUILD;
        assert!(scope.contains(AnalysisScope::IMPORTS));
        assert!(scope.contains(AnalysisScope::BINDINGS));
        assert!(scope.contains(AnalysisScope::MACROS));
        assert!(scope.contains(AnalysisScope::MACRO_TYPE_DEPS));
        assert!(scope.contains(AnalysisScope::EXPORT_SIGNATURES));
        assert!(scope.contains(AnalysisScope::STYLE_VBIND));
        assert!(scope.contains(AnalysisScope::STYLE_SCOPED));
        // Should NOT have template or cross-file
        assert!(!scope.contains(AnalysisScope::TPL_COMPONENTS));
        assert!(!scope.contains(AnalysisScope::CROSS_RENDER_TREE));
        assert!(!scope.contains(AnalysisScope::STYLE_CSS));
        assert!(!scope.contains(AnalysisScope::REACTIVITY));
    }

    /// @ai-generated - LSP preset has all flags
    #[test]
    fn lsp_preset_has_all_flags() {
        let scope = AnalysisScope::LSP;
        assert_eq!(scope, AnalysisScope::all());
        assert!(scope.contains(AnalysisScope::IMPORTS));
        assert!(scope.contains(AnalysisScope::STYLE_CSS));
        assert!(scope.contains(AnalysisScope::TPL_COMPONENTS));
        assert!(scope.contains(AnalysisScope::CROSS_RENDER_TREE));
        assert!(scope.contains(AnalysisScope::FUNC_RETURNS));
    }

    /// @ai-generated - LINTER preset has template flags
    #[test]
    fn linter_preset_has_template_flags() {
        let scope = AnalysisScope::LINTER;
        assert!(scope.contains(AnalysisScope::TPL_COMPONENTS));
        assert!(scope.contains(AnalysisScope::TPL_BINDINGS));
        assert!(scope.contains(AnalysisScope::TPL_SLOTS));
        assert!(scope.contains(AnalysisScope::TPL_REFS));
        assert!(scope.contains(AnalysisScope::TPL_EVENTS));
        assert!(scope.contains(AnalysisScope::REACTIVITY));
        // Should NOT have cross-file or style CSS
        assert!(!scope.contains(AnalysisScope::CROSS_RENDER_TREE));
        assert!(!scope.contains(AnalysisScope::STYLE_CSS));
    }

    /// @ai-generated - Scope flag check correctly reports analysis needs
    #[test]
    fn scope_flag_check_skips_unneeded_analysis() {
        let scope = AnalysisScope::BUILD;
        assert!(scope.needs_script_analysis());
        assert!(scope.needs_style_analysis()); // STYLE_VBIND + STYLE_SCOPED
        assert!(!scope.needs_full_css_analysis()); // No STYLE_CSS
        assert!(!scope.needs_template_analysis());
        assert!(!scope.needs_cross_file_analysis());
    }

    /// @ai-generated - ESSENTIAL preset matches old Essential level behavior
    #[test]
    fn migrate_essential_level_to_scope() {
        let scope = AnalysisScope::ESSENTIAL;
        // Essential ran script analysis (imports, macros, bindings) but not style
        assert!(scope.needs_script_analysis());
        assert!(!scope.needs_style_analysis());
        assert!(!scope.needs_template_analysis());
        assert!(!scope.needs_cross_file_analysis());
    }

    /// @ai-generated - LSP preset matches old Full level behavior
    #[test]
    fn migrate_full_level_to_scope() {
        let scope = AnalysisScope::LSP;
        // Full ran everything
        assert!(scope.needs_script_analysis());
        assert!(scope.needs_style_analysis());
        assert!(scope.needs_full_css_analysis());
        assert!(scope.needs_template_analysis());
        assert!(scope.needs_cross_file_analysis());
    }

    /// @ai-generated - NONE preset matches old None level behavior
    #[test]
    fn none_preset_skips_all() {
        let scope = AnalysisScope::NONE;
        assert!(!scope.needs_script_analysis());
        assert!(!scope.needs_style_analysis());
        assert!(!scope.needs_template_analysis());
        assert!(!scope.needs_cross_file_analysis());
    }

    /// @ai-generated - BUILD_OPTIMIZED extends BUILD with optimization flags
    #[test]
    fn build_optimized_extends_build() {
        let build = AnalysisScope::BUILD;
        let optimized = AnalysisScope::BUILD_OPTIMIZED;
        // Optimized should contain everything BUILD has
        assert!(optimized.contains(build));
        // Plus optimization flags
        assert!(optimized.contains(AnalysisScope::REACTIVITY));
        assert!(optimized.contains(AnalysisScope::TPL_COMPONENTS));
        assert!(optimized.contains(AnalysisScope::TPL_BINDINGS));
        assert!(optimized.contains(AnalysisScope::TPL_CONSTNESS));
        assert!(optimized.contains(AnalysisScope::CROSS_RENDER_TREE));
        assert!(optimized.contains(AnalysisScope::CROSS_PROVIDE));
        assert!(optimized.contains(AnalysisScope::CROSS_PROP_CONST));
    }

    /// @ai-generated - Serialization round-trip preserves flags
    #[test]
    fn serde_roundtrip() {
        let scope = AnalysisScope::BUILD_OPTIMIZED;
        let json = serde_json::to_string(&scope).unwrap();
        let deserialized: AnalysisScope = serde_json::from_str(&json).unwrap();
        assert_eq!(scope, deserialized);
    }
}
