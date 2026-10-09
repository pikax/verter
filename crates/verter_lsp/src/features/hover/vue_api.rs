use super::*;

pub(super) fn vue_api_hover_at_offset(
    offset: u32,
    analysis: &FileAnalysisSnapshot,
    ssr_context: bool,
) -> Option<Hover> {
    let call = analysis
        .vue_api_calls
        .iter()
        .find(|c| offset >= c.span.start && offset < c.span.end)?;

    let api = &call.api;
    let name = api.display_name();

    let mut lines = Vec::new();

    lines.push(format!("```typescript\n{name}()\n```"));

    // Category label
    let category = if api.is_lifecycle() {
        "Lifecycle Hook"
    } else if api.is_watcher() {
        "Watcher"
    } else if matches!(
        api,
        verter_session_query::analysis::types::VueApiClassification::Provide
            | verter_session_query::analysis::types::VueApiClassification::Inject
    ) {
        "Dependency Injection"
    } else if matches!(
        api,
        verter_session_query::analysis::types::VueApiClassification::Ref
            | verter_session_query::analysis::types::VueApiClassification::ShallowRef
            | verter_session_query::analysis::types::VueApiClassification::Reactive
            | verter_session_query::analysis::types::VueApiClassification::ShallowReactive
            | verter_session_query::analysis::types::VueApiClassification::Computed
            | verter_session_query::analysis::types::VueApiClassification::ToRef
            | verter_session_query::analysis::types::VueApiClassification::ToRefs
            | verter_session_query::analysis::types::VueApiClassification::Readonly
            | verter_session_query::analysis::types::VueApiClassification::ShallowReadonly
            | verter_session_query::analysis::types::VueApiClassification::CustomRef
            | verter_session_query::analysis::types::VueApiClassification::TriggerRef
    ) {
        "Reactivity Primitive"
    } else {
        "Vue API"
    };

    lines.push(format!("*{category}*"));

    if api.requires_sync_context() {
        lines.push("Must be called during synchronous `setup()` execution.".to_string());
    }

    // SSR warning for client-only hooks
    if ssr_context && CLIENT_ONLY_HOOKS.contains(api) {
        lines.push(
            "**⚠ SSR Warning:** This hook does not fire during server-side rendering. \
             Move DOM-dependent logic here, or use `onServerPrefetch()` for data fetching."
                .to_string(),
        );
    }

    // SSR note for useTemplateRef
    if ssr_context
        && matches!(
            api,
            verter_session_query::analysis::types::VueApiClassification::UseTemplateRef
        )
    {
        lines.push(
            "**⚠ SSR Warning:** Template refs are `null` during SSR. \
             Access `.value` inside `onMounted()` or guard with `import.meta.client`."
                .to_string(),
        );
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    })
}

pub(in crate::features) fn hover_for_word(
    word: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<VerterHoverResult> {
    // Check bindings
    if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
        let vue_kind_label = reactivity_kind_label(binding);
        return Some(VerterHoverResult {
            hover: format_binding_hover(binding),
            vue_kind_label,
            source_token: None,
        });
    }

    // Check imports
    for import in &analysis.imports {
        if let Some(binding) = import.bindings.iter().find(|b| b.name == word) {
            return Some(format_import_hover(binding, &import.source).into());
        }
    }

    // Check macros
    for mac in analysis.macros.iter() {
        if mac.binding_name.as_ref().is_some_and(|name| name == word) {
            return Some(format_macro_hover(mac).into());
        }
    }

    None
}

/// Map a binding's reactivity kind to a label for the hover kind prefix.
fn reactivity_kind_label(
    binding: &verter_session_query::analysis::types::AnalyzedBinding,
) -> Option<String> {
    match binding.reactivity_kind {
        verter_session_query::analysis::types::ReactivityKind::Ref => Some("ref".to_string()),
        verter_session_query::analysis::types::ReactivityKind::Computed => {
            Some("computed".to_string())
        }
        verter_session_query::analysis::types::ReactivityKind::Reactive => {
            Some("reactive".to_string())
        }
        verter_session_query::analysis::types::ReactivityKind::MaybeRef => {
            Some("maybe ref".to_string())
        }
        verter_session_query::analysis::types::ReactivityKind::Mutable => {
            Some("mutable".to_string())
        }
        verter_session_query::analysis::types::ReactivityKind::None => {
            if binding.is_reactive {
                Some("reactive".to_string())
            } else {
                None
            }
        }
    }
}

