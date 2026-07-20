//! Per-file ambient rune typing for standalone Svelte rune modules.
//!
//! A `.svelte.ts` / `.svelte.js` rune module uses Svelte 5 runes at
//! module scope (`export const s = $state(0)`). For Verter's own
//! type-resolution engine to infer the exported rune-derived types correctly
//! (Channel A — the session side), the module-valid rune declarations
//! (`$state`/`$derived`/`$effect`/`$inspect`) must be in scope when the
//! module's declarations are resolved.
//!
//! The runes enter through the CENTRALIZED effective-lookup
//! ([`crate::resolver_core::ShallowFileState::effective_value_decl`] and its
//! siblings): a name the user did NOT declare, in a file classified as a
//! Svelte rune module, resolves to the rune ambient inventory — WITHOUT
//! touching the real module source (its bytes stay the verbatim module, so
//! every OXC span is source-absolute and cross-file resolution / diagnostics /
//! hover are unaffected). The merge is PER-FILE scoped: only a file the
//! language classifier resolves to the Svelte rune-module flavor sees the
//! runes, so a plain `.ts` / `.js` never sees `$state`.
//!
//! The rune declaration text is the SINGLE shared rune source
//! ([`verter_compiler::svelte::ide::prelude::module_rune_ambient_source`]) — no
//! second declaration list. Its version
//! ([`verter_compiler::svelte::ide::prelude::RUNE_AMBIENT_PRELUDE_VERSION`])
//! feeds the rune module's `parse_env_hash` (via the workspace parser flag) so
//! a prelude fix invalidates a rune module's stale inferred exports through the
//! whole content-addressed cache lineage.

use std::sync::{Arc, OnceLock};

use rustc_hash::FxHashMap;
use verter_language::FileLanguage;
use verter_semantic::analysis::type_eval::EvalEnv;

use crate::decl_body_memo::{LoweredTypeDecl, LoweredValueDecl};

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

/// The graph-native rune ambient inventory: the module-valid rune
/// declarations lowered ONCE from the shared prelude source into per-name
/// declaration RECORDS (`LoweredValueDecl` / `LoweredTypeDecl`) — the SAME
/// per-symbol shape the lazy declaration-body memo hands the graph-native
/// readers, plus the folded `EvalEnv` form the whole-env oracle consumes.
///
/// This is the SINGLE authority for rune visibility: both the centralized
/// effective-lookup (graph-native readers) and the whole-env oracle obtain
/// the runes from THIS inventory, so the two can never diverge for the rune
/// symbols.
#[derive(Debug)]
pub(crate) struct RuneAmbientInventory {
    /// Per-name lowered VALUE declarations (`$state`/`$derived`/`$effect`/
    /// `$inspect`, the rune functions + namespace values).
    value_decls: FxHashMap<String, Arc<LoweredValueDecl>>,
    /// Per-name lowered TYPE declarations (the rune namespace types).
    type_decls: FxHashMap<String, Arc<LoweredTypeDecl>>,
    /// The folded eval-env form — the value/type symbol groups the
    /// whole-env oracle merges in. Built from the same single prelude
    /// lowering, so it is byte-for-byte the symbols the per-name records
    /// above carry.
    env: EvalEnv,
}

impl RuneAmbientInventory {
    /// The lowered VALUE declaration for `name`, if the rune ambient
    /// declares it.
    fn value_decl(&self, name: &str) -> Option<Arc<LoweredValueDecl>> {
        self.value_decls.get(name).cloned()
    }

    /// The lowered TYPE declaration for `name`, if the rune ambient
    /// declares it.
    fn type_decl(&self, name: &str) -> Option<Arc<LoweredTypeDecl>> {
        self.type_decls.get(name).cloned()
    }

    /// Whether the rune ambient declares a VALUE symbol named `name`.
    fn has_value(&self, name: &str) -> bool {
        self.value_decls.contains_key(name)
    }

    /// Whether the rune ambient declares a TYPE symbol named `name`.
    fn has_type(&self, name: &str) -> bool {
        self.type_decls.contains_key(name)
    }
}

/// The process-wide rune ambient inventory, lowered ONCE from the shared
/// module-rune declarations.
///
/// A fixed declaration string (NOT a workspace file) lowered ONCE into a
/// process-wide `OnceLock`, using the production env-build primitive
/// `build_eval_env` over a one-shot OXC parse of the prelude source, then
/// CONVERTED into per-name declaration records. It is NOT the per-file
/// second parse the `no_production_parse_and_build_env_in_session` guard
/// targets — no file's materialise flight is re-parsed; the prelude has no
/// canonical id.
fn rune_ambient_inventory() -> &'static RuneAmbientInventory {
    static INVENTORY: OnceLock<RuneAmbientInventory> = OnceLock::new();
    INVENTORY.get_or_init(|| {
        let source = verter_compiler::svelte::ide::prelude::module_rune_ambient_source();
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
        // The prelude has NO canonical id — the empty anchor canonical makes
        // every minted body locator fail the deref's canonical-coherence gate
        // closed (ambient bodies are served from THIS inventory, never a
        // memo re-borrow).
        let build_ctx = verter_semantic::analysis::type_eval_build::BuildEvalEnvContext::new("");
        let env = verter_semantic::analysis::type_eval_build::build_eval_env(
            &parsed.program,
            source,
            &build_ctx,
        );

        // Convert the folded value/type groups into the per-name lowered
        // records the graph-native readers consume, through the SAME shared
        // per-symbol fold the lazy declaration-body memo uses
        // ([`crate::decl_body_memo::lowered_decls_from_env_and_program`] —
        // transients re-lowered from the one prelude parse) so the ambient
        // records can never diverge from the memo-served shape. Ambient
        // records never enter the parse fact rail, so the lens-free
        // fingerprints are inert there.
        let (types, values) = crate::decl_body_memo::lowered_decls_from_env_and_program(
            &env,
            &parsed.program,
            source,
        );
        let value_decls: FxHashMap<String, Arc<LoweredValueDecl>> = values
            .into_iter()
            .map(|(key, lowered)| (key.name.to_string(), Arc::new(lowered)))
            .collect();
        let type_decls: FxHashMap<String, Arc<LoweredTypeDecl>> = types
            .into_iter()
            .map(|(key, lowered)| (key.name.to_string(), Arc::new(lowered)))
            .collect();

        RuneAmbientInventory {
            value_decls,
            type_decls,
            env,
        }
    })
}

