//! Declaration construction: a framework's public contract plus the captured
//! dependency closure, assembled into one declaration source and its map.

use super::*;
use rustc_hash::FxHashSet;
use verter_audit::observe::Uncaptured;
use verter_identity::identity::{ContentId, SourceUnitId};
use verter_span::Span;

use crate::assembly::emit_plan::{
    EmitPlanBuilder, EmitPlanError, MapMode, QualifiedSource, SourceKey,
};
use crate::assembly::source_space::SourceSpaceKind;
use crate::assembly::source_unit::carrier_source_id;
use crate::framework_common::generated_identifier::is_generated_identifier;
use crate::framework_common::virtual_file_naming::{
    classify_carrier_companion_reference, CarrierCompanionKind,
};
use crate::ide::vue_projection::binder_capture::{
    declaration_reference_inventory, BinderCapturePlan, DependencyOrigin,
};
use crate::tsc::script::GeneratedMapping;
use crate::work_ledger::CompilerWorkLedger;

/// What a framework fills into the product.
pub struct DeclarationContractInput<'a> {
    pub module: DeclarationModuleIdentity,
    /// The public declaration text (default export and its types).
    pub public_declaration: &'a str,
    /// Authored imports and module-scope declarations, with carrier ranges.
    pub module_scope: &'a [(u32, u32)],
    pub source: &'a str,
}

pub fn build_declaration_product(
    input: DeclarationContractInput<'_>,
    capture: &BinderCapturePlan,
) -> DeclarationProjectionProduct {
    let qualified = QualifiedSource {
        key: SourceKey {
            unit: SourceUnitId::from_lineage(
                &carrier_source_id(&input.module.canonical_id),
                "carrier",
            ),
            content: ContentId::from_content_bytes(input.source.as_bytes()),
            space: SourceSpaceKind::Original,
        },
        text: input.source,
    };
    let mut result = DeclarationProjectionProduct {
        module: input.module,
        declaration_source: String::new(),
        closure: DeclarationDependencyClosure::default(),
        map: DeclarationMapProjection::default(),
        diagnostics: Vec::new(),
    };
    let mut plan = EmitPlanBuilder::new(MapMode::Map);
    for &(start, end) in input.module_scope {
        if input.source.get(start as usize..end as usize).is_none() {
            result.diagnostics.push(DeclarationDiagnostic {
                start,
                end,
                message: "Invalid authored declaration range".into(),
            });
        }
    }
    let dependencies = capture
        .slices
        .iter()
        .flat_map(|slice| &slice.dependencies)
        .chain(
            capture
                .declarations
                .iter()
                .flat_map(|declaration| &declaration.dependencies),
        );
    let needed_imports: FxHashSet<usize> = dependencies
        .filter_map(|dependency| match &dependency.origin {
            DependencyOrigin::Import { index } => Some(*index),
            _ => None,
        })
        .collect();
    let mut emitted_imports = FxHashSet::default();
    for (index, import) in capture.imports.iter().enumerate() {
        if !needed_imports.contains(&index)
            || !emitted_imports.insert((import.span.start, import.span.end))
        {
            continue;
        }
        let Some(text) = input
            .source
            .get(import.span.start as usize..import.span.end as usize)
        else {
            refuse(
                &mut result,
                ClosureRefusal::InaccessibleCapture {
                    name: import.local.clone(),
                },
            );
            continue;
        };
        if plan
            .push_source(&qualified, Span::new(import.span.start, import.span.end))
            .is_err()
        {
            refuse(
                &mut result,
                ClosureRefusal::InaccessibleCapture {
                    name: import.local.clone(),
                },
            );
            continue;
        }
        plan.push_unmapped("\n");
        result.closure.retained_imports.push(text.to_owned());
    }
    let mut emitted = FxHashSet::default();
    for declaration in &capture.declarations {
        if !emitted.insert((declaration.span.start, declaration.span.end)) {
            continue;
        }
        if !declaration.binder_params.is_empty() {
            refuse(
                &mut result,
                ClosureRefusal::InaccessibleCapture {
                    name: declaration.name.clone(),
                },
            );
            continue;
        }
        match declaration.emission.as_ref() {
            Some(fragment) => {
                // An authored export stays one: a `.d.ts` with a default
                // export exports nothing implicitly.
                if declaration.exported {
                    plan.push_unmapped("export ");
                }
                if append_rendered(&mut plan, &qualified, fragment.text(), fragment.mappings())
                    .is_err()
                {
                    refuse(
                        &mut result,
                        ClosureRefusal::InaccessibleCapture {
                            name: declaration.name.clone(),
                        },
                    );
                    continue;
                }
                plan.push_unmapped("\n");
                result.closure.lifted.push(fragment.text().to_owned());
            }
            None => refuse(
                &mut result,
                ClosureRefusal::InaccessibleCapture {
                    name: declaration.name.clone(),
                },
            ),
        }
    }
    for duplicate in &capture.duplicates {
        refuse(
            &mut result,
            ClosureRefusal::InaccessibleCapture {
                name: duplicate.name.clone(),
            },
        );
    }
    plan.push_unmapped(input.public_declaration);
    match plan
        .finish()
        .and_then(|plan| plan.write(&mut CompilerWorkLedger::<Uncaptured>::new()))
    {
        Ok(artifact) => {
            result.declaration_source = artifact.text().to_owned();
            result.map.mappings = artifact
                .map()
                .expect("declaration map capture was selected")
                .segments()
                .iter()
                .map(|segment| DeclarationMapping {
                    generated_start: segment.generated.start,
                    generated_end: segment.generated.end,
                    source_start: segment.source_start,
                    source_end: segment.source_start + segment.generated.end
                        - segment.generated.start,
                })
                .collect();
        }
        Err(error) => result.diagnostics.push(DeclarationDiagnostic {
            start: 0,
            end: 0,
            message: format!("Declaration assembly failed: {error:?}"),
        }),
    }
    match declaration_reference_inventory(&result.declaration_source) {
        Some((names, specifiers)) => {
            // A free name is a checking helper, or an authored binding the
            // closure did not carry; a global of that name would answer.
            for name in names {
                if is_generated_identifier(&name)
                    || capture.module_bindings.binary_search(&name).is_ok()
                {
                    refuse(&mut result, ClosureRefusal::InaccessibleCapture { name });
                }
            }
            for specifier in specifiers {
                if classify_carrier_companion_reference(&specifier.text)
                    .is_some_and(|companion| companion.kind != CarrierCompanionKind::Declaration)
                {
                    refuse(
                        &mut result,
                        ClosureRefusal::PrivateCheckingPath {
                            text: specifier.text,
                        },
                    );
                } else if verter_session_query::resolution::is_absolute_specifier(&specifier.text) {
                    refuse(
                        &mut result,
                        ClosureRefusal::WorkspaceOnlyImport {
                            specifier: specifier.text,
                        },
                    );
                }
            }
        }
        _ => result.diagnostics.push(DeclarationDiagnostic {
            start: 0,
            end: 0,
            message: "Declaration references could not be validated".into(),
        }),
    }
    result
}

