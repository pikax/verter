use crate::{
    code_transform::CodeTransform,
    syntax::plugins::code_gen::script::macros::types::MacroProcessReturn,
};

/// Process props section: applies macro transformations and integrates with models.
///
/// Returns `true` if mergeModels import is needed.
pub fn emit_props_section<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    prop: Option<Option<MacroProcessReturn>>,
    models: &[Option<MacroProcessReturn>],
    insert_pos: u32,
) -> bool {
    let mut needs_merge_models = false;
    let mut processed = false;

    if let Some(Some(process)) = prop {
        processed = true;
        if let Some(span) = process.move_span {
            if !models.is_empty() {
                needs_merge_models = true;
            }

            code_transform.move_wrapped(
                span.start,
                span.end,
                insert_pos,
                if models.is_empty() {
                    "props:"
                } else {
                    "props:/*@__PURE__*/_mergeModels("
                },
                if models.is_empty() { ",\n" } else { ",{" },
            );
        }
        if let Some((span, s)) = process.overwrite_span {
            code_transform.overwrite(span.start, span.end, s.as_str());
        }
        if let Some(span) = process.remove {
            code_transform.remove(span.start, span.end);
        }

        if !models.is_empty() {
            for model in models.iter().flatten() {
                if let Some((span, name)) = &model.overwrite_span {
                    code_transform.move_wrapped(
                        span.start,
                        span.end,
                        insert_pos,
                        format!("\"{}\":", name).as_str(),
                        format!(",\"{}Modifiers\":{{}},", name).as_str(),
                    );
                }
            }
            code_transform.append_left(insert_pos, "}),");
        }
    }

    // Handle models-only props (no defineProps but has defineModel)
    if !processed && !models.is_empty() {
        code_transform.append_left(insert_pos, "props:{");
        for model in models.iter().flatten() {
            if let Some((span, name)) = &model.overwrite_span {
                if span.start == 0 {
                    code_transform.prepend_left(
                        insert_pos,
                        format!("\"{}\":{{}},\"{}Modifiers\":{{}},", name, name).as_str(),
                    );
                } else {
                    code_transform.move_wrapped(
                        span.start,
                        span.end,
                        insert_pos,
                        format!("\"{}\":", name).as_str(),
                        format!(",\"{}Modifiers\":{{}},", name).as_str(),
                    );
                }
            }
        }
        code_transform.append_left(insert_pos, "},");
    }

    needs_merge_models
}
/// Process emits section: applies macro transformations and integrates with models.
///
/// Returns `true` if mergeModels import is needed.
pub fn emit_emits_section<'a>(
    code_transform: &mut CodeTransform<'a>,
    emit: Option<Option<MacroProcessReturn>>,
    models: Vec<Option<MacroProcessReturn>>,
    insert_pos: u32,
) -> bool {
    let mut needs_merge_models = false;
    let mut processed = false;

    if let Some(Some(process)) = emit {
        processed = true;
        if let Some(span) = process.move_span {
            if !models.is_empty() {
                needs_merge_models = true;
            }

            code_transform.move_wrapped(
                span.start,
                span.end,
                insert_pos,
                if models.is_empty() {
                    "emits:"
                } else {
                    "emits:/*@__PURE__*/_mergeModels("
                },
                if models.is_empty() { ",\n" } else { ",[" },
            );
        }
        if let Some((span, s)) = process.overwrite_span {
            code_transform.overwrite(span.start, span.end, s.as_str());
        }
        if let Some(span) = process.remove {
            code_transform.remove(span.start, span.end);
        }

        if !models.is_empty() {
            // Batch all model emit entries + close into a single prepend_left.
            // prepend_left is LIFO, so batching preserves order and avoids N+1 Vec::insert calls.
            let mut buf = String::new();
            for model in models.iter().flatten() {
                if let Some((_span, name)) = &model.overwrite_span {
                    buf.push_str("\"update:");
                    buf.push_str(name);
                    buf.push_str("\",");
                }
            }
            buf.push_str("]),");
            code_transform.prepend_left(insert_pos, &buf);
        }
    }

    // Handle models-only emits (no defineEmits but has defineModel)
    if !processed && !models.is_empty() {
        // Batch all emits into a single prepend_left to avoid multiple Vec::insert calls
        let mut buf = String::from("emits:[");
        for model in models.into_iter().flatten() {
            if let Some((_span, name)) = model.overwrite_span {
                buf.push_str("\"update:");
                buf.push_str(&name);
                buf.push_str("\",");
            }
        }
        buf.push_str("],");
        code_transform.prepend_left(insert_pos, &buf);
    }

    needs_merge_models
}
