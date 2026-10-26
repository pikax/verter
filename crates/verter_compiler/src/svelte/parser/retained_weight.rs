//! Parser-owned retained-byte facts for [`super::ParsedSvelte`].
//!
//! The standalone compiler's inspectable carrier weight must ask the parse
//! result, not re-encode Svelte AST geometry at the compiler seam.

use super::options_custom_element::{
    AcceptedCustomElementValue, CustomElementDescriptor, CustomElementShadow,
};
use super::template_ast::{
    ParsedSvelte, SvelteAttribute, SvelteAttributeKind, SvelteBlock, SvelteBlockKind,
    SvelteDirective, SvelteElement, SvelteNode, SvelteScript, SvelteStyle, SvelteTag,
};

impl ParsedSvelte {
    /// Bytes this parse result retains independently of the source `&str`:
    /// collection buffers by capacity, owned strings, and recursively nested
    /// template nodes/attributes. Not RSS.
    pub fn retained_bytes(&self) -> usize {
        let mut n = std::mem::size_of::<Self>();
        n = n.saturating_add(option_script_bytes(self.instance_script.as_ref()));
        n = n.saturating_add(option_script_bytes(self.module_script.as_ref()));
        n = n.saturating_add(vec_cap_bytes(&self.styles));
        for style in &self.styles {
            n = n.saturating_add(style_bytes(style));
        }
        n = n.saturating_add(vec_cap_bytes(&self.template));
        for node in &self.template {
            n = n.saturating_add(node_bytes(node));
        }
        n = n.saturating_add(vec_cap_bytes(&self.diagnostics));
        for diagnostic in &self.diagnostics {
            n = n.saturating_add(diagnostic.message.capacity());
        }
        n = n.saturating_add(vec_cap_bytes(&self.close_tag_violations));
        for violation in &self.close_tag_violations {
            n = n.saturating_add(violation.tag.capacity());
        }
        n = n.saturating_add(vec_cap_bytes(&self.strict_parse_errors));
        n = n.saturating_add(vec_cap_bytes(&self.parse_reject_facts));
        n = n.saturating_add(vec_cap_bytes(&self.script_body_probes));
        n = n.saturating_add(vec_cap_bytes(&self.style_body_probes));
        n = n.saturating_add(vec_cap_bytes(&self.options_custom_element_probes));
        for probe in &self.options_custom_element_probes {
            if let Ok(AcceptedCustomElementValue::Descriptor(descriptor)) = &probe.resolution {
                n = n.saturating_add(descriptor_bytes(descriptor));
            }
        }
        n = n.saturating_add(vec_cap_bytes(&self.options_custom_element_text_tags));
        for tag in &self.options_custom_element_text_tags {
            n = n.saturating_add(descriptor_bytes(&tag.descriptor));
        }
        n
    }
}

fn descriptor_bytes(descriptor: &CustomElementDescriptor) -> usize {
    let mut n = string_bytes(descriptor.tag.as_ref());
    n = n.saturating_add(string_bytes(descriptor.extend.as_ref()));
    if let CustomElementShadow::ObjectInit(init) = &descriptor.shadow {
        n = n.saturating_add(init.capacity());
    }
    n = n.saturating_add(vec_cap_bytes(&descriptor.props));
    for prop in &descriptor.props {
        n = n.saturating_add(prop.name.capacity());
        n = n.saturating_add(string_bytes(prop.attribute.as_ref()));
        n = n.saturating_add(string_bytes(prop.type_hint.as_ref()));
    }
    n
}

fn vec_cap_bytes<T>(items: &Vec<T>) -> usize {
    items.capacity().saturating_mul(std::mem::size_of::<T>())
}

fn option_script_bytes(script: Option<&SvelteScript>) -> usize {
    script.map(script_bytes).unwrap_or(0)
}

fn script_bytes(script: &SvelteScript) -> usize {
    let mut n = vec_cap_bytes(&script.attributes);
    n = n.saturating_add(string_bytes(script.lang.as_ref()));
    for attr in &script.attributes {
        n = n.saturating_add(attribute_bytes(attr));
    }
    n
}

fn style_bytes(style: &SvelteStyle) -> usize {
    let mut n = vec_cap_bytes(&style.attributes);
    n = n.saturating_add(string_bytes(style.lang.as_ref()));
    for attr in &style.attributes {
        n = n.saturating_add(attribute_bytes(attr));
    }
    n
}

fn string_bytes(s: Option<&String>) -> usize {
    s.map(String::capacity).unwrap_or(0)
}

fn node_bytes(node: &SvelteNode) -> usize {
    match node {
        SvelteNode::Text(_) | SvelteNode::Comment(_) | SvelteNode::Interpolation(_) => 0,
        SvelteNode::Element(element) => element_bytes(element),
        SvelteNode::Block(block) => block_bytes(block),
        SvelteNode::Tag(tag) => tag_bytes(tag),
    }
}

fn element_bytes(element: &SvelteElement) -> usize {
    let mut n = element.name.capacity();
    n = n.saturating_add(vec_cap_bytes(&element.attributes));
    for attr in &element.attributes {
        n = n.saturating_add(attribute_bytes(attr));
    }
    n = n.saturating_add(vec_cap_bytes(&element.children));
    for child in &element.children {
        n = n.saturating_add(node_bytes(child));
    }
    n
}

fn block_bytes(block: &SvelteBlock) -> usize {
    let mut n = vec_cap_bytes(&block.children);
    for child in &block.children {
        n = n.saturating_add(node_bytes(child));
    }
    n = n.saturating_add(vec_cap_bytes(&block.clauses));
    for clause in &block.clauses {
        n = n.saturating_add(vec_cap_bytes(&clause.children));
        for child in &clause.children {
            n = n.saturating_add(node_bytes(child));
        }
    }
    if let SvelteBlockKind::Snippet { name_text, .. } = &block.kind {
        n = n.saturating_add(name_text.capacity());
    }
    n
}

fn tag_bytes(_tag: &SvelteTag) -> usize {
    0
}

fn attribute_bytes(attr: &SvelteAttribute) -> usize {
    let mut n = vec_cap_bytes(&attr.mixed_parts);
    n = n.saturating_add(match &attr.kind {
        SvelteAttributeKind::Plain { name, .. } => name.capacity(),
        SvelteAttributeKind::Spread(_) | SvelteAttributeKind::Attach { .. } => 0,
        SvelteAttributeKind::Directive(directive) => directive_bytes(directive),
    });
    n
}

fn directive_bytes(directive: &SvelteDirective) -> usize {
    let mut n = directive.prefix.capacity();
    n = n.saturating_add(directive.local.capacity());
    n = n.saturating_add(vec_cap_bytes(&directive.modifiers));
    for modifier in &directive.modifiers {
        n = n.saturating_add(modifier.capacity());
    }
    n = n.saturating_add(vec_cap_bytes(&directive.modifier_spans));
    n
}
