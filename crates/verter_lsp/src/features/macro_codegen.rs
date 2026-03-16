// Builder for generating Vue macro call source code.
//
// Abstracts the text generation logic for defineProps, defineEmits, defineModel,
// defineSlots, etc. Used by macro_actions.rs and component_actions.rs.

use crate::features::action_utils::needs_quoting;

/// Builder for generating Vue macro call source code.
pub struct MacroCodegen {
    macro_name: &'static str,
    binding_name: Option<String>,
    type_members: Vec<TypeMember>,
    /// Named string argument for the macro call, e.g., `'title'` for `defineModel('title')`.
    named_arg: Option<String>,
}

/// A single member in a TypeScript type literal.
pub struct TypeMember {
    pub name: String,
    pub type_annotation: String,
    pub optional: bool,
}

impl MacroCodegen {
    /// Create a builder for `defineProps<{...}>()`.
    pub fn define_props() -> Self {
        Self {
            macro_name: "defineProps",
            binding_name: None,
            type_members: Vec::new(),
            named_arg: None,
        }
    }

    /// Create a builder for `defineEmits<{...}>()`.
    pub fn define_emits() -> Self {
        Self {
            macro_name: "defineEmits",
            binding_name: None,
            type_members: Vec::new(),
            named_arg: None,
        }
    }

    /// Create a builder for `defineModel<T>()` or `defineModel<T>('name')`.
    pub fn define_model(model_name: Option<&str>) -> Self {
        Self {
            macro_name: "defineModel",
            binding_name: None,
            type_members: Vec::new(),
            named_arg: model_name.map(|n| n.to_string()),
        }
    }

    /// Create a builder for `defineSlots<{...}>()`.
    pub fn define_slots() -> Self {
        Self {
            macro_name: "defineSlots",
            binding_name: None,
            type_members: Vec::new(),
            named_arg: None,
        }
    }

    /// Set the binding name: `const <name> = defineXxx<...>()`.
    pub fn with_binding(mut self, name: &str) -> Self {
        self.binding_name = Some(name.to_string());
        self
    }

    /// Add a type member to the type literal.
    pub fn add_type_member(mut self, name: &str, ty: &str, optional: bool) -> Self {
        self.type_members.push(TypeMember {
            name: name.to_string(),
            type_annotation: ty.to_string(),
            optional,
        });
        self
    }

    /// Generate the full macro call source text.
    ///
    /// Examples:
    /// - `defineProps<{\n  foo: string\n}>()\n`
    /// - `const props = defineProps<{\n  foo: string\n}>()\n`
    /// - `defineModel<unknown>('title')\n`
    pub fn build(&self) -> String {
        let mut out = String::new();

        // Optional binding: `const name = `
        if let Some(ref name) = self.binding_name {
            out.push_str("const ");
            out.push_str(name);
            out.push_str(" = ");
        }

        out.push_str(self.macro_name);

        if self.type_members.is_empty() {
            // No type members — use simple type parameter if this is defineModel
            if self.macro_name == "defineModel" {
                out.push_str("<unknown>");
            }
        } else {
            // Type literal: <{\n  member: type\n}>
            out.push_str("<{\n");
            for member in &self.type_members {
                out.push_str("  ");
                let name = if needs_quoting(&member.name) {
                    format!("'{}'", member.name)
                } else {
                    member.name.clone()
                };
                out.push_str(&name);
                if member.optional {
                    out.push('?');
                }
                out.push_str(": ");
                out.push_str(&member.type_annotation);
                out.push('\n');
            }
            out.push_str("}>");
        }

        // Arguments
        out.push('(');
        if let Some(ref arg) = self.named_arg {
            out.push('\'');
            out.push_str(arg);
            out.push('\'');
        }
        out.push_str(")\n");

        out
    }

