//! Script lowering into the module composer's declared fragment slots.

use oxc_allocator::Allocator;
use verter_audit::observe::ObserveExec;

use crate::assembly::emit_plan::EmitPlanError;
use crate::assembly::vue_module::{SfcSlots, VueScriptFragment};
use crate::code_transform::CodeTransform;
use crate::compile::{extract_block_ranges, remove_inter_block_gaps};
use crate::emission::{EmissionFragment, MarkerId, QualifiedSource};
use crate::parser::types::ParsedSfc;
use crate::script::bindings::ScriptLaneBindings;
use crate::script::prepared::PreparedScript;
use crate::script::{generate_script, ScriptCodeGenOptions};

pub(crate) struct LoweredScript<'s> {
    fragment: EmissionFragment<'s>,
    slots: SfcSlots,
    helpers: Vec<&'static str>,
}

impl<'s> LoweredScript<'s> {
    pub(crate) fn into_fragment(self, helpers: &'s [&'static str]) -> VueScriptFragment<'s> {
        VueScriptFragment {
            fragment: self.fragment,
            slots: self.slots,
            helpers,
            witness: None,
        }
    }

    pub(super) fn helpers(&self) -> Vec<&'static str> {
        self.helpers.clone()
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "The assembly boundary preserves the source lifetime across parsed, prepared, execution and emission inputs."
)]
pub(super) fn lower_script<'s, M: ObserveExec>(
    source: QualifiedSource<'s>,
    parsed: &ParsedSfc,
    prepared: &PreparedScript<'s>,
    lane: ScriptLaneBindings<'s>,
    options: &ScriptCodeGenOptions<'_>,
    runtime_module: &str,
    allocator: &'s Allocator,
    mode: crate::assembly::emit_plan::MapMode,
) -> Result<Option<LoweredScript<'s>>, EmitPlanError> {
    if parsed.script().is_none() && parsed.script_setup().is_none() {
        let generated = if options.has_scoped_style || options.is_vapor {
            Some(crate::script::component_scaffold(
                crate::script::SFC_BINDING,
                options.is_vapor,
                options.has_scoped_style.then_some(options.scope_id),
            ))
        } else if parsed.template_ast().is_none()
            && parsed.style_nodes().is_empty()
            && parsed.unknown_nodes().is_empty()
        {
            Some(crate::script::empty_component_scaffold(
                options.component_name,
                runtime_module,
            ))
        } else {
            None
        };
        if let Some((code, bindings)) = generated {
            let code = allocator.alloc_str(&code);
            let mut builder = crate::emission::EmissionBuilder::new(mode);
            let mut cursor = 0;
            let mut markers = Vec::new();
            for binding in bindings {
                builder.push_borrowed(&code[cursor..binding.start as usize]);
                let marker = MarkerId::fresh();
                builder
                    .push_marked(
                        crate::emission::Piece::new(
                            crate::emission::PieceText::Borrowed(
                                &code[binding.start as usize..binding.end as usize],
                            ),
                            crate::emission::Provenance::Unmapped,
                        ),
                        marker,
                    )
                    .map_err(EmitPlanError::from)?;
                markers.push(marker);
                cursor = binding.end as usize;
            }
            builder.push_borrowed(&code[cursor..]);
            let fragment = builder.finish().map_err(EmitPlanError::from)?;
            let slots = SfcSlots::of(&fragment, &markers, None, None);
            return Ok(Some(LoweredScript {
                fragment,
                slots,
                helpers: Vec::new(),
            }));
        }
        return Ok(None);
    }
    let mut transform = CodeTransform::<M>::with_mode_in(source.text, allocator, mode);
    let result = generate_script(
        parsed.script(),
        parsed.script_setup(),
        prepared,
        source.text,
        &mut transform,
        allocator,
        options,
        lane,
    )
    .map_err(EmitPlanError::Edit)?;
    // Keep only script blocks. The parser owns every block range and gap.
    let blocks = extract_block_ranges(parsed, source.text);
    for (start, end) in &blocks {
        let script = [parsed.script(), parsed.script_setup()]
            .into_iter()
            .flatten()
            .any(|script| script.tag_open.start == *start);
        if !script {
            transform
                .try_remove(*start, *end)
                .map_err(EmitPlanError::Edit)?;
        }
    }
    if parsed.script_setup().is_some() {
        if let Some(companion) = parsed.script() {
            transform
                .try_remove(companion.tag_open.start, companion.tag_open.end)
                .map_err(EmitPlanError::Edit)?;
            if let Some(close) = &companion.tag_close {
                transform
                    .try_remove(close.start, close.end)
                    .map_err(EmitPlanError::Edit)?;
            }
        }
    }
    remove_inter_block_gaps(&mut transform, source.text.len() as u32, &blocks);
    if !options.keep_ts_types {
        if let Some(setup) = prepared.setup() {
            crate::strip_types::typescript::strip_typescript_body_types(
                setup.program(),
                &mut transform,
                setup.content_start(),
                setup.content_str(),
            );
        }
        if let Some(companion) = prepared.companion() {
            crate::strip_types::typescript::strip_typescript_types(
                companion.program(),
                &mut transform,
                companion.content_start(),
                companion.content_str(),
            );
        }
    }
    let setup_return = if options.inline_template {
        Some(result.inline_inject_pos.ok_or(EmitPlanError::UnknownSlot)?)
    } else {
        None
    };
    freeze_script(
        transform,
        source,
        result.sfc_binding_markers,
        result.sfc_export_statement_marker,
        setup_return,
        result.imports,
    )
    .map(Some)
}

/// Freeze the lowering's declared positions, including its inline return
/// hole. The comparison selector already owns the lowered transform and
/// uses this same boundary without running script lowering again.
pub(crate) fn freeze_script<'s, M: ObserveExec>(
    mut transform: CodeTransform<'s, M>,
    source: QualifiedSource<'s>,
    binding_markers: Vec<crate::code_transform::GeneratedContentMarker<'s>>,
    export_marker: Option<crate::code_transform::GeneratedContentMarker<'s>>,
    setup_return: Option<u32>,
    helpers: Vec<&'static str>,
) -> Result<LoweredScript<'s>, EmitPlanError> {
    let setup_return = setup_return
        .map(|position| {
            const HOLE: &str = "/* setup return */";
            transform
                .prepend_left_with_generated_marker(position, HOLE, 0, HOLE.len() as u32)
                .ok_or(EmitPlanError::UnknownSlot)
        })
        .transpose()?;
    let mut marks = Vec::new();
    let mut bindings = Vec::new();
    for marker in binding_markers {
        let id = MarkerId::fresh();
        marks.push((marker, id));
        bindings.push(id);
    }
    let export = export_marker.map(|marker| {
        let id = MarkerId::fresh();
        marks.push((marker, id));
        id
    });
    let setup_return = setup_return.map(|marker| {
        let id = MarkerId::fresh();
        marks.push((marker, id));
        id
    });
    let fragment = transform
        .freeze_marked(source, &marks)
        .map_err(|_| EmitPlanError::UnknownSlot)?;
    let slots = SfcSlots::of(&fragment, &bindings, export, setup_return);
    Ok(LoweredScript {
        fragment,
        slots,
        helpers,
    })
}
