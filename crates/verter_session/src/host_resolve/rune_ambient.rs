//! Per-file ambient rune typing for standalone Svelte rune modules.
//!
//! A `.svelte.ts` / `.svelte.js` rune module (D-bk) uses Svelte 5 runes at
//! module scope (`export const s = $state(0)`). For Verter's own
//! type-resolution engine to infer the exported rune-derived types correctly
//! (Channel A — the session side), the module-valid rune declarations
//! (`$state`/`$derived`/`$effect`/`$inspect`) must be in scope when the
//! module's eval environment is built.
//!
//! This injects them as AMBIENT symbols into the rune module's
//! [`EvalEnv`](verter_semantic::analysis::type_eval::EvalEnv), exactly the way
//! `<script setup generic>` type parameters are merged in — WITHOUT touching
//! `eval_source` (its bytes stay the real module verbatim, so every OXC span is
//! source-absolute and cross-file resolution / diagnostics / hover are
//! unaffected). The merge is PER-FILE scoped: only a file the language
//! classifier resolves to the Svelte rune-module flavor receives the runes, so
//! a plain `.ts` / `.js` never sees `$state`.
//!
//! The rune declaration text is the SINGLE shared rune source
//! ([`verter_compiler::svelte::ide::prelude::module_rune_ambient_source`]) — no
//! second declaration list. Its version
//! ([`verter_compiler::svelte::ide::prelude::RUNE_AMBIENT_PRELUDE_VERSION`])
//! feeds the rune module's type/eval-env cache key so a prelude fix invalidates
//! stale inferred exports.

use std::sync::OnceLock;

use verter_language::FileLanguage;
use verter_semantic::analysis::type_eval::EvalEnv;

/// Whether `language` is a Svelte standalone rune module (the
/// [`ScriptFlavor::AdapterModule`](verter_language::ScriptFlavor::AdapterModule)
/// owned by the Svelte adapter). A plain script and every framework
/// carrier/template are NOT rune modules.
pub(crate) fn is_svelte_rune_module(language: &FileLanguage) -> bool {
    language
        .adapter_script_language()
        .is_some_and(|(adapter, lang)| {
            adapter.is_svelte() && lang.as_str() == verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID
        })
}

/// The isolated env built ONCE from the shared module-rune declarations. Its
/// value/type symbols are cloned into each rune module's env on demand.
fn rune_ambient_env() -> &'static EvalEnv {
    static ENV: OnceLock<EvalEnv> = OnceLock::new();
    ENV.get_or_init(|| {
        verter_semantic::analysis::type_eval_build::parse_and_build_env(
            verter_compiler::svelte::ide::prelude::module_rune_ambient_source(),
        )
    })
}

/// Merge the module-valid rune ambient declarations into `env` when the file is
/// a Svelte rune module. No-op for every other file (per-file scoping).
///
/// User declarations win: a name the user already declared (a local `$state`
/// shadow, an import) is NOT clobbered — the rune symbol is added only when the
/// name is absent in the target env.
pub(crate) fn apply_svelte_rune_ambient_env(env: &mut EvalEnv, language: &FileLanguage) {
    if !is_svelte_rune_module(language) {
        return;
    }
    let ambient = rune_ambient_env();
    for (name, group) in &ambient.value_symbols {
        if !env.value_symbols.contains_key(name) {
            env.value_symbols.insert(name.clone(), group.clone());
        }
    }
    for (name, group) in &ambient.type_symbols {
        if !env.type_symbols.contains_key(name) {
            env.type_symbols.insert(name.clone(), group.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_language::{FrameworkAdapterId, LanguageId, ScriptSourceType};

    fn rune_module_ts() -> FileLanguage {
        FileLanguage::adapter_module(
            ScriptSourceType::Ts,
            FrameworkAdapterId::svelte(),
            LanguageId::new(verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID),
        )
    }

    #[test]
    fn rune_module_classification_is_recognised() {
        assert!(is_svelte_rune_module(&rune_module_ts()));
        // A plain script is NOT a rune module (per-file scoping).
        assert!(!is_svelte_rune_module(&FileLanguage::script_ts()));
        // The `.svelte` COMPONENT carrier is NOT a rune module.
        assert!(!is_svelte_rune_module(&FileLanguage::svelte()));
        // A non-svelte adapter module (hypothetical) is NOT a svelte rune module.
        assert!(!is_svelte_rune_module(&FileLanguage::adapter_module(
            ScriptSourceType::Ts,
            FrameworkAdapterId::new("other"),
            LanguageId::new(verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID),
        )));
    }

    #[test]
    fn rune_module_env_gains_the_module_runes_but_a_plain_env_does_not() {
        // A rune module's env gains $state/$derived/$effect/$inspect.
        let mut rune_env = EvalEnv::default();
        apply_svelte_rune_ambient_env(&mut rune_env, &rune_module_ts());
        for rune in ["$state", "$derived", "$effect", "$inspect"] {
            assert!(
                rune_env.value_symbols.contains_key(rune),
                "the rune module env must carry {rune}"
            );
        }

        // DISCRIMINATING per-file scoping: a PLAIN script env is untouched —
        // no rune leaks (req 4). A global injection would fail this.
        let mut plain_env = EvalEnv::default();
        apply_svelte_rune_ambient_env(&mut plain_env, &FileLanguage::script_ts());
        assert!(
            plain_env.value_symbols.is_empty(),
            "a plain script must NOT gain any rune symbols, got {:?}",
            plain_env.value_symbols.keys().collect::<Vec<_>>()
        );
        assert!(!plain_env.value_symbols.contains_key("$state"));
    }

    #[test]
    fn rune_ambient_parser_flag_tracks_the_prelude_version() {
        // The parse-env flag's version suffix MUST track the rune-prelude
        // version so a prelude-surface change invalidates a rune module's stale
        // inferred exports through `parse_env_hash`. The version constant lives
        // in `verter_compiler`; the flag lives in `verter_workspace`; this guard
        // (in the crate that sees both) pins them in lockstep.
        let expected = format!(
            "svelte-rune-ambient-v{}",
            verter_compiler::svelte::ide::prelude::RUNE_AMBIENT_PRELUDE_VERSION
        );
        assert_eq!(
            verter_workspace::SVELTE_RUNE_AMBIENT_PARSER_FLAG,
            expected,
            "the rune-ambient parse-env flag must encode the current RUNE_AMBIENT_PRELUDE_VERSION; \
             bump the flag suffix when you bump the version"
        );
    }

    #[test]
    fn user_declarations_are_not_clobbered() {
        // If the module already declares a `$state` (a local shadow / import),
        // the rune ambient must NOT overwrite it.
        let mut env = EvalEnv::default();
        let user = verter_semantic::analysis::type_eval::ValueDeclInfo {
            name: "$state".to_string(),
            declaration_id: 0,
            kind: verter_semantic::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: None,
            signatures: Vec::new(),
            object_shape: None,
        };
        env.add_value(user);
        let before = env
            .value_symbols
            .get("$state")
            .map(|g| g.contributors.len());
        apply_svelte_rune_ambient_env(&mut env, &rune_module_ts());
        let after = env
            .value_symbols
            .get("$state")
            .map(|g| g.contributors.len());
        assert_eq!(
            before, after,
            "a user $state declaration must not be clobbered by the rune ambient"
        );
    }
}
