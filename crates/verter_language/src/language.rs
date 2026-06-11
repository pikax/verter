//! The open file-language descriptor.

use std::sync::Arc;

use crate::ids::{FrameworkAdapterId, LanguageId};

/// Module grammar of a JavaScript dialect (closed set, mirroring OXC's
/// own `ModuleKind` extension model).
///
/// `import` / `export` are MODULE-ONLY syntax: collapsing every
/// JavaScript file onto one module kind either rejects module `.js` /
/// `.mjs` dependencies (classic-script grammar) or mislabels CommonJS
/// `.cjs` files (module grammar), so the neutral descriptor carries the
/// kind explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum JsModuleKind {
    /// Module-or-script, decided by the presence of ESM syntax after
    /// parse (`.js`, `.jsx`).
    Unambiguous,
    /// Always an ES module (`.mjs`).
    Module,
    /// CommonJS (`.cjs`).
    CommonJs,
    /// Always a classic script — no `import`/`export`. No registry row
    /// produces this; it exists for carrier regions whose producer
    /// resolves a classic-script dialect (the Vue `<script lang="js">`
    /// mapping).
    Script,
}

/// Source dialect of a plain script file (closed set).
///
/// This is descriptor data on [`FileLanguage::Script`]: it records what
/// the extension says the file is. Parse-time source-type computation
/// for embedded carrier scripts stays with the carrier's own parse data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ScriptSourceType {
    /// TypeScript (`.ts`, `.mts`, `.cts`).
    Ts,
    /// TypeScript with JSX (`.tsx`).
    Tsx,
    /// JavaScript (`.js`, `.mjs`, `.cjs`), with its module kind.
    Js(JsModuleKind),
    /// JavaScript with JSX (`.jsx`), with its module kind.
    Jsx(JsModuleKind),
    /// TypeScript declaration file (`.d.ts`, `.d.mts`, `.d.cts`).
    Dts,
}

impl ScriptSourceType {
    /// The `.js` dialect: JavaScript, module-or-script decided by ESM
    /// syntax (OXC's extension model for `js`).
    pub const fn js() -> Self {
        Self::Js(JsModuleKind::Unambiguous)
    }

    /// The `.mjs` dialect: JavaScript, always an ES module.
    pub const fn mjs() -> Self {
        Self::Js(JsModuleKind::Module)
    }

    /// The `.cjs` dialect: JavaScript, CommonJS.
    pub const fn cjs() -> Self {
        Self::Js(JsModuleKind::CommonJs)
    }

    /// The `.jsx` dialect: JavaScript with JSX, module-or-script
    /// decided by ESM syntax (OXC's extension model for `jsx`).
    pub const fn jsx() -> Self {
        Self::Jsx(JsModuleKind::Unambiguous)
    }
}

/// The single open language descriptor every Verter crate routes files
/// through.
///
/// * [`FileLanguage::Script`] — a plain script tracked for dependency,
///   export, and type resolution.
/// * [`FileLanguage::Framework`] — a framework CARRIER file (a file that
///   embeds script/template/style regions, e.g. a Vue SFC). The adapter
///   id is open; whether a registered carrier implementation exists
///   behind it is a dispatch-time question, not a classification-time
///   one — a carrier row without an implementation dispatches to the
///   typed unsupported-language error.
/// * [`FileLanguage::FrameworkTemplate`] — an external template owned by
///   a framework component (e.g. an Angular `templateUrl` target). Never
///   produced by a built-in row; project-gated rows resolve it at the
///   host level.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FileLanguage {
    /// A plain script file.
    Script {
        /// Extension-derived source dialect.
        source_type: ScriptSourceType,
    },
    /// A framework carrier file.
    Framework {
        /// Owning adapter (open set).
        adapter_id: FrameworkAdapterId,
        /// Concrete language within the adapter (open set).
        language_id: LanguageId,
    },
    /// An external framework template routed to its owning component.
    FrameworkTemplate {
        /// Owning adapter (open set).
        adapter_id: FrameworkAdapterId,
        /// Canonical id of the owning component file, when known at
        /// classification time. Static classification leaves this
        /// `None`; owner binding is host-level work.
        owner_hint: Option<Arc<str>>,
    },
}

impl FileLanguage {
    /// A plain script of the given dialect.
    pub fn script(source_type: ScriptSourceType) -> Self {
        Self::Script { source_type }
    }

