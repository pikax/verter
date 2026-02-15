use crate::{
    code_transform,
    cursor::ScriptLanguage,
    syntax::{
        plugins::code_gen::{
            script::{
                macros::process_macro,
                sections::{emit_emits_section, emit_props_section},
            },
            types::ScriptSetupImportDependencies,
        },
        types::OxcScript,
    },
    utils::oxc::vue::{ScriptItem, ScriptMacro, TypeDeclarationKind, VueMacroKind},
};

pub struct ProcessScriptOptions<'alloc> {
    pub is_production: bool,
    pub source: &'alloc str,
    pub component_name: &'alloc str,

    pub inline_template: bool,
    pub keep_ts_types: bool,
    pub is_vapor: bool,
}

pub struct ProcessedScript {
    pub imports: ScriptSetupImportDependencies,
    pub diagnostics: Vec<super::macros::types::MacroDiagnostic>,
    /// Deferred closing text for inline template mode.
    /// When set, this must be emitted AFTER the template content to close
    /// setup() and the component definition.
    pub deferred_closing: Option<String>,
}

pub fn process_script_event<'alloc>(
    script: &OxcScript<'alloc>,
    code_transform: &mut code_transform::CodeTransform<'alloc>,
    opts: ProcessScriptOptions<'alloc>,
) -> ProcessedScript {
    let mut imports = ScriptSetupImportDependencies::default();
    let mut diagnostics = Vec::new();
    // not in setup
    if script.setup.is_none() {
        // Regular <script> (no setup): strip the <script> and </script> tags
        // but keep the content between them.
        code_transform.remove(script.tag_open_start, script.tag_open_end);
        code_transform.remove(script.tag_close_start, script.tag_close_end);

        // TODO
        return ProcessedScript {
            imports,
            diagnostics,
            deferred_closing: None,
        };
    }

    // setup

    let is_typescript = matches!(script.lang, Some(lang) if lang == ScriptLanguage::TypeScript || lang == ScriptLanguage::TSX);

    let mut returned: Vec<&'alloc str> = Vec::with_capacity(script.result.bindings.len());
    let mut prop = None;
    let mut options = None;
    let mut expose = None;
    let mut emit = None;
    let mut models = Vec::new();

    let mut has_emit_declarator = false;

    // Opening tag: emit "const __sfc__ = /*@__PURE__*/".
    // The actual `export default __sfc__` is appended by ScriptGeneratorPlugin::end(),
    // which also handles __scopeId for scoped styles. Using a variable here instead of
    // `export default` avoids string-based detection issues (e.g., "export default" in
    // comments/strings being mistakenly matched by downstream processors).
    code_transform.overwrite(
        script.tag_open_start,
        script.tag_open_end,
        "const __sfc__ = /*@__PURE__*/",
    );

    // Process each script item
    for item in script.result.items.iter() {
        match item {
            ScriptItem::Import(event) => {
                if event.is_type_only {
                    // Strip type-only imports (`import type { ... }`) — invalid in JS output
                    code_transform.remove(event.span.start, event.span.end);
                } else {
                    code_transform.move_with_suffix(
                        event.span.start,
                        event.span.end,
                        script.tag_open_start,
                        "\n",
                    );
                    // Include imported bindings in __returned__ so they're accessible
                    // via $setup in the render function (components, helpers, constants).
                    // Skip per-specifier type imports (`import { type Foo }`) — they have no runtime value.
                    for binding in &event.bindings {
                        if !binding.is_type_only {
                            returned.push(binding.name);
                        }
                    }
                }
            }
            ScriptItem::Declaration(decl) => {
                // Track declarations by name to include in return statement
                // Use name instead of span to avoid including function bodies
                if let Some(name) = decl.name {
                    returned.push(name);
                }
            }
            ScriptItem::TypeDeclaration(type_decl) => {
                if opts.keep_ts_types {
                    // Move TypeScript declarations outside the component (to where imports go)
                    // This ensures interfaces/types are at module scope, not inside setup()
                    code_transform.move_with_suffix(
                        type_decl.span.start,
                        type_decl.span.end,
                        script.tag_open_start,
                        "\n",
                    );
                } else {
                    match type_decl.kind {
                        TypeDeclarationKind::Enum => {
                            // TODO convert to JS enum-like object instead of removing
                            code_transform.remove(type_decl.span.start, type_decl.span.end);
                        }
                        _ => {
                            // Remove interfaces, type aliases, namespaces
                            code_transform.remove(type_decl.span.start, type_decl.span.end);
                        }
                    }
                }
            }
            ScriptItem::Async(e) => {
                // Wrap top-level await with async context helper
                code_transform.overwrite(
                    e.span.start,
                    e.span.start + 5,
                    r#"
;(([__temp,__restore]=_withAsyncContext(()=>"#,
                );

                code_transform.prepend_left(
                    e.span.end,
                    r#")),await __temp,__restore())
"#,
                );
            }
            ScriptItem::Macro(macro_item) => {
                if let ScriptMacro::DefineEmits { declarator, .. } = macro_item {
                    if declarator.is_some() {
                        has_emit_declarator = true;
                    }
                };

                // Track defineExpose before processing (it returns None but we need to track it)
                if matches!(macro_item.kind(), VueMacroKind::DefineExpose) {
                    expose = Some(true);
                }

                let mut result = process_macro(
                    script,
                    macro_item,
                    code_transform,
                    opts.source,
                    opts.is_production,
                );

                // Collect diagnostics from macro processing
                if let Some(ref mut r) = result {
                    if let Some(d) = r.diagnostic.take() {
                        diagnostics.push(d);
                    }
                }

                // Handle macros that don't return a result
                if result.is_none() {
                    // DefineSlots needs import even though it returns None
                    if matches!(macro_item.kind(), VueMacroKind::DefineSlots) {
                        imports.add(ScriptSetupImportDependencies::USE_SLOTS);
                    }
                    continue;
                }

                match macro_item.kind() {
                    VueMacroKind::DefineProps => {
                        prop = Some(result);
                    }
                    VueMacroKind::WithDefaults => {
                        prop = Some(result);
                    }
                    VueMacroKind::DefineOptions => {
                        options = Some(result);
                    }
                    VueMacroKind::DefineModel => {
                        imports.add(ScriptSetupImportDependencies::USE_MODEL);
                        models.push(result);
                    }
                    VueMacroKind::DefineExpose => {
                        // Already tracked above
                    }
                    VueMacroKind::DefineEmits => {
                        emit = Some(result);
                    }
                    VueMacroKind::DefineSlots => {
                        // Import already added above
                    }
                }
            }
            _ => {}
        }
    }

    let needs_processing = if let Some(Some(opt)) = options {
        if let Some(span) = opt.move_span {
            code_transform.move_wrapped(
                span.start,
                span.end,
                script.tag_open_end,
                if is_typescript {
                    imports.add(ScriptSetupImportDependencies::DEFINE_COMPONENT);
                    "_defineComponent({..."
                } else {
                    "Object.assign("
                },
                if is_typescript { "," } else { ",{" },
            );
            false
        } else {
            true
        }
    } else {
        true
    };
    if needs_processing {
        if is_typescript {
            imports.add(ScriptSetupImportDependencies::DEFINE_COMPONENT);
            code_transform.prepend_left(script.tag_open_end, "_defineComponent({\n");
        } else {
            code_transform.prepend_left(script.tag_open_end, "{\n");
        }
    }
    {
        let mut buf = String::with_capacity(opts.component_name.len() + 32);
        buf.push_str("__name: '");
        buf.push_str(opts.component_name);
        buf.push_str("',");
        if opts.is_vapor {
            buf.push_str("__vapor: true,");
        }
        code_transform.prepend_left(script.tag_open_end, &buf);
    }

    // Process props and emits sections
    let insert_pos = script.tag_open_end;
    let props_needs_merge = emit_props_section(code_transform, prop, &models, insert_pos);
    let emits_needs_merge = emit_emits_section(code_transform, emit, models, insert_pos);

    // Add mergeModels import if needed by either section
    if props_needs_merge || emits_needs_merge {
        imports.add(ScriptSetupImportDependencies::MERGE_MODELS);
    }

    // Production mode: minimal setup signature unless expose/emit needed
    // Development mode: full signature with expose for devtools
    let needs_expose_in_signature = !opts.is_production || expose.is_some();
    let needs_emit_in_signature = has_emit_declarator;

    if needs_expose_in_signature || needs_emit_in_signature {
        let mut buf = String::with_capacity(48);
        buf.push_str("setup(__props,{");
        if needs_expose_in_signature {
            buf.push_str("expose:__expose");
        }
        if needs_emit_in_signature {
            if needs_expose_in_signature {
                buf.push_str(",emit:__emit");
            } else {
                buf.push_str("emit:__emit");
            }
        }
        buf.push_str("}){");
        code_transform.prepend_left(script.tag_open_end, &buf);
    } else {
        // Minimal signature for production
        code_transform.prepend_left(script.tag_open_end, "setup(__props){");
    }

    // Auto-call __expose() only in development mode (when expose is in signature)
    if !opts.is_production && expose.is_none() {
        code_transform.prepend_left(script.tag_open_end, "__expose();");
    }

    // Replace </script> closing tag
    let closing_paren = if is_typescript || !needs_processing {
        ")"
    } else {
        ""
    };

    // Build closing text and apply it.
    // When inline_template is true, setup() is left open for the template to provide
    // the return value (the arrow render function). The deferred_closing is emitted
    // AFTER the template content by ScriptGeneratorPlugin::end().
    let (closing_text, deferred_closing) = if opts.inline_template {
        // Leave setup() open — template will provide `return (_ctx,_cache) => { ... }`
        // The deferred closing will close setup() and the component definition.
        let mut deferred = String::with_capacity(8);
        deferred.push_str("\n}}");
        deferred.push_str(closing_paren);
        deferred.push(';');
        ("\n".to_string(), Some(deferred))
    } else {
        let joined = returned.join(", ");
        let mut buf = String::with_capacity(joined.len() + 128);
        if opts.is_production {
            buf.push_str("\nreturn {");
            buf.push_str(&joined);
            buf.push_str("}\n}}");
            buf.push_str(closing_paren);
            buf.push_str(";\n");
        } else {
            buf.push_str("\nconst __returned__={");
            buf.push_str(&joined);
            buf.push_str("}\nObject.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })\nreturn __returned__\n}}");
            buf.push_str(closing_paren);
            buf.push(';');
        }
        (buf, None)
    };

    code_transform.overwrite(script.tag_close_start, script.tag_close_end, &closing_text);

    // TODO remove typescript types if keep_ts_types is false (eg: playground mode)
    // // Note: We don't move the script block. After template processing moves its content
    // // to the end of the file, the script block naturally appears first in the output.
    // // This avoids complex move interactions.

    // // Strip TypeScript type annotations when keep_ts is false (playground mode)
    // if !opts.keep_ts_types {
    //     let script_content =
    //         &source[info.event.content_start as usize..info.event.content_end as usize];
    //     strip_typescript_types(
    //         &info.event.program,
    //         code_transform,
    //         info.event.content_start,
    //         script_content,
    //     );
    // }

    ProcessedScript {
        imports,
        diagnostics,
        deferred_closing,
    }
}
