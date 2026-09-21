//! Props for slot inference, emitted inside the template's existing lexical scope.
//! The duplicate inference expression is unmapped; the JSX props retain navigation.

use crate::ast::types::ElementNode;
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::expression::{
    build_prefixed_expr_segments, resolve_simple_expr_segments,
};
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};

pub(super) fn component_props(
    element: &ElementNode,
    parsed: Option<&OxcParsedElement<'_>>,
    source: &str,
    resolver: &BindingResolver<'_>,
) -> String {
    let resolve = |start: u32, end: u32, parsed: Option<&OxcParsedExpression<'_>>| {
        let text = &source[start as usize..end as usize];
        match parsed {
            Some(parsed) => build_prefixed_expr_segments(text, start, parsed, resolver, &[]).text,
            None => resolve_simple_expr_segments(resolver, text, start).text,
        }
    };
    let mut entries = Vec::new();
    for (index, prop) in element.props.iter().enumerate() {
        let parsed = parsed.and_then(|element| element.prop(index));
        if !prop.is_directive {
            let name = super::props::normalized_component_prop_name(
                true,
                &source[prop.start as usize..prop.name_end as usize],
            );
            let value = match (prop.value_start, prop.value_end) {
                (Some(start), Some(end)) => {
                    serde_json::to_string(&source[start as usize..end as usize]).unwrap()
                }
                _ => "true".to_owned(),
            };
            entries.push(format!(
                "{}: {value}",
                serde_json::to_string(&name).unwrap()
            ));
            continue;
        }
        let directive = super::directive_name(prop, source);
        if !matches!(directive, "bind" | "model") {
            // Events are checked in JSX. Inference takes the covariant prop
            // inputs, without re-evaluating inline statements or $event here.
            continue;
        }
        let dot_key = source[prop.start as usize..prop.name_end as usize].strip_prefix('.');
        let value = match (prop.value_start, prop.value_end) {
            (Some(start), Some(end)) if start < end => {
                resolve(start, end, parsed.and_then(|prop| prop.exp.as_ref()))
            }
            _ => match (prop.arg_start, prop.arg_end) {
                (Some(start), Some(end)) => resolver.resolve_simple_expr(
                    &super::props::kebab_to_camel_case(source[start as usize..end as usize].trim()),
                ),
                _ => match dot_key {
                    Some(key) => {
                        resolver.resolve_simple_expr(&super::props::kebab_to_camel_case(key))
                    }
                    None => continue,
                },
            },
        };
        let key = match (prop.arg_start, prop.arg_end) {
            (Some(start), Some(end)) if prop.is_dynamic == Some(true) => format!(
                "[{}]",
                resolve(
                    start + 1,
                    end - 1,
                    parsed.and_then(|prop| prop.arg.as_ref())
                )
            ),
            (Some(start), Some(end)) => {
                serde_json::to_string(&super::props::normalized_component_prop_name(
                    true,
                    &source[start as usize..end as usize],
                ))
                .unwrap()
            }
            _ if dot_key.is_some() => serde_json::to_string(
                &super::props::normalized_component_prop_name(true, dot_key.unwrap()),
            )
            .unwrap(),
            _ if directive == "model" => "\"modelValue\"".to_owned(),
            _ => {
                entries.push(format!("...({value})"));
                continue;
            }
        };
        entries.push(format!("{key}: ({value})"));
    }
    format!("{{{}}}", entries.join(", "))
}
