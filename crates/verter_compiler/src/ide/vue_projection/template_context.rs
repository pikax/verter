//! The template context view: how template expressions read setup bindings,
//! props and public instance members.

use verter_parser::types::BindingType;

use crate::ide::vue_projection::checking_fragment::{CheckingFragment, PreludeDeclaration};
use crate::ide::vue_projection::public_constructor::{
    VuePublicConstructorContract, PUBLIC_INSTANCE, PUBLIC_PROPS, TEMPLATE_INSTANCE, TEMPLATE_PROPS,
};
use crate::ide::vue_projection::script_setup::{ScriptProjectionFacts, SetupStatementKind};

/// Binding every qualified template identifier is read through.
pub const TEMPLATE_CONTEXT: &str = "__VerterCtx";
/// Declarations the context statement reads.
pub const TEMPLATE_CONTEXT_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "10-template-context",
    text: concat!(
        "declare function __VerterTemplateContext<B, I>(bindings: B, instance: I): ",
        "import(\"vue\").ShallowUnwrapRef<B> & Omit<I, keyof B>;\n",
    ),
};

/// One script binding the template context lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBinding {
    pub name: String,
    /// Read through an accessor pair rather than a shorthand property: the
    /// binding is reassignable (`let` or `var`), so a read in the template
    /// must not count as a read before a later assignment.
    pub getter: bool,
}

/// The one context statement of a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateContextView {
    /// Script value bindings the template references, in authored order.
    /// Only referenced names are listed, so an unused setup binding stays
    /// reported by `noUnusedLocals`.
    pub bindings: Vec<ContextBinding>,
    /// Type text of the instance half (props, `$attrs`, `$slots`, `$emit`,
    /// global properties).
    pub instance_type: String,
}

impl TemplateContextView {
    /// `const __VerterCtx = __VerterTemplateContext({ a, get b() { return b; }, set b(v) { b = v; } }, null! as <instance>);`
    /// A `const` binding is listed by name. A `let` or `var` binding is an
    /// accessor pair, so a binding assigned later is not read before
    /// assignment and stays assignable from the template.
    #[must_use]
    pub fn statement(&self) -> CheckingFragment {
        CheckingFragment::synth(format!(
            "const {TEMPLATE_CONTEXT} = {};\n",
            self.call_text()
        ))
    }

    /// The context call alone, as a statement: keeps the listed bindings
    /// read when no emitted statement reads the context.
    #[must_use]
    pub fn unread_statement(&self) -> CheckingFragment {
        CheckingFragment::synth(format!("{};\n", self.call_text()))
    }

    fn call_text(&self) -> String {
        let members: Vec<String> = self
            .bindings
            .iter()
            .map(|binding| {
                let name = &binding.name;
                if binding.getter {
                    format!("get {name}() {{ return {name}; }}, set {name}(v) {{ {name} = v; }}")
                } else {
                    name.clone()
                }
            })
            .collect();
        let object = if members.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {} }}", members.join(", "))
        };
        format!(
            "__VerterTemplateContext({object}, null! as {})",
            self.instance_type
        )
    }
}

/// Build the context view for one carrier. `referenced` holds the names the
/// template's context references spell, in template order; `kinds` holds the
/// setup bindings with the parser's classification of each.
///
/// A referenced name is listed when the setup scope or the normal script
/// binds it; every other name is read from the instance half, which holds
/// the props and the public instance members. When `reads_setup_return`,
/// the public props read the setup scope's return, which is inferred from
/// this context, so the instance half is the template aliases instead.
#[must_use]
pub fn project_template_context(
    script: &ScriptProjectionFacts,
    public: &VuePublicConstructorContract,
    referenced: &[String],
    kinds: &[(String, BindingType)],
    reads_setup_return: bool,
) -> TemplateContextView {
    let setup_names = script.setup.iter().flat_map(|setup| {
        setup
            .statements
            .iter()
            .flat_map(|statement| match &statement.kind {
                SetupStatementKind::Import { names }
                | SetupStatementKind::Declaration { names } => names.as_slice(),
                SetupStatementKind::TypeDeclaration { .. } | SetupStatementKind::Other => &[],
            })
    });
    let mut bindings: Vec<ContextBinding> = Vec::new();
    for name in script
        .module
        .normal_script_bindings
        .iter()
        .chain(setup_names)
    {
        if !referenced.contains(name) {
            continue;
        }
        let getter = kinds
            .iter()
            .any(|(bound, kind)| bound == name && *kind == BindingType::SetupLet);
        // A setup binding shadows a normal-script one of the same name.
        if let Some(listed) = bindings.iter_mut().find(|listed| listed.name == *name) {
            listed.getter = getter;
            continue;
        }
        bindings.push(ContextBinding {
            name: name.clone(),
            getter,
        });
    }
    let args = if public.binder.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = public.binder.iter().map(|p| p.name.as_str()).collect();
        format!("<{}>", names.join(", "))
    };
    TemplateContextView {
        bindings,
        instance_type: if reads_setup_return {
            format!("{TEMPLATE_INSTANCE}{args} & {TEMPLATE_PROPS}{args}")
        } else {
            format!("{PUBLIC_INSTANCE}{args} & {PUBLIC_PROPS}{args}")
        },
    }
}