    /// Generate text to insert into an existing macro's type literal.
    ///
    /// Produces only the member lines (indented), without the surrounding `<{` and `}>`.
    /// E.g., `  foo: string\n`
    pub fn build_member_insertion(&self) -> String {
        let mut out = String::new();
        for member in &self.type_members {
            out.push_str("  ");
            let name = if needs_quoting(&member.name) {
                format!("'{}'", member.name)
            } else {
                member.name.clone()
            };
            out.push_str(&name);
            if member.optional {
                out.push('?');
            }
            out.push_str(": ");
            out.push_str(&member.type_annotation);
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Full macro generation ───────────────────────────────────────────

    #[test]
    fn define_props_type_based_single_member() {
        let result = MacroCodegen::define_props()
            .add_type_member("msg", "string", false)
            .build();
        assert_eq!(result, "defineProps<{\n  msg: string\n}>()\n");
        // Negative: does NOT contain binding name or defineEmits
        assert!(!result.contains("const "));
        assert!(!result.contains("defineEmits"));
    }

    #[test]
    fn define_props_type_based_optional_member() {
        let result = MacroCodegen::define_props()
            .add_type_member("count", "number", true)
            .build();
        assert!(
            result.contains("count?: number"),
            "should have optional mark"
        );
        // Negative: does NOT contain "count: number" without ?
        assert!(!result.contains("count: number"));
    }

    #[test]
    fn define_props_with_binding_name() {
        let result = MacroCodegen::define_props()
            .with_binding("props")
            .add_type_member("msg", "string", false)
            .build();
        assert!(result.starts_with("const props = defineProps"));
        // Negative: does NOT omit "const props ="
        assert!(result.contains("const props ="));
    }

    #[test]
    fn define_props_multiple_members() {
        let result = MacroCodegen::define_props()
            .add_type_member("msg", "string", false)
            .add_type_member("count", "number", true)
            .build();
        assert!(result.contains("msg: string"));
        assert!(result.contains("count?: number"));
        // Both in same block
        let define_count = result.matches("defineProps").count();
        assert_eq!(define_count, 1);
    }

    #[test]
    fn define_emits_type_based() {
        let result = MacroCodegen::define_emits()
            .add_type_member("(e: 'save', ...args: any[]): void", "", false)
            .build();
        assert!(result.contains("defineEmits<{"));
        // Negative: does NOT contain defineProps
        assert!(!result.contains("defineProps"));
    }

    #[test]
    fn define_model_named() {
        let result = MacroCodegen::define_model(Some("title")).build();
        assert_eq!(result, "defineModel<unknown>('title')\n");
        // Negative: does NOT contain "modelValue"
        assert!(!result.contains("modelValue"));
    }

    #[test]
    fn define_model_default() {
        let result = MacroCodegen::define_model(None).build();
        assert_eq!(result, "defineModel<unknown>()\n");
        // Negative: does NOT contain a named argument
        assert!(!result.contains("'"));
    }

    #[test]
    fn define_model_with_type_and_name() {
        let result = MacroCodegen::define_model(Some("title"))
            .add_type_member("value", "string", false)
            .build();
        // When type members exist, uses type literal form
        assert!(result.contains("defineModel<{"));
        assert!(result.contains("'title'"));
    }

    #[test]
    fn define_slots_with_members() {
        let result = MacroCodegen::define_slots()
            .add_type_member("header", "(props: {}): any", false)
            .add_type_member("default", "(props: {}): any", false)
            .build();
        assert!(result.contains("defineSlots<{"));
        assert!(result.contains("header"));
        assert!(result.contains("default"));
        // Negative: does NOT contain defineProps
        assert!(!result.contains("defineProps"));
    }

    // ── Member insertion (into existing macro) ──────────────────────────

    #[test]
    fn member_insertion_single_prop() {
        let result = MacroCodegen::define_props()
            .add_type_member("foo", "string", false)
            .build_member_insertion();
        assert_eq!(result, "  foo: string\n");
        // Negative: does NOT contain "defineProps" or "{" or "}"
        assert!(!result.contains("defineProps"));
        assert!(!result.contains('{'));
        assert!(!result.contains('}'));
    }

    #[test]
    fn member_insertion_multiple_props() {
        let result = MacroCodegen::define_props()
            .add_type_member("foo", "string", false)
            .add_type_member("bar", "number", true)
            .build_member_insertion();
        assert_eq!(result, "  foo: string\n  bar?: number\n");
    }

    #[test]
    fn member_insertion_quoted_name() {
        let result = MacroCodegen::define_props()
            .add_type_member("nav-bar", "string", false)
            .build_member_insertion();
        assert!(result.contains("'nav-bar': string"));
        // Negative: does NOT contain unquoted hyphenated name
        assert!(!result.contains(" nav-bar:"));
    }
}
