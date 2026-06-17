#![deny(missing_docs)]
//! Standalone Svelte rune-module (`.svelte.ts`/`.svelte.js`) provider surface.
//!
//! A rune module is a NON-COMPONENT carrier: it has no template, no
//! component API, and never dispatches through the carrier parse path. Its
//! TypeProvider surface (Channel B) is `<module rune prelude> + <real module
//! bytes>`, served from the module's OWN canonical path so a consumer resolving
//! the module from disk sees its inferred rune-derived exported types
//! (`export const s = $state(0)` ⇒ `s: number`).
//!
//! The prelude is the SINGLE shared rune source rendered in
//! [`RunePreludeMode::Module`](verter_compiler::svelte::ide::prelude::RunePreludeMode::Module),
//! with a leading `export {};` keeping the prepended declarations MODULE-LOCAL
//! (a bare top-level `declare` in a script-context file leaks the runes
//! globally). The `.ts` form uses TS `declare`; the `.js` form uses JS-valid
//! JSDoc-typed functions (checked under `checkJs`). The prelude is prepended as
//! WHOLE LINES, so the user-source positions shift by exactly
//! [`RuneModuleProviderContent::prelude_line_count`] lines — a uniform offset a
//! position mapper applies to the rune module's own diagnostics/hover.

use verter_compiler::svelte::ide::prelude::{
    render_rune_prelude, RuneModuleSourceType, RunePreludeMode,
};
use verter_language::{FileLanguage, ScriptSourceType};

/// The provider content for a Svelte rune module, plus the prelude line count
/// the position mapper applies to map the provider content back to the real
/// module source.
#[derive(Debug, Clone)]
pub struct RuneModuleProviderContent {
    /// `<module rune prelude> + <module bytes>` — the content fed to the
    /// TypeProvider for the rune module's canonical path.
    pub content: String,
    /// The number of whole lines the prelude prepends. Every original
    /// module-source line is shifted down by exactly this many lines in
    /// [`Self::content`]; columns are unchanged.
    pub prelude_line_count: u32,
}

/// Build the rune-module provider content for `module_bytes` when `language` is
/// a Svelte standalone rune module; `None` for every other file (a plain
/// script's provider content is its bytes verbatim — no prelude, per-file
/// scoping).
///
/// `module_bytes` is the post-import-rewrite module source (the same bytes a
/// plain script would be fed). The rune prelude is prepended whole-line.
#[must_use]
pub fn rune_module_provider_content(
    language: &FileLanguage,
    module_bytes: &str,
) -> Option<RuneModuleProviderContent> {
    let source_type = svelte_rune_module_source_type(language)?;
    let prelude = render_rune_prelude(RunePreludeMode::Module { source_type });
    let prelude_line_count =
        u32::try_from(prelude.bytes().filter(|&b| b == b'\n').count()).unwrap_or(u32::MAX);
    let content = format!("{prelude}{module_bytes}");
    Some(RuneModuleProviderContent {
        content,
        prelude_line_count,
    })
}

/// The rune-module prelude dialect for `language`, when it is a Svelte rune
/// module (`.svelte.ts` ⇒ TS form, `.svelte.js` ⇒ JS form). `None` otherwise.
#[must_use]
pub fn svelte_rune_module_source_type(language: &FileLanguage) -> Option<RuneModuleSourceType> {
    let (adapter, lang) = language.adapter_script_language()?;
    if !adapter.is_svelte() || lang.as_str() != verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID {
        return None;
    }
    match language.script_source_type()? {
        ScriptSourceType::Ts | ScriptSourceType::Tsx | ScriptSourceType::Dts => {
            Some(RuneModuleSourceType::Ts)
        }
        ScriptSourceType::Js(_) | ScriptSourceType::Jsx(_) => Some(RuneModuleSourceType::Js),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_language::{FrameworkAdapterId, LanguageId};

    fn rune(source_type: ScriptSourceType) -> FileLanguage {
        FileLanguage::adapter_module(
            source_type,
            FrameworkAdapterId::svelte(),
            LanguageId::new(verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID),
        )
    }

    #[test]
    fn ts_rune_module_content_is_prelude_plus_bytes_module_local() {
        let bytes = "export const s = $state(0);\n";
        let built = rune_module_provider_content(&rune(ScriptSourceType::Ts), bytes)
            .expect("a .svelte.ts rune module has provider content");
        // Module-local marker FIRST; the user bytes are preserved at the tail.
        assert!(built.content.starts_with("export {};\n"));
        assert!(built.content.ends_with(bytes));
        // TS `declare` form.
        assert!(built.content.contains("declare function $state"));
        // The prelude is whole lines; the offset is the prelude's newline count.
        assert!(built.prelude_line_count > 0);
        let prelude_only = built.content.strip_suffix(bytes).unwrap();
        assert_eq!(
            built.prelude_line_count,
            prelude_only.bytes().filter(|&b| b == b'\n').count() as u32
        );
    }

    #[test]
    fn js_rune_module_content_is_js_valid() {
        let bytes = "export const s = $state(0);\n";
        let built = rune_module_provider_content(&rune(ScriptSourceType::js()), bytes)
            .expect("a .svelte.js rune module has provider content");
        assert!(built.content.starts_with("export {};\n"));
        // JS-valid: NO TS `declare` syntax.
        assert!(!built.content.contains("declare "));
        assert!(built.content.contains("function $state(initial)"));
    }

    #[test]
    fn plain_and_component_have_no_rune_provider_content() {
        assert!(rune_module_provider_content(&FileLanguage::script_ts(), "x").is_none());
        assert!(rune_module_provider_content(&FileLanguage::svelte(), "x").is_none());
        assert!(svelte_rune_module_source_type(&FileLanguage::script_ts()).is_none());
    }

    #[test]
    fn prelude_line_count_is_the_exact_offset_an_own_buffer_position_mapper_must_apply() {
        // The rune module's provider content is `<whole-line prelude> + <bytes>`,
        // served from the module's OWN canonical path (a non-carrier file). Every
        // original source line therefore appears at `original_line +
        // prelude_line_count` in the provider content. A diagnostic/hover the
        // TypeProvider reports at provider line N maps back to source line
        // `N - prelude_line_count` (columns unchanged). This characterizes the
        // EXACT uniform offset an own-buffer position mapper must consume; an
        // implementation that ignores it lands diagnostics off by this many lines.
        let bytes = "const a = 1;\nexport const s = $state(a);\n";
        let built = rune_module_provider_content(&rune(ScriptSourceType::Ts), bytes)
            .expect("a .svelte.ts rune module has provider content");

        // The first user-source line (`const a = 1;`) sits at provider line
        // `prelude_line_count` (0-based) — i.e. shifted by exactly the offset.
        let provider_lines: Vec<&str> = built.content.split_inclusive('\n').collect();
        let first_user_line_idx = provider_lines
            .iter()
            .position(|line| line.starts_with("const a = 1;"))
            .expect("the first user line appears in the provider content");
        assert_eq!(
            first_user_line_idx as u32, built.prelude_line_count,
            "the first user-source line must sit at provider line `prelude_line_count`; \
             an own-buffer position mapper subtracts exactly this offset to recover the \
             source line (off-by-`prelude_line_count` if unwired)"
        );

        // The second user line follows directly (the prelude is a contiguous
        // whole-line block — a single uniform offset, no per-line skew).
        assert!(
            provider_lines[first_user_line_idx + 1].starts_with("export const s ="),
            "user lines stay contiguous after the prelude — a uniform offset"
        );
    }
}