fn refuse(result: &mut DeclarationProjectionProduct, refusal: ClosureRefusal) {
    if !result.closure.refused.contains(&refusal) {
        result.diagnostics.push(DeclarationDiagnostic {
            start: 0,
            end: 0,
            message: format!("Unpublishable declaration dependency: {refusal:?}"),
        });
        result.closure.refused.push(refusal);
    }
}

/// Place the declaration owner's exact authored runs and unmapped generated
/// pieces into the shared emission plan; final bytes and maps have one writer.
fn append_rendered<'s>(
    plan: &mut EmitPlanBuilder<'s>,
    source: &QualifiedSource<'s>,
    text: &str,
    mappings: &[GeneratedMapping],
) -> Result<(), EmitPlanError> {
    let mut cursor = 0;
    for (index, mapping) in mappings.iter().enumerate() {
        let start = mapping.generated_offset;
        let end = mappings
            .get(index + 1)
            .map_or(text.len(), |next| next.generated_offset);
        if start > cursor {
            plan.push_unmapped(&text[cursor..start]);
        }
        if let Some(span) = mapping.source_span {
            if source.text.get(span.start as usize..span.end as usize) != text.get(start..end) {
                return Err(EmitPlanError::InvalidSourceSlice { span });
            }
            plan.push_source(source, span)?;
        } else {
            plan.push_unmapped(&text[start..end]);
        }
        cursor = end;
    }
    if cursor < text.len() {
        plan.push_unmapped(&text[cursor..]);
    }
    Ok(())
}
