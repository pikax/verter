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
        verter_semantic::analysis::VueApiClassification::Provide
            | verter_semantic::analysis::VueApiClassification::Inject
    ) {
        "Dependency Injection"
    } else if matches!(
        api,
        verter_semantic::analysis::VueApiClassification::Ref
            | verter_semantic::analysis::VueApiClassification::ShallowRef
            | verter_semantic::analysis::VueApiClassification::Reactive
            | verter_semantic::analysis::VueApiClassification::ShallowReactive
            | verter_semantic::analysis::VueApiClassification::Computed
            | verter_semantic::analysis::VueApiClassification::ToRef
            | verter_semantic::analysis::VueApiClassification::ToRefs
            | verter_semantic::analysis::VueApiClassification::Readonly
            | verter_semantic::analysis::VueApiClassification::ShallowReadonly
            | verter_semantic::analysis::VueApiClassification::CustomRef
            | verter_semantic::analysis::VueApiClassification::TriggerRef
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
            verter_semantic::analysis::VueApiClassification::UseTemplateRef
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
fn reactivity_kind_label(binding: &verter_semantic::analysis::AnalyzedBinding) -> Option<String> {
    match binding.reactivity_kind {
        verter_semantic::analysis::ReactivityKind::Ref => Some("ref".to_string()),
        verter_semantic::analysis::ReactivityKind::Computed => Some("computed".to_string()),
        verter_semantic::analysis::ReactivityKind::Reactive => Some("reactive".to_string()),
        verter_semantic::analysis::ReactivityKind::MaybeRef => Some("maybe ref".to_string()),
        verter_semantic::analysis::ReactivityKind::Mutable => Some("mutable".to_string()),
        verter_semantic::analysis::ReactivityKind::None => {
            if binding.is_reactive {
                Some("reactive".to_string())
            } else {
                None
            }
        }
    }
}

fn format_binding_hover(binding: &verter_semantic::analysis::AnalyzedBinding) -> Hover {
    let mut lines = Vec::new();

    let kind_str = match binding.kind {
        verter_semantic::analysis::AnalyzedBindingKind::Const => "const",
        verter_semantic::analysis::AnalyzedBindingKind::Let => "let",
        verter_semantic::analysis::AnalyzedBindingKind::Var => "var",
        verter_semantic::analysis::AnalyzedBindingKind::Function => "function",
        verter_semantic::analysis::AnalyzedBindingKind::AsyncFunction => "async function",
        verter_semantic::analysis::AnalyzedBindingKind::Class => "class",
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
        verter_semantic::analysis::ReactivityKind::None => {
            if binding.is_reactive {
                lines.push("*(reactive)*".to_string());
            }
        }
        verter_semantic::analysis::ReactivityKind::Ref => {
            lines.push("*(ref — needs `.value`)*".to_string())
        }
        verter_semantic::analysis::ReactivityKind::Computed => {
            lines.push("*(computed — needs `.value`, read-only)*".to_string());
        }
        verter_semantic::analysis::ReactivityKind::Reactive => {
            lines.push("*(reactive — direct property access)*".to_string());
        }
        verter_semantic::analysis::ReactivityKind::MaybeRef => {
            lines.push("*(maybe ref — may need `.value`)*".to_string());
        }
        verter_semantic::analysis::ReactivityKind::Mutable => {
            lines.push("*(mutable — reassignable)*".to_string());
        }
    }

    if let Some(ref init) = binding.initializer {
        match init {
            verter_semantic::analysis::BindingInitializer::FunctionCall {
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
            verter_semantic::analysis::BindingInitializer::Literal { kind } => {
                lines.push(format!("Literal: {kind:?}"));
            }
            verter_semantic::analysis::BindingInitializer::Reference { name } => {
                lines.push(format!("References `{name}`"));
            }
            verter_semantic::analysis::BindingInitializer::Other => {}
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
    binding: &verter_semantic::analysis::AnalyzedImportBinding,
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

fn format_macro_hover(mac: &verter_semantic::analysis::AnalyzedMacro) -> Hover {
    let macro_name = match mac.kind {
        verter_semantic::analysis::AnalyzedMacroKind::DefineProps => "defineProps",
        verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => "defineEmits",
        verter_semantic::analysis::AnalyzedMacroKind::DefineModel => "defineModel",
        verter_semantic::analysis::AnalyzedMacroKind::DefineExpose => "defineExpose",
        verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => "defineOptions",
        verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => "defineSlots",
        verter_semantic::analysis::AnalyzedMacroKind::WithDefaults => "withDefaults",
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