fn format_binding_hover(binding: &verter_session_query::analysis::types::AnalyzedBinding) -> Hover {
    let mut lines = Vec::new();

    let kind_str = match binding.kind {
        verter_session_query::analysis::types::AnalyzedBindingKind::Const => "const",
        verter_session_query::analysis::types::AnalyzedBindingKind::Let => "let",
        verter_session_query::analysis::types::AnalyzedBindingKind::Var => "var",
        verter_session_query::analysis::types::AnalyzedBindingKind::Function => "function",
        verter_session_query::analysis::types::AnalyzedBindingKind::AsyncFunction => {
            "async function"
        }
        verter_session_query::analysis::types::AnalyzedBindingKind::Class => "class",
    };

    // Show type annotation if available
    let type_str = binding
        .type_annotation
        .as_deref()
        .map(|t| format!(": {t}"))
        .unwrap_or_default();

    lines.push(format!(
        "```typescript\n{kind_str} {}{type_str}\n```",
        binding.name
    ));

    // Show granular reactivity kind
    match binding.reactivity_kind {
        verter_session_query::analysis::types::ReactivityKind::None => {
            if binding.is_reactive {
                lines.push("*(reactive)*".to_string());
            }
        }
        verter_session_query::analysis::types::ReactivityKind::Ref => {
            lines.push("*(ref — needs `.value`)*".to_string())
        }
        verter_session_query::analysis::types::ReactivityKind::Computed => {
            lines.push("*(computed — needs `.value`, read-only)*".to_string());
        }
        verter_session_query::analysis::types::ReactivityKind::Reactive => {
            lines.push("*(reactive — direct property access)*".to_string());
        }
        verter_session_query::analysis::types::ReactivityKind::MaybeRef => {
            lines.push("*(maybe ref — may need `.value`)*".to_string());
        }
        verter_session_query::analysis::types::ReactivityKind::Mutable => {
            lines.push("*(mutable — reassignable)*".to_string());
        }
    }

    if let Some(ref init) = binding.initializer {
        match init {
            verter_session_query::analysis::types::BindingInitializer::FunctionCall {
                callee,
                callee_import_source,
                ..
            } => {
                let source_info = callee_import_source
                    .as_ref()
                    .map(|s| format!(" (from `{s}`)"))
                    .unwrap_or_default();
                lines.push(format!("Initialized via `{callee}()`{source_info}"));
            }
            verter_session_query::analysis::types::BindingInitializer::Literal { kind } => {
                lines.push(format!("Literal: {kind:?}"));
            }
            verter_session_query::analysis::types::BindingInitializer::Reference { name } => {
                lines.push(format!("References `{name}`"));
            }
            verter_session_query::analysis::types::BindingInitializer::Other => {}
        }
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    }
}

fn format_import_hover(
    binding: &verter_session_query::analysis::types::AnalyzedImportBinding,
    source: &str,
) -> Hover {
    let type_prefix = if binding.is_type_only { "type " } else { "" };
    let mut lines = vec![format!(
        "```typescript\nimport {type_prefix}{{ {} }} from '{}'\n```",
        binding.name, source
    )];

    if let Some(ref api) = binding.vue_api {
        lines.push(format!("Vue API: `{api:?}`"));
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    }
}

fn format_macro_hover(mac: &verter_session_query::analysis::types::AnalyzedMacro) -> Hover {
    let macro_name = match mac.kind {
        verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps => "defineProps",
        verter_session_query::analysis::types::AnalyzedMacroKind::DefineEmits => "defineEmits",
        verter_session_query::analysis::types::AnalyzedMacroKind::DefineModel => "defineModel",
        verter_session_query::analysis::types::AnalyzedMacroKind::DefineExpose => "defineExpose",
        verter_session_query::analysis::types::AnalyzedMacroKind::DefineOptions => "defineOptions",
        verter_session_query::analysis::types::AnalyzedMacroKind::DefineSlots => "defineSlots",
        verter_session_query::analysis::types::AnalyzedMacroKind::WithDefaults => "withDefaults",
    };

    let mut lines = Vec::new();

    if let Some(ref binding) = mac.binding_name {
        lines.push(format!(
            "```typescript\nconst {binding} = {macro_name}()\n```"
        ));
    } else {
        lines.push(format!("```typescript\n{macro_name}()\n```"));
    }

    if mac.is_type_based {
        let types = if mac.type_references.is_empty() {
            "inline type".to_string()
        } else {
            mac.type_references.join(", ")
        };
        lines.push(format!("Type-based: `<{types}>`"));
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    }
}
