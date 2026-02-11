/// Parse props from a CompiledElementStart into OxcProp and ElementScope vectors.
pub fn parse_element_props(
    mut compiled: CompiledElementStart,
    ctx: &SyntaxPluginContext<'alloc>,
) -> OxcCompiledElementStart<'alloc> {
    let mut oxc_props: Vec<OxcProp<'alloc>> = Vec::new();
    let mut scopes: Vec<ElementScope<'alloc>> = Vec::new();

    let element_id = compiled.element_id;
    let parent_id = compiled.parent_id;
    let is_template = compiled.event_open_tag.kind == ElementKind::Template;

    // Take props out to avoid partial move issues
    let props = std::mem::take(&mut compiled.props);

    for prop in props {
        match prop.kind {
            // Structural directives → extract into scopes
            PropKind::If => {
                let scope = self.parse_if_condition(&prop, element_id, ctx);
                scopes.push(ElementScope::If(scope));
            }
            PropKind::ElseIf => {
                let scope = self.parse_else_if_condition(&prop, element_id, ctx);
                scopes.push(ElementScope::ElseIf(scope));
            }
            PropKind::Else => {
                let scope = ElementScope::Else(OxcElseCondition {
                    element_id,
                    start: prop.start,
                    end: prop.end,
                    event: ElementScopeConditionElse {
                        element_start: element_id,
                        start: prop.start,
                        end: prop.end,
                    },
                });
                scopes.push(scope);
            }
            PropKind::For => {
                if let Some(scope) = self.parse_vfor(&prop, element_id, ctx) {
                    scopes.push(ElementScope::For(scope));
                }
            }
            PropKind::Slot => {
                if is_template {
                    if let Some(scope) = self.parse_vslot_template(&prop, element_id, ctx) {
                        scopes.push(ElementScope::SlotTemplate(scope));
                    }
                } else if let Some(scope) =
                    self.parse_vslot_element(&prop, element_id, &compiled.event_open_tag_end, ctx)
                {
                    scopes.push(ElementScope::SlotElement(scope));
                }
            }
            // Regular props → parse into OxcProp
            _ => {
                let oxc_prop = self.parse_prop(prop, element_id, parent_id, ctx);
                oxc_props.push(oxc_prop);
            }
        }
    }

    // Sort scopes by Vue priority: If/ElseIf/Else > For > Slot
    scopes.sort_by_key(|s| match s {
        ElementScope::If(_) | ElementScope::ElseIf(_) | ElementScope::Else(_) => 0,
        ElementScope::For(_) => 1,
        ElementScope::SlotElement(_) | ElementScope::SlotTemplate(_) => 2,
    });

    OxcCompiledElementStart {
        props: oxc_props,
        scopes,
        event: compiled,
    }
}

/// Parse a single prop's value and arg expressions.
pub fn parse_prop(
    prop: Prop,
    element_id: u32,
    parent_id: u32,
    ctx: &SyntaxPluginContext<'alloc>,
) -> OxcProp<'alloc> {
    let arg = if let Some(arg_span) = prop.arg {
        if prop.has_dynamic_arg {
            // Dynamic arg: :[key]="value" — parse the arg expression
            let (expression, errors, bindings) = self.parse_expression(arg_span, ctx);
            Some(OxcPropProcessed {
                start: arg_span.start,
                end: arg_span.end,
                expression,
                errors,
                bindings,
            })
        } else {
            // Static arg: :prop="value" — no parsing needed, just a span
            None
        }
    } else {
        None
    };

    let exp = if let Some(value_span) = prop.value {
        if prop.is_directive {
            // Directive value is an expression — parse it
            let (expression, errors, bindings) = self.parse_expression(value_span, ctx);
            Some(OxcPropProcessed {
                start: value_span.start,
                end: value_span.end,
                expression,
                errors,
                bindings,
            })
        } else {
            // Static attribute value — no parsing needed
            None
        }
    } else {
        None
    };

    OxcProp {
        element_id,
        parent_id,
        start: prop.start,
        name_end: prop.name_end,
        arg,
        exp,
        modifiers: prop.modifiers.clone(),
        event: prop,
    }
}
