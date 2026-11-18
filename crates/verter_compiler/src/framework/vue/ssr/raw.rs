//! `v-html` and `v-text`: the element's content is one expression.

use crate::ast::types::ElementNode;
use crate::template::oxc::types::OxcParsedElement;

use super::state::SsrEmitter;

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// Check if an element has v-html directive.
    pub(super) fn get_v_html_expr(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'c>>,
        source: &str,
    ) -> Option<String> {
        for (i, prop) in el.props.iter().enumerate() {
            let name = &source[prop.start as usize..prop.name_end as usize];
            if name == "v-html" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let oxc_prop = oxc.and_then(|o| o.prop(i));
                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                    return Some(self.resolve_expr(expr, vs, oxc_expr));
                }
            }
        }
        None
    }

    /// Check if an element has v-text directive.
    pub(super) fn get_v_text_expr(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'c>>,
        source: &str,
    ) -> Option<String> {
        for (i, prop) in el.props.iter().enumerate() {
            let name = &source[prop.start as usize..prop.name_end as usize];
            if name == "v-text" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let oxc_prop = oxc.and_then(|o| o.prop(i));
                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                    return Some(self.resolve_expr(expr, vs, oxc_expr));
                }
            }
        }
        None
    }
}
