//! `v-model` on form elements: server output renders the bound value.

use crate::ast::types::ElementNode;
use crate::template::oxc::types::OxcParsedElement;

use super::escape::escape_js_string;
use super::state::SsrEmitter;

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// Extract v-model or :value expression from an element (for textarea content injection).
    pub(super) fn get_v_model_expr(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'c>>,
        source: &str,
    ) -> Option<String> {
        let mut value_binding: Option<String> = None;
        for (i, prop) in el.props.iter().enumerate() {
            let name = &source[prop.start as usize..prop.name_end as usize];
            if name.starts_with("v-model") {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let oxc_prop = oxc.and_then(|o| o.prop(i));
                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                    return Some(self.resolve_expr(expr, vs, oxc_expr));
                }
            }
            // Also check for :value binding (rendered as content for textarea)
            if prop.is_directive {
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let attr_name = &source[as_ as usize..ae as usize];
                    if attr_name == "value" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| o.prop(i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            value_binding = Some(self.resolve_expr(expr, vs, oxc_expr));
                        }
                    }
                }
            }
        }
        value_binding
    }

    /// Detect the input type from element props (for v-model handling).
    pub(super) fn get_input_type<'s>(&self, el: &ElementNode, source: &'s str) -> Option<&'s str> {
        for prop in &el.props {
            if !prop.is_directive {
                let name = &source[prop.start as usize..prop.name_end as usize];
                if name == "type" {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        return Some(&source[vs as usize..ve as usize]);
                    }
                }
            }
        }
        None
    }

    /// Extract the `value` expression for an `<option>` element.
    ///
    /// - Static `value="X"` → `"X"` (string literal)
    /// - Dynamic `:value="expr"` → resolved expression
    /// - No value attribute → `""` (empty string)
    pub(super) fn get_option_value(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'c>>,
        source: &str,
    ) -> String {
        for (i, prop) in el.props.iter().enumerate() {
            let name = &source[prop.start as usize..prop.name_end as usize];
            if !prop.is_directive && name == "value" {
                // Static value="X"
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let val = &source[vs as usize..ve as usize];
                    return format!("\"{}\"", escape_js_string(val));
                }
                return "\"\"".to_string();
            }
            // Dynamic :value="expr" or v-bind:value="expr"
            if prop.is_directive && (name == ":" || name == "v-bind") {
                if let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) {
                    let arg = &source[arg_s as usize..arg_e as usize];
                    if arg == "value" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| o.prop(i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            return self.resolve_expr(expr, vs, oxc_expr);
                        }
                        return "\"\"".to_string();
                    }
                }
            }
        }
        // No value attribute
        "\"\"".to_string()
    }
}