/// The lowered VALUE declaration for `name` from the rune ambient inventory.
/// The centralized effective-lookup consults this AFTER a user/synthesized
/// declaration miss, gated on the file's rune-module classification.
pub(crate) fn rune_ambient_value_decl(name: &str) -> Option<Arc<LoweredValueDecl>> {
    rune_ambient_inventory().value_decl(name)
}

/// The lowered TYPE declaration for `name` from the rune ambient inventory.
pub(crate) fn rune_ambient_type_decl(name: &str) -> Option<Arc<LoweredTypeDecl>> {
    rune_ambient_inventory().type_decl(name)
}

/// Whether the rune ambient inventory declares a VALUE symbol named `name`
/// (header-presence probe — no body materialisation).
pub(crate) fn rune_ambient_has_value(name: &str) -> bool {
    rune_ambient_inventory().has_value(name)
}

/// Whether the rune ambient inventory declares a TYPE symbol named `name`.
pub(crate) fn rune_ambient_has_type(name: &str) -> bool {
    rune_ambient_inventory().has_type(name)
}

/// Merge the module-valid rune ambient declarations into `env` when the file
/// is a Svelte rune module — sourced from the SAME centralized inventory the
/// graph-native effective-lookup consults. No-op for every other file
/// (per-file scoping).
///
/// User declarations win: a name the user already declared (a local `$state`
/// shadow, an import) is NOT clobbered — the rune symbol is added only when
/// the name is absent in the target env.
///
/// This is the whole-env ORACLE's rune-visibility entry: it folds the runes
/// in from the centralized inventory so the oracle and the graph-native
/// readers agree on rune visibility (the `whole_env()` → graph-native
/// equivalence cross-check).
pub(crate) fn merge_rune_ambient_into_env(env: &mut EvalEnv, language: &FileLanguage) {
    if !is_svelte_rune_module(language) {
        return;
    }
    merge_rune_ambient_inventory_into_env(env);
}

/// Merge the module-valid rune declarations into an environment after the
/// caller has independently proved rune visibility (for example, a `.svelte`
/// component classified as runes mode from its retained AST). This ungated
/// primitive remains crate-private; the centralized file-state lookup owns the
/// visibility decision, while this module remains the single declaration
/// inventory.
pub(crate) fn merge_rune_ambient_inventory_into_env(env: &mut EvalEnv) {
    let ambient = rune_ambient_inventory();
    for (name, group) in &ambient.env.value_symbols {
        if !env.value_symbols.contains_key(name) {
            env.value_symbols.insert(name.clone(), group.clone());
        }
    }
    for (name, group) in &ambient.env.type_symbols {
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
    fn rune_ambient_inventory_carries_the_module_runes_as_value_decls() {
        // The inventory exposes $state/$derived/$effect/$inspect as per-name
        // lowered VALUE declarations — the graph-native effective-lookup shape.
        for rune in ["$state", "$derived", "$effect", "$inspect"] {
            assert!(
                rune_ambient_value_decl(rune).is_some(),
                "the rune ambient inventory must carry {rune} as a lowered value decl"
            );
            assert!(
                rune_ambient_has_value(rune),
                "the rune ambient inventory must report {rune} present (header probe)"
            );
        }
        // A non-rune name is absent (the inventory is exactly the rune surface).
        assert!(rune_ambient_value_decl("notARune").is_none());
        assert!(!rune_ambient_has_value("notARune"));
    }

    #[test]
    fn rune_module_env_gains_the_module_runes_but_a_plain_env_does_not() {
        // A rune module's oracle env gains $state/$derived/$effect/$inspect
        // from the centralized inventory.
        let mut rune_env = EvalEnv::default();
        merge_rune_ambient_into_env(&mut rune_env, &rune_module_ts());
        for rune in ["$state", "$derived", "$effect", "$inspect"] {
            assert!(
                rune_env.value_symbols.contains_key(rune),
                "the rune module env must carry {rune}"
            );
        }

        // DISCRIMINATING per-file scoping: a PLAIN script env is untouched —
        // no rune leaks (req 4). A global injection would fail this.
        let mut plain_env = EvalEnv::default();
        merge_rune_ambient_into_env(&mut plain_env, &FileLanguage::script_ts());
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
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 0,
            kind: verter_semantic::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: verter_type_expr::facts::ValueTypeAnnotationFact {
                typeof_alias_target: None,
                classification: verter_type_expr::facts::ValueAnnotationClass::Absent,
                annotation: None,
            },
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        };
        env.add_value(user);
        let before = env
            .value_symbols
            .get("$state")
            .map(|g| g.contributors.len());
        merge_rune_ambient_into_env(&mut env, &rune_module_ts());
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
