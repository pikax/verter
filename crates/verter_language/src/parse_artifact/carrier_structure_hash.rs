use crate::registered_source_authority::sha256;

use super::carrier_inventory::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CarrierStructureHash([u8; 32]);

impl CarrierStructureHash {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub fn compute_carrier_structure_hash(inventory: &CarrierBlockInventory) -> CarrierStructureHash {
    let mut out = Vec::new();
    out.extend_from_slice(b"verter.carrier-structure.v1\0");
    push_u32(&mut out, inventory.blocks().len() as u32);
    for block in inventory.blocks() {
        match block {
            CarrierBlock::Section { role, syntax, .. } => {
                out.push(1);
                push_section_role(&mut out, inventory, role);
                push_name(&mut out, inventory, syntax.normalized_name);
                push_attributes(&mut out, inventory, &syntax.attributes);
                push_termination(&mut out, &syntax.termination);
            }
            CarrierBlock::MarkupRoot { node, .. } => {
                out.push(2);
                push_u32(&mut out, node.0);
            }
        }
    }
    push_u32(&mut out, inventory.markup().nodes().len() as u32);
    for node in inventory.markup().nodes() {
        push_u32(&mut out, node.root_block.0);
        push_optional_u32(&mut out, node.parent.map(|id| id.0));
        let children = &inventory.markup().child_ids()
            [node.children.start as usize..node.children.end as usize];
        push_u32(&mut out, children.len() as u32);
        for child in children {
            push_u32(&mut out, child.0);
        }
        push_node(&mut out, inventory, &node.kind);
    }
    CarrierStructureHash(sha256(&[&out]))
}

fn push_node(out: &mut Vec<u8>, inventory: &CarrierBlockInventory, kind: &MarkupNodeKind) {
    match kind {
        MarkupNodeKind::Element(element) => {
            out.push(1);
            push_name(out, inventory, element.normalized_name);
            push_namespace(out, element.namespace);
            push_element_kind(out, inventory, &element.kind);
            out.extend_from_slice(&[
                element.self_closing as u8,
                element.void_element as u8,
                element.raw_text as u8,
            ]);
            push_termination(out, &element.termination);
            push_attributes(out, inventory, &element.attributes);
        }
        MarkupNodeKind::Text { content_span } => {
            out.push(2);
            push_span_text(out, inventory, *content_span);
        }
        MarkupNodeKind::Comment {
            content_span,
            termination,
            ..
        } => {
            out.push(3);
            push_span_text(out, inventory, *content_span);
            push_termination(out, termination);
        }
        MarkupNodeKind::Interpolation {
            family,
            expression_span,
            termination,
            ..
        } => {
            out.push(4);
            out.push(match family {
                MarkupInterpolationFamily::VueInterpolation => 1,
                MarkupInterpolationFamily::SvelteInterpolation => 2,
            });
            push_span_text(out, inventory, *expression_span);
            push_termination(out, termination);
        }
        MarkupNodeKind::SvelteControlBlock(value) => {
            out.push(5);
            push_control_head(out, inventory, &value.head);
            push_termination(out, &value.termination);
        }
        MarkupNodeKind::SvelteClause(value) => {
            out.push(6);
            push_clause_head(out, inventory, &value.head);
            push_termination(out, &value.termination);
        }
        MarkupNodeKind::SvelteStandaloneTag(value) => {
            out.push(7);
            push_standalone(out, inventory, value);
        }
        MarkupNodeKind::Recovered {
            termination,
            expected,
            reason,
            ..
        } => {
            out.push(8);
            push_termination(out, termination);
            push_recovered_kind(out, *expected);
            push_recovery_reason(out, *reason);
        }
        MarkupNodeKind::Unknown {
            termination,
            authored_head,
            reason,
            ..
        } => {
            out.push(9);
            push_termination(out, termination);
            if let Some(slice) = authored_head {
                push_slice(out, inventory, *slice);
            }
            push_unknown_reason(out, *reason);
        }
    }
}

fn push_attributes(
    out: &mut Vec<u8>,
    inventory: &CarrierBlockInventory,
    attrs: &[CarrierAttribute],
) {
    push_u32(out, attrs.len() as u32);
    for attr in attrs {
        match attr {
            CarrierAttribute::Named {
                name,
                syntax,
                value,
                duplicate_of,
                ..
            } => {
                out.push(1);
                push_name(out, inventory, name.normalized);
                out.push(match syntax {
                    NamedAttributeSyntax::Explicit => 1,
                    NamedAttributeSyntax::SvelteShorthand => 2,
                });
                push_value(out, inventory, value);
                push_optional_u32(out, duplicate_of.map(|id| id.0));
            }
            CarrierAttribute::Spread {
                expression_span,
                termination,
                ..
            } => {
                out.push(2);
                push_span_text(out, inventory, *expression_span);
                push_termination(out, termination);
            }
            CarrierAttribute::Directive {
                family,
                local_name,
                argument,
                modifiers,
                value,
                duplicate_of,
                ..
            } => {
                out.push(3);
                push_directive_family(out, inventory, family);
                if let Some(name) = local_name {
                    push_name(out, inventory, name.normalized);
                } else {
                    out.push(0);
                }
                push_argument(out, inventory, argument);
                push_u32(out, modifiers.len() as u32);
                for modifier in modifiers.iter() {
                    push_name(out, inventory, modifier.normalized);
                }
                push_value(out, inventory, value);
                push_optional_u32(out, duplicate_of.map(|id| id.0));
            }
            CarrierAttribute::Attach {
                expression_span,
                termination,
                ..
            } => {
                out.push(4);
                push_span_text(out, inventory, *expression_span);
                push_termination(out, termination);
            }
        }
    }
}

fn push_value(out: &mut Vec<u8>, inventory: &CarrierBlockInventory, value: &AttributeValue) {
    match value {
        AttributeValue::Missing => out.push(0),
        AttributeValue::Static {
            raw,
            decoded,
            quote,
            ..
        } => {
            out.push(1);
            push_quote(out, *quote);
            push_slice(out, inventory, *raw);
            push_lazy_decode(out, decoded);
        }
        AttributeValue::Expression {
            syntax,
            expression_span,
            termination,
            ..
        } => {
            out.push(2);
            push_dynamic_syntax(out, *syntax);
            push_span_text(out, inventory, *expression_span);
            push_termination(out, termination);
        }
        AttributeValue::Mixed { parts, .. } => {
            out.push(3);
            push_u32(out, parts.len() as u32);
            for part in parts.iter() {
                match part {
                    AttributeValuePart::Static { raw, decoded } => {
                        out.push(1);
                        push_slice(out, inventory, *raw);
                        push_lazy_decode(out, decoded);
                    }
                    AttributeValuePart::Expression {
                        syntax,
                        expression_span,
                        termination,
                        ..
                    } => {
                        out.push(2);
                        push_dynamic_syntax(out, *syntax);
                        push_span_text(out, inventory, *expression_span);
                        push_termination(out, termination);
                    }
                }
            }
        }
    }
}
fn push_argument(out: &mut Vec<u8>, inventory: &CarrierBlockInventory, arg: &DirectiveArgument) {
    match arg {
        DirectiveArgument::None => out.push(0),
        DirectiveArgument::Static { name } => {
            out.push(1);
            push_name(out, inventory, name.normalized);
        }
        DirectiveArgument::Dynamic {
            expression_span,
            termination,
            ..
        } => {
            out.push(2);
            push_span_text(out, inventory, *expression_span);
            push_termination(out, termination);
        }
    }
}
fn push_control_head(
    out: &mut Vec<u8>,
    inventory: &CarrierBlockInventory,
    head: &SvelteControlBlockHead,
) {
    match head {
        SvelteControlBlockHead::If { condition } => {
            out.push(1);
            push_optional_span_text(out, inventory, *condition);
        }
        SvelteControlBlockHead::Each {
            iterable,
            item,
            index,
            key,
        } => {
            out.push(2);
            push_optional_span_text(out, inventory, *iterable);
            push_optional_span_text(out, inventory, *item);
            push_optional_span_text(out, inventory, *index);
            push_optional_span_text(out, inventory, *key);
        }
        SvelteControlBlockHead::Await {
            promise,
            inline_branch,
        } => {
            out.push(3);
            push_optional_span_text(out, inventory, *promise);
            push_inline_branch(out, inventory, inline_branch);
        }
        SvelteControlBlockHead::Key { expression } => {
            out.push(4);
            push_optional_span_text(out, inventory, *expression);
        }
        SvelteControlBlockHead::Snippet {
            authored_name,
            params_span,
            ..
        } => {
            out.push(5);
            push_slice(out, inventory, *authored_name);
            push_optional_span_text(out, inventory, *params_span);
        }
    }
}
fn push_clause_head(out: &mut Vec<u8>, inventory: &CarrierBlockInventory, head: &SvelteClauseHead) {
    match head {
        SvelteClauseHead::Else => out.push(1),
        SvelteClauseHead::ElseIf { condition } => {
            out.push(2);
            push_optional_span_text(out, inventory, *condition);
        }
        SvelteClauseHead::Then { binding } => {
            out.push(3);
            push_optional_span_text(out, inventory, *binding);
        }
        SvelteClauseHead::Catch { binding } => {
            out.push(4);
            push_optional_span_text(out, inventory, *binding);
        }
    }
}
fn push_standalone(
    out: &mut Vec<u8>,
    inventory: &CarrierBlockInventory,
    value: &SvelteStandaloneTagSyntax,
) {
    push_standalone_family(out, inventory, &value.family);
    push_optional_span_text(out, inventory, value.expression_span);
    push_termination(out, &value.termination);
}
fn push_name(out: &mut Vec<u8>, inventory: &CarrierBlockInventory, id: InternedNameId) {
    push_str(
        out,
        inventory
            .normalized_name(id)
            .expect("validated normalized name"),
    );
}
fn push_slice(out: &mut Vec<u8>, inventory: &CarrierBlockInventory, slice: SourceSlice) {
    push_str(out, inventory.slice(slice).expect("validated source slice"));
}
fn push_span_text(out: &mut Vec<u8>, inventory: &CarrierBlockInventory, span: SourceSpan) {
    push_str(
        out,
        inventory.slice_span(span).expect("validated source span"),
    );
}
fn push_optional_span_text(
    out: &mut Vec<u8>,
    inventory: &CarrierBlockInventory,
    span: Option<SourceSpan>,
) {
    if let Some(span) = span {
        out.push(1);
        push_span_text(out, inventory, span);
    } else {
        out.push(0);
    }
}
fn push_section_role(out: &mut Vec<u8>, inventory: &CarrierBlockInventory, role: &SectionRole) {
    match role {
        SectionRole::TemplateHost => out.push(1),
        SectionRole::Script { role, dialect } => {
            out.push(2);
            out.push(match role {
                ScriptRole::Instance => 1,
                ScriptRole::Setup => 2,
                ScriptRole::Module => 3,
            });
            match dialect {
                ScriptSourceType::JavaScript => out.push(1),
                ScriptSourceType::TypeScript => out.push(2),
                ScriptSourceType::Jsx => out.push(3),
                ScriptSourceType::Tsx => out.push(4),
                ScriptSourceType::Custom {
                    authored,
                    normalized,
                } => {
                    out.push(5);
                    push_slice(out, inventory, *authored);
                    push_name(out, inventory, *normalized);
                }
                ScriptSourceType::Missing => out.push(6),
            }
        }
        SectionRole::Style {
            dialect,
            scoped,
            module,
        } => {
            out.extend_from_slice(&[3, *scoped as u8]);
            match dialect {
                StyleDialect::Css => out.push(1),
                StyleDialect::Scss => out.push(2),
                StyleDialect::Sass => out.push(3),
                StyleDialect::Less => out.push(4),
                StyleDialect::Stylus => out.push(5),
                StyleDialect::PostCss => out.push(6),
                StyleDialect::Custom {
                    authored,
                    normalized,
                } => {
                    out.push(7);
                    push_slice(out, inventory, *authored);
                    push_name(out, inventory, *normalized);
                }
                StyleDialect::Missing => out.push(8),
            }
            match module {
                StyleModule::None => out.push(1),
                StyleModule::Default => out.push(2),
                StyleModule::Named { name } => {
                    out.push(3);
                    push_slice(out, inventory, *name);
                }
            }
        }
        SectionRole::Custom { normalized_name } => {
            out.push(4);
            push_str(out, normalized_name);
        }
    }
}

fn push_namespace(out: &mut Vec<u8>, value: MarkupNamespace) {
    out.push(match value {
        MarkupNamespace::Html => 1,
        MarkupNamespace::Svg => 2,
        MarkupNamespace::MathMl => 3,
        MarkupNamespace::Foreign => 4,
        MarkupNamespace::Unknown => 5,
    });
}

fn push_element_kind(
    out: &mut Vec<u8>,
    inventory: &CarrierBlockInventory,
    value: &MarkupElementKind,
) {
    match value {
        MarkupElementKind::Native => out.push(1),
        MarkupElementKind::Component => out.push(2),
        MarkupElementKind::DynamicComponent => out.push(3),
        MarkupElementKind::DynamicElement => out.push(4),
        MarkupElementKind::SvelteNestedStyle => out.push(5),
        MarkupElementKind::SvelteSpecial(kind) => {
            out.push(6);
            match kind {
                SvelteSpecialElementKind::Head => out.push(1),
                SvelteSpecialElementKind::Window => out.push(2),
                SvelteSpecialElementKind::Document => out.push(3),
                SvelteSpecialElementKind::Body => out.push(4),
                SvelteSpecialElementKind::Element => out.push(5),
                SvelteSpecialElementKind::Boundary => out.push(6),
                SvelteSpecialElementKind::Options => out.push(7),
                SvelteSpecialElementKind::Component => out.push(8),
                SvelteSpecialElementKind::SelfRef => out.push(9),
                SvelteSpecialElementKind::Fragment => out.push(10),
                SvelteSpecialElementKind::Unknown { authored_local } => {
                    out.push(11);
                    push_slice(out, inventory, *authored_local);
                }
            }
        }
        MarkupElementKind::Unknown => out.push(7),
    }
}

fn push_termination(out: &mut Vec<u8>, value: &SyntaxTermination) {
    match value {
        SyntaxTermination::Closed => out.push(1),
        SyntaxTermination::SelfClosing => out.push(2),
        SyntaxTermination::Void => out.push(3),
        SyntaxTermination::UnclosedEof => out.push(4),
        SyntaxTermination::Recovered {
            reason,
            recovery_span,
        } => {
            out.push(5);
            push_recovery_reason(out, *reason);
            out.push(recovery_span.is_some() as u8);
        }
    }
}

fn push_recovery_reason(out: &mut Vec<u8>, value: BlockRecoveryReason) {
    out.push(match value {
        BlockRecoveryReason::MissingCloseTag => 1,
        BlockRecoveryReason::MismatchedCloseTag => 2,
        BlockRecoveryReason::StrayCloseTag => 3,
        BlockRecoveryReason::UnterminatedOpenTag => 4,
        BlockRecoveryReason::UnterminatedAttribute => 5,
        BlockRecoveryReason::InvalidNesting => 6,
        BlockRecoveryReason::DuplicateSingletonRoot => 7,
        BlockRecoveryReason::InvalidSelfClosing => 8,
        BlockRecoveryReason::InvalidRawTextTermination => 9,
        BlockRecoveryReason::ParserRejectedSyntax => 10,
    });
}

fn push_recovered_kind(out: &mut Vec<u8>, value: RecoveredMarkupKind) {
    out.push(match value {
        RecoveredMarkupKind::Element => 1,
        RecoveredMarkupKind::Comment => 2,
        RecoveredMarkupKind::Interpolation => 3,
        RecoveredMarkupKind::SvelteControlBlock => 4,
        RecoveredMarkupKind::SvelteClause => 5,
        RecoveredMarkupKind::SvelteStandaloneTag => 6,
    });
}

fn push_unknown_reason(out: &mut Vec<u8>, value: UnknownMarkupReason) {
    out.push(match value {
        UnknownMarkupReason::ParserUnknownVariant => 1,
        UnknownMarkupReason::UnsupportedAuthoredHead => 2,
        UnknownMarkupReason::MalformedAuthoredHead => 3,
        UnknownMarkupReason::RecoveryBudgetExceeded => 4,
    });
}

fn push_directive_family(
    out: &mut Vec<u8>,
    inventory: &CarrierBlockInventory,
    value: &DirectiveFamily,
) {
    match value {
        DirectiveFamily::Vue(kind) => {
            out.push(1);
            match kind {
                VueDirectiveKind::Bind => out.push(1),
                VueDirectiveKind::On => out.push(2),
                VueDirectiveKind::Model => out.push(3),
                VueDirectiveKind::Show => out.push(4),
                VueDirectiveKind::If => out.push(5),
                VueDirectiveKind::ElseIf => out.push(6),
                VueDirectiveKind::Else => out.push(7),
                VueDirectiveKind::For => out.push(8),
                VueDirectiveKind::Slot => out.push(9),
                VueDirectiveKind::Pre => out.push(10),
                VueDirectiveKind::Cloak => out.push(11),
                VueDirectiveKind::Once => out.push(12),
                VueDirectiveKind::Memo => out.push(13),
                VueDirectiveKind::Html => out.push(14),
                VueDirectiveKind::Text => out.push(15),
                VueDirectiveKind::Custom {
                    authored,
                    normalized,
                } => {
                    out.push(16);
                    push_slice(out, inventory, *authored);
                    push_name(out, inventory, *normalized);
                }
            }
        }
        DirectiveFamily::Svelte(kind) => {
            out.push(2);
            match kind {
                SvelteDirectiveKind::Bind => out.push(1),
                SvelteDirectiveKind::On => out.push(2),
                SvelteDirectiveKind::Use => out.push(3),
                SvelteDirectiveKind::Class => out.push(4),
                SvelteDirectiveKind::Style => out.push(5),
                SvelteDirectiveKind::Let => out.push(6),
                SvelteDirectiveKind::Transition => out.push(7),
                SvelteDirectiveKind::In => out.push(8),
                SvelteDirectiveKind::Out => out.push(9),
                SvelteDirectiveKind::Animate => out.push(10),
                SvelteDirectiveKind::Custom => out.push(11),
                SvelteDirectiveKind::Unknown {
                    authored_family,
                    reason,
                } => {
                    out.push(12);
                    push_slice(out, inventory, *authored_family);
                    out.push(match reason {
                        UnknownDirectiveReason::ParserUnknownVariant => 1,
                        UnknownDirectiveReason::UnsupportedAuthoredPrefix => 2,
                        UnknownDirectiveReason::MalformedAuthoredPrefix => 3,
                    });
                }
            }
        }
    }
}

fn push_quote(out: &mut Vec<u8>, value: AttributeQuote) {
    out.push(match value {
        AttributeQuote::Unquoted => 1,
        AttributeQuote::Single => 2,
        AttributeQuote::Double => 3,
    });
}
fn push_dynamic_syntax(out: &mut Vec<u8>, value: AttributeDynamicSyntax) {
    out.push(match value {
        AttributeDynamicSyntax::VueBracedExpression => 1,
        AttributeDynamicSyntax::VueDynamicArgument => 2,
        AttributeDynamicSyntax::VueShorthand => 3,
        AttributeDynamicSyntax::SvelteMustacheExpression => 4,
        AttributeDynamicSyntax::SvelteShorthand => 5,
        AttributeDynamicSyntax::SvelteExpressionTag => 6,
    });
}
fn push_lazy_decode(out: &mut Vec<u8>, value: &LazyDecodedText) {
    match value {
        LazyDecodedText::SameAsSource => out.push(1),
        LazyDecodedText::EntityDecode { key } => {
            out.push(2);
            match key.recipe {
                EntityDecodeRecipe::Html5Text => out.push(1),
                EntityDecodeRecipe::Html5Attribute { quote } => {
                    out.push(2);
                    push_quote(out, quote);
                }
                EntityDecodeRecipe::XmlText => out.push(3),
                EntityDecodeRecipe::XmlAttribute { quote } => {
                    out.push(4);
                    out.push(match quote {
                        QuotedAttributeQuote::Single => 1,
                        QuotedAttributeQuote::Double => 2,
                    });
                }
                EntityDecodeRecipe::SvelteText => out.push(5),
                EntityDecodeRecipe::SvelteAttribute { quote } => {
                    out.push(6);
                    push_quote(out, quote);
                }
            }
        }
    }
}

fn push_inline_branch(
    out: &mut Vec<u8>,
    inventory: &CarrierBlockInventory,
    value: &SvelteAwaitInlineBranch,
) {
    match value {
        SvelteAwaitInlineBranch::None => out.push(1),
        SvelteAwaitInlineBranch::Then { binding, .. } => {
            out.push(2);
            push_optional_span_text(out, inventory, *binding);
        }
        SvelteAwaitInlineBranch::Catch { binding, .. } => {
            out.push(3);
            push_optional_span_text(out, inventory, *binding);
        }
    }
}

fn push_standalone_family(
    out: &mut Vec<u8>,
    inventory: &CarrierBlockInventory,
    value: &SvelteStandaloneTagFamily,
) {
    match value {
        SvelteStandaloneTagFamily::Render => out.push(1),
        SvelteStandaloneTagFamily::Html => out.push(2),
        SvelteStandaloneTagFamily::LegacyConst => out.push(3),
        SvelteStandaloneTagFamily::Const => out.push(4),
        SvelteStandaloneTagFamily::Let => out.push(5),
        SvelteStandaloneTagFamily::Debug => out.push(6),
        SvelteStandaloneTagFamily::Attach => out.push(7),
        SvelteStandaloneTagFamily::Unknown {
            authored_name,
            reason,
        } => {
            out.push(8);
            push_slice(out, inventory, *authored_name);
            push_unknown_reason(out, *reason);
        }
    }
}
fn push_str(out: &mut Vec<u8>, value: &str) {
    push_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}
fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn push_optional_u32(out: &mut Vec<u8>, value: Option<u32>) {
    if let Some(value) = value {
        out.push(1);
        push_u32(out, value);
    } else {
        out.push(0);
    }
}