    /// The catch-all plain-script routing used when no registry row
    /// matches (unknown extensions route as TypeScript scripts).
    pub fn script_ts() -> Self {
        Self::script(ScriptSourceType::Ts)
    }

    /// The built-in Vue SFC carrier language.
    ///
    /// Memoized: this constructor runs on hot per-file/per-request
    /// paths, so the row is built once and cloned (two refcount
    /// bumps) — never re-locking the intern table.
    pub fn vue() -> Self {
        static VUE: std::sync::OnceLock<FileLanguage> = std::sync::OnceLock::new();
        VUE.get_or_init(|| Self::Framework {
            adapter_id: FrameworkAdapterId::vue(),
            language_id: LanguageId::new("vue"),
        })
        .clone()
    }

    /// The built-in Svelte carrier language. Memoized like
    /// [`Self::vue`].
    pub fn svelte() -> Self {
        static SVELTE: std::sync::OnceLock<FileLanguage> = std::sync::OnceLock::new();
        SVELTE
            .get_or_init(|| Self::Framework {
                adapter_id: FrameworkAdapterId::svelte(),
                language_id: LanguageId::new("svelte"),
            })
            .clone()
    }

    /// `true` for any framework CARRIER file ([`FileLanguage::Framework`]).
    pub fn is_framework_carrier(&self) -> bool {
        matches!(self, Self::Framework { .. })
    }

    /// `true` when this language is the built-in Vue SFC carrier.
    ///
    /// Checks the FULL row — adapter id AND language id. `language_id`
    /// exists so one adapter can own several languages; only the SFC
    /// carrier row may dispatch into the Vue SFC parse path.
    pub fn is_vue(&self) -> bool {
        matches!(
            self,
            Self::Framework {
                adapter_id,
                language_id,
            } if adapter_id.is_vue() && language_id.as_str() == "vue"
        )
    }

    /// The owning adapter id, for framework carriers and templates.
    pub fn adapter_id(&self) -> Option<&FrameworkAdapterId> {
        match self {
            Self::Script { .. } => None,
            Self::Framework { adapter_id, .. } | Self::FrameworkTemplate { adapter_id, .. } => {
                Some(adapter_id)
            }
        }
    }

    /// The script dialect, for plain scripts.
    pub fn script_source_type(&self) -> Option<ScriptSourceType> {
        match self {
            Self::Script { source_type } => Some(*source_type),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_discriminate_variants() {
        assert!(FileLanguage::vue().is_framework_carrier());
        assert!(FileLanguage::vue().is_vue());
        assert!(FileLanguage::svelte().is_framework_carrier());
        assert!(!FileLanguage::svelte().is_vue());
        assert!(!FileLanguage::script_ts().is_framework_carrier());
        assert!(!FileLanguage::script_ts().is_vue());

        assert_eq!(
            FileLanguage::svelte().adapter_id(),
            Some(&FrameworkAdapterId::svelte())
        );
        assert_eq!(FileLanguage::script_ts().adapter_id(), None);
        assert_eq!(
            FileLanguage::script(ScriptSourceType::Dts).script_source_type(),
            Some(ScriptSourceType::Dts)
        );
        assert_eq!(FileLanguage::vue().script_source_type(), None);
    }

    /// `is_vue()` accepts ONLY the built-in Vue SFC carrier row — the
    /// adapter id alone is not enough. `language_id` exists precisely
    /// so one adapter can own several languages; a future Vue-adapter
    /// non-carrier language must not dispatch into the Vue SFC parse
    /// path.
    #[test]
    fn is_vue_requires_the_carrier_language_id_not_just_the_adapter() {
        let vue_adapter_other_language = FileLanguage::Framework {
            adapter_id: FrameworkAdapterId::vue(),
            language_id: LanguageId::new("vue_template"),
        };
        assert!(vue_adapter_other_language.is_framework_carrier());
        assert!(
            !vue_adapter_other_language.is_vue(),
            "a Vue-adapter language that is not the SFC carrier must not be is_vue"
        );
        assert!(FileLanguage::vue().is_vue());
    }

    #[test]
    fn descriptor_equality_is_structural() {
        assert_eq!(FileLanguage::vue(), FileLanguage::vue());
        assert_ne!(FileLanguage::vue(), FileLanguage::svelte());
        assert_ne!(
            FileLanguage::script(ScriptSourceType::Ts),
            FileLanguage::script(ScriptSourceType::Dts)
        );
    }
}
