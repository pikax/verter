//! Fine-grained render contracts pinned to the native Vue Vapor compiler.
//!
//! Each synthetic component isolates one contract of the generated render
//! function. Every module must be valid ESM whose names all resolve where the
//! native output's do; each case then compares the construct it isolates with
//! the native golden: loop key callbacks, component asset declarations,
//! component event handler values, the DOM shape of the static templates
//! that fix insertion anchors and block roots, component slot signatures and
//! template ref setters. Every case also compares the DOM node each runtime
//! operation receives, under the pinned runtime's navigation semantics.
use std::collections::{BTreeMap, BTreeSet, HashMap};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, Expression, ImportDeclarationSpecifier, ObjectPropertyKind, Statement,
};
use oxc_ast::AstKind;
use oxc_semantic::{Semantic, SemanticBuilder, SymbolId};
use oxc_span::{GetSpan, SourceType};
use verter_parser::oxc_parse::{with_program_stack, Parser};
use verter_vue_conformance::canon::{canonicalize_module, Canon};
use verter_vue_conformance::{read_text_normalized, sha256_hex, workspace_root};

use super::*;

/// One argument of a runtime helper call.
#[derive(Debug, Clone)]
struct Argument {
    text: String,
    /// The cooked value of a string literal argument.
    string: Option<String>,
}

/// Facts read from one generated module.
#[derive(Debug, Default)]
struct ModuleFacts {
    /// Names read without a declaration in scope.
    unresolved: BTreeSet<String>,
    /// Calls to `vue` runtime helpers in source order: imported name and arguments.
    calls: Vec<(String, Vec<Argument>)>,
    /// `const`/`let` initializers by declared name.
    initializers: BTreeMap<String, String>,
    /// Event props of component creations, in source order: key and getter result.
    events: Vec<(String, String)>,
    /// Each component creation's prop keys and slot signatures.
    components: Vec<ComponentFacts>,
}

/// The props and slots one component creation passes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ComponentFacts {
    /// Static prop keys, sorted.
    props: Vec<String>,
    /// Static slot keys, sorted, with the number of parameters each slot
    /// function declares. The dynamic slot list is keyed `$` with the kind of
    /// each element in order, and its length.
    slots: Vec<(String, usize)>,
}

const COMPONENT_CREATIONS: [&str; 4] = [
    "createComponent",
    "createComponentWithFallback",
    "createAssetComponent",
    "createDynamicComponent",
];

fn getter_result<'a>(arrow: &'a ArrowFunctionExpression<'a>) -> Option<&'a Expression<'a>> {
    if !arrow.params.items.is_empty() {
        return None;
    }
    arrow.get_expression()
}

fn facts(code: &str) -> Result<ModuleFacts, String> {
    let allocator = Allocator::new();
    let parse = Parser::new(&allocator, code, SourceType::mjs()).parse();
    if parse.fatal_error || !parse.diagnostics.is_empty() {
        return Err(format!("module does not parse: {:?}", parse.diagnostics));
    }
    let program = parse.program;
    let built = with_program_stack(&program, || {
        SemanticBuilder::new()
            .with_build_nodes(true)
            .build(&program)
    });
    if !built.diagnostics.is_empty() {
        return Err(format!(
            "module fails semantic analysis: {}",
            built
                .diagnostics
                .iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    let semantic = built.semantic;
    let text = |span: oxc_span::Span| code[span.start as usize..span.end as usize].to_string();
    let mut helpers = BTreeMap::new();
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else {
            continue;
        };
        if import.source.value != "vue" {
            continue;
        }
        for specifier in import.specifiers.iter().flatten() {
            if let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier {
                helpers.insert(
                    specifier.local.name.to_string(),
                    specifier.imported.name().to_string(),
                );
            }
        }
    }
    // The render function is the compared surface: a script around it may
    // read globals the template-only native module does not.
    let render = program
        .body
        .iter()
        .find_map(|statement| {
            let function = match statement {
                Statement::FunctionDeclaration(function) => function,
                Statement::ExportDeclaration(export) => match &export.declaration {
                    oxc_ast::ast::Declaration::FunctionDeclaration(function) => function,
                    _ => return None,
                },
                _ => return None,
            };
            (function.id.as_ref()?.name == "render").then_some(function.span)
        })
        .ok_or("module declares no render function")?;
    let mut facts = ModuleFacts::default();
    for node in semantic.nodes().iter() {
        match node.kind() {
            AstKind::IdentifierReference(identifier) => {
                let inside =
                    identifier.span.start >= render.start && identifier.span.end <= render.end;
                if inside && semantic.is_unresolved_reference(node.id()) {
                    facts.unresolved.insert(identifier.name.to_string());
                }
            }
            AstKind::VariableDeclarator(declarator) => {
                if let (Some(name), Some(init)) =
                    (declarator.id.get_identifier_name(), &declarator.init)
                {
                    facts
                        .initializers
                        .insert(name.to_string(), text(init.span()));
                }
            }
            AstKind::CallExpression(call) => {
                let Expression::Identifier(callee) = &call.callee else {
                    continue;
                };
                // Vue declares the dynamic template ref setter as a local.
                let setter = facts
                    .initializers
                    .get(callee.name.as_str())
                    .and_then(|init| init.strip_suffix("()"))
                    .and_then(|init| helpers.get(init))
                    .filter(|helper| *helper == "createTemplateRefSetter")
                    .map(|_| "setTemplateRef".to_string());
                let Some(helper) = helpers.get(callee.name.as_str()).cloned().or(setter) else {
                    continue;
                };
                let arguments = call
                    .arguments
                    .iter()
                    .map(|argument| Argument {
                        text: text(argument.span()),
                        string: match argument.as_expression() {
                            Some(Expression::StringLiteral(literal)) => {
                                Some(literal.value.to_string())
                            }
                            _ => None,
                        },
                    })
                    .collect();
                facts.calls.push((helper.clone(), arguments));
                if !COMPONENT_CREATIONS.contains(&helper.as_str()) {
                    continue;
                }
                facts.components.push(component_facts(call));
                let Some(Expression::ObjectExpression(props)) = call
                    .arguments
                    .get(1)
                    .and_then(|argument| argument.as_expression())
                else {
                    continue;
                };
                for property in &props.properties {
                    let ObjectPropertyKind::ObjectProperty(property) = property else {
                        continue;
                    };
                    let Some(key) = property.key.static_name() else {
                        continue;
                    };
                    let is_event = key.strip_prefix("on").is_some_and(|rest| {
                        rest.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    });
                    if !is_event {
                        continue;
                    }
                    let Expression::ArrowFunctionExpression(getter) = &property.value else {
                        facts
                            .events
                            .push((key.to_string(), text(property.value.span())));
                        continue;
                    };
                    let result = getter_result(getter)
                        .map_or_else(|| text(getter.span()), |result| text(result.span()));
                    facts.events.push((key.to_string(), result));
                }
            }
            _ => {}
        }
    }
    Ok(facts)
}

/// A slot function's declared parameter count.
fn slot_parameters(value: &Expression<'_>) -> Option<usize> {
    match value {
        Expression::ArrowFunctionExpression(arrow) => Some(arrow.params.items.len()),
        Expression::FunctionExpression(function) => Some(function.params.items.len()),
        Expression::ParenthesizedExpression(inner) => slot_parameters(&inner.expression),
        // `_extend(fn, flags)` marks the slot function; its flags are compared apart.
        Expression::CallExpression(call) => {
            slot_parameters(call.arguments.first()?.as_expression()?)
        }
        _ => None,
    }
}

/// What one element of a dynamic slot list is: a static slots object, a
/// named getter, a conditional getter, or a looped slot list.
fn dynamic_slot_kind(element: Option<&Expression<'_>>) -> &'static str {
    let mut element = element;
    while let Some(Expression::ParenthesizedExpression(inner)) = element {
        element = Some(&inner.expression);
    }
    match element {
        Some(Expression::ObjectExpression(_)) => "static",
        Some(Expression::CallExpression(_)) => "loop",
        Some(Expression::ArrowFunctionExpression(arrow)) => {
            let mut body = arrow.get_expression();
            while let Some(Expression::ParenthesizedExpression(inner)) = body {
                body = Some(&inner.expression);
            }
            match body {
                Some(Expression::ObjectExpression(_)) => "named",
                Some(Expression::ConditionalExpression(_)) => "conditional",
                _ => "getter",
            }
        }
        _ => "other",
    }
}

fn component_facts(call: &oxc_ast::ast::CallExpression<'_>) -> ComponentFacts {
    let argument = |index: usize| {
        call.arguments
            .get(index)
            .and_then(|argument| argument.as_expression())
    };
    let key_of = |property: &oxc_ast::ast::ObjectProperty<'_>| {
        property
            .key
            .static_name()
            .map_or_else(|| "[computed]".to_string(), |name| name.to_string())
    };
    let mut props = Vec::new();
    if let Some(Expression::ObjectExpression(object)) = argument(1) {
        for property in &object.properties {
            if let ObjectPropertyKind::ObjectProperty(property) = property {
                props.push(key_of(property));
            }
        }
    }
    props.sort();
    let mut slots = Vec::new();
    match argument(2) {
        Some(Expression::ObjectExpression(object)) => {
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    continue;
                };
                let key = key_of(property);
                match (&property.value, key.as_str()) {
                    (_, "_") => {}
                    (Expression::ArrayExpression(list), "$") => {
                        let kinds: Vec<_> = list
                            .elements
                            .iter()
                            .map(|element| dynamic_slot_kind(element.as_expression()))
                            .collect();
                        slots.push((format!("$[{}]", kinds.join(", ")), list.elements.len()));
                    }
                    (value, _) => {
                        slots.push((key, slot_parameters(value).unwrap_or(usize::MAX)));
                    }
                }
            }
        }
        Some(value) => {
            if let Some(count) = slot_parameters(value) {
                slots.push(("default".to_string(), count));
            }
        }
        None => {}
    }
    slots.sort();
    ComponentFacts { props, slots }
}

/// Every runtime operation with the DOM nodes it receives, as structural
/// paths under the pinned runtime-vapor 3.6.0-rc.15 `dom/node.ts`:
/// `child(n)` and `txt(n)` are `n.firstChild`, `next(n)` is `n.nextSibling`
/// (its second argument is only a hydration text flag), `nthChild(n, i)` is
/// `n.childNodes[i]`, and a template factory call is its payload's first root.
fn node_operations(code: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let allocator = Allocator::new();
    let parse = Parser::new(&allocator, code, SourceType::mjs()).parse();
    if parse.fatal_error || !parse.diagnostics.is_empty() {
        return Err(format!("module does not parse: {:?}", parse.diagnostics));
    }
    let program = parse.program;
    let built = with_program_stack(&program, || {
        SemanticBuilder::new()
            .with_build_nodes(true)
            .build(&program)
    });
    let semantic = built.semantic;
    let mut helpers = BTreeMap::new();
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else {
            continue;
        };
        for specifier in import.specifiers.iter().flatten() {
            if let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier {
                helpers.insert(
                    specifier.local.name.to_string(),
                    specifier.imported.name().to_string(),
                );
            }
        }
    }
    let mut paths = NodePaths {
        semantic: &semantic,
        helpers: &helpers,
        templates: HashMap::new(),
        values: HashMap::new(),
    };
    let mut operations = Vec::new();
    for node in semantic.nodes().iter() {
        match node.kind() {
            AstKind::VariableDeclarator(declarator) => {
                let (Some(id), Some(init)) =
                    (declarator.id.get_binding_identifier(), &declarator.init)
                else {
                    continue;
                };
                if let Some(payload) = paths.template_payload(init) {
                    paths
                        .templates
                        .insert(id.symbol_id(), format!("T({})", template_shape(&payload)));
                } else if let Some(path) = paths.eval(init) {
                    paths.values.insert(id.symbol_id(), path);
                }
            }
            AstKind::ExpressionStatement(statement) => {
                if matches!(statement.expression, Expression::AssignmentExpression(_)) {
                    paths.eval(&statement.expression);
                }
            }
            AstKind::CallExpression(call) => {
                let Expression::Identifier(callee) = &call.callee else {
                    continue;
                };
                let helper = helpers
                    .get(callee.name.as_str())
                    .map_or(callee.name.as_str(), String::as_str);
                if NAVIGATION_HELPERS.contains(&helper) {
                    continue;
                }
                let nodes: Vec<String> = call
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument.as_expression()? {
                        Expression::Identifier(identifier) => paths.read(identifier),
                        _ => None,
                    })
                    .collect();
                if !nodes.is_empty() {
                    operations.push((helper.to_string(), nodes));
                }
            }
            _ => {}
        }
    }
    operations.sort();
    Ok(operations)
}

const NAVIGATION_HELPERS: [&str; 5] = ["child", "next", "nthChild", "txt", "template"];

struct NodePaths<'s, 'a> {
    semantic: &'s Semantic<'a>,
    helpers: &'s BTreeMap<String, String>,
    templates: HashMap<SymbolId, String>,
    values: HashMap<SymbolId, String>,
}

impl NodePaths<'_, '_> {
    fn symbol(&self, identifier: &oxc_ast::ast::IdentifierReference<'_>) -> Option<SymbolId> {
        self.semantic
            .scoping()
            .get_reference(identifier.reference_id())
            .symbol_id()
    }

    fn read(&self, identifier: &oxc_ast::ast::IdentifierReference<'_>) -> Option<String> {
        self.values.get(&self.symbol(identifier)?).cloned()
    }

    fn template_payload(&self, init: &Expression<'_>) -> Option<String> {
        let Expression::CallExpression(call) = init else {
            return None;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        if self.helpers.get(callee.name.as_str())? != "template" {
            return None;
        }
        match call.arguments.first()?.as_expression()? {
            Expression::StringLiteral(literal) => Some(literal.value.to_string()),
            _ => None,
        }
    }

    /// The node an expression evaluates to; an assignment updates its local.
    fn eval(&mut self, expression: &Expression<'_>) -> Option<String> {
        match expression {
            Expression::ParenthesizedExpression(inner) => self.eval(&inner.expression),
            Expression::Identifier(identifier) => self.read(identifier),
            Expression::AssignmentExpression(assignment) => {
                let value = self.eval(&assignment.right);
                if let oxc_ast::ast::AssignmentTarget::AssignmentTargetIdentifier(target) =
                    &assignment.left
                {
                    if let (Some(symbol), Some(value)) = (self.symbol(target), &value) {
                        self.values.insert(symbol, value.clone());
                    }
                }
                value
            }
            Expression::CallExpression(call) => {
                let Expression::Identifier(callee) = &call.callee else {
                    return None;
                };
                if let Some(root) = self
                    .symbol(callee)
                    .and_then(|symbol| self.templates.get(&symbol))
                {
                    return Some(root.clone());
                }
                let helper = self.helpers.get(callee.name.as_str())?.clone();
                let base = self.eval(call.arguments.first()?.as_expression()?)?;
                match helper.as_str() {
                    "child" | "txt" => Some(format!("{base}/0")),
                    "nthChild" => match call.arguments.get(1)?.as_expression()? {
                        Expression::NumericLiteral(index) => {
                            Some(format!("{base}/{}", index.value))
                        }
                        _ => None,
                    },
                    "next" => Some(match base.rsplit_once('/') {
                        Some((parent, last)) if last.parse::<u32>().is_ok() => {
                            format!("{parent}/{}", last.parse::<u32>().unwrap() + 1)
                        }
                        _ => format!("{base}+1"),
                    }),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

impl ModuleFacts {
    fn arguments(&self, helper: &str, index: usize) -> Vec<&Argument> {
        self.calls
            .iter()
            .filter(|(name, _)| name == helper)
            .filter_map(|(_, arguments)| arguments.get(index))
            .collect()
    }

    /// Each event prop's handler: a getter returning a hoisted local yields
    /// that local's initializer.
    fn handlers(&self) -> Vec<(String, Canon)> {
        self.events
            .iter()
            .map(|(key, result)| {
                let handler = self.initializers.get(result.trim()).unwrap_or(result);
                (key.clone(), snippet(handler))
            })
            .collect()
    }
}

/// One expression canonicalized on its own: names outside it stay exact.
fn snippet(expression: &str) -> Canon {
    canonicalize_module(&format!("({expression});\n"), &BTreeSet::new())
        .unwrap_or_else(|error| panic!("{expression}: {error}"))
        .tree
}

/// The node forest a template payload parses to: elements by tag with their
/// children, comments, insertion placeholders and text. Attributes and
/// omitted end tags do not change the forest.
fn template_shape(html: &str) -> String {
    const VOID: [&str; 14] = [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ];
    let mut stack: Vec<(String, Vec<String>)> = vec![(String::new(), Vec::new())];
    let close = |stack: &mut Vec<(String, Vec<String>)>| {
        let (tag, children) = stack.pop().expect("open element");
        stack
            .last_mut()
            .expect("forest root")
            .1
            .push(format!("{tag}({})", children.join(",")));
    };
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<!--") {
            let end = after.find("-->").expect("closed comment");
            stack.last_mut().unwrap().1.push("C".to_string());
            rest = &after[end + 3..];
        } else if let Some(after) = rest.strip_prefix("<!>") {
            stack.last_mut().unwrap().1.push("A".to_string());
            rest = after;
        } else if let Some(after) = rest.strip_prefix("</") {
            let end = after.find('>').expect("closed end tag");
            let tag = after[..end].trim().to_ascii_lowercase();
            if stack.iter().skip(1).any(|(open, _)| *open == tag) {
                while stack.last().is_some_and(|(open, _)| *open != tag) {
                    close(&mut stack);
                }
                close(&mut stack);
            }
            rest = &after[end + 1..];
        } else if let Some(after) = rest.strip_prefix('<') {
            let end = after.find('>').expect("closed start tag");
            let tag = after[..end]
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if VOID.contains(&tag.as_str()) {
                stack.last_mut().unwrap().1.push(format!("{tag}()"));
            } else {
                stack.push((tag, Vec::new()));
            }
            rest = &after[end + 1..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            stack.last_mut().unwrap().1.push("T".to_string());
            rest = &rest[end..];
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().unwrap().1.join(",")
}

fn template_shapes(facts: &ModuleFacts) -> BTreeSet<String> {
    facts
        .arguments("template", 0)
        .into_iter()
        .map(|argument| template_shape(argument.string.as_deref().expect("literal payload")))
        .collect()
}

/// The construct each case isolates, beyond valid ESM and resolved names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Contract {
    /// Generated element, pointer and text locals are declared where used.
    GeneratedLocals,
    /// Loop key callbacks read setup bindings through the context and keep
    /// their own aliases.
    LoopKeys,
    /// Each resolved component asset is declared once.
    ComponentAssets,
    /// Component event props return their handler instead of running it.
    ComponentEvents,
    /// Static templates place insertion anchors and block roots as Vue does.
    Templates,
    /// Component creations pass the props Vue passes, each slot function
    /// declares its slot props parameter, and dynamic slot names stay dynamic.
    ComponentSlots,
    /// Template refs go through the setter Vue calls, with its arguments.
    TemplateRefs,
    /// Slot functions whose roots may render nothing are marked NON_STABLE,
    /// slot templates beside default content leave it its own roots, and the
    /// slots, dynamic slot order and refs are Vue's.
    SlotRoots,
    /// Dynamic attribute, class and style setters pass Vue's arguments,
    /// including the trailing SVG flag.
    Setters,
    /// Each static template is parsed in the namespace Vue declares for it.
    TemplateNamespaces,
}

const CASES: [(&str, Contract); 21] = [
    ("generated-local-container", Contract::GeneratedLocals),
    ("setup-read-loop-key", Contract::LoopKeys),
    ("component-asset-repeat", Contract::ComponentAssets),
    ("component-event-statement", Contract::ComponentEvents),
    ("condition-anchor", Contract::Templates),
    ("multiple-root-condition", Contract::Templates),
    ("indexed-navigation", Contract::GeneratedLocals),
    ("static-template-ref", Contract::TemplateRefs),
    ("component-slot-props", Contract::ComponentSlots),
    ("slot-fallback-locals", Contract::Templates),
    ("dynamic-slot-name", Contract::ComponentSlots),
    ("slot-roots", Contract::SlotRoots),
    ("empty-element-siblings", Contract::Templates),
    ("trailing-text-run", Contract::Templates),
    ("text-runs-beside-elements", Contract::Templates),
    ("multi-root-slot-body", Contract::Templates),
    ("svg-attributes", Contract::Setters),
    ("template-namespace", Contract::TemplateNamespaces),
    ("insertion-anchor-whitespace", Contract::Templates),
    ("forced-multi-root", Contract::Templates),
    ("conditional-slot-template", Contract::ComponentSlots),
];

/// Each component creation's props and slots, in a stable order.
fn components(facts: &ModuleFacts) -> Vec<ComponentFacts> {
    let mut components = facts.components.clone();
    components.sort();
    components
}

fn component_slots_difference(ours: &ModuleFacts, vue: &ModuleFacts) -> Option<String> {
    let for_slots = |facts: &ModuleFacts| facts.arguments("createForSlots", 0).len();
    if components(ours) != components(vue) {
        Some(format!(
            "components {:?} against {:?}",
            components(ours),
            components(vue)
        ))
    } else if for_slots(ours) != for_slots(vue) {
        Some("looped dynamic slots differ".to_string())
    } else {
        None
    }
}

/// Setter calls with every argument after the node, in a stable order.
fn setters(facts: &ModuleFacts) -> Vec<(String, Vec<String>)> {
    const SETTERS: [&str; 6] = [
        "setAttr",
        "setProp",
        "setClass",
        "setStyle",
        "setDOMProp",
        "setValue",
    ];
    let mut calls: Vec<_> = facts
        .calls
        .iter()
        .filter(|(helper, _)| SETTERS.contains(&helper.as_str()))
        .map(|(helper, arguments)| {
            let rest: Vec<_> = arguments
                .iter()
                .skip(1)
                .map(|argument| argument.text.clone())
                .collect();
            (helper.clone(), rest)
        })
        .collect();
    calls.sort();
    calls
}

/// Each static template's node forest with its namespace argument, sorted.
fn template_namespaces(facts: &ModuleFacts) -> Vec<(String, String)> {
    let mut templates: Vec<_> = facts
        .calls
        .iter()
        .filter(|(helper, _)| helper == "template")
        .map(|(_, arguments)| {
            let payload = arguments[0].string.as_deref().expect("literal payload");
            let namespace = arguments
                .get(2)
                .map_or_else(String::new, |argument| argument.text.clone());
            (template_shape(payload), namespace)
        })
        .collect();
    templates.sort();
    templates
}

/// Template ref calls with every argument after the node.
fn template_refs(facts: &ModuleFacts) -> Vec<(String, Vec<String>)> {
    let mut refs: Vec<_> = facts
        .calls
        .iter()
        .filter(|(helper, _)| {
            matches!(
                helper.as_str(),
                "setStaticTemplateRef" | "setTemplateRef" | "setTemplateRefBinding"
            )
        })
        .map(|(helper, arguments)| {
            let rest: Vec<_> = arguments
                .iter()
                .skip(1)
                .map(|argument| argument.text.clone())
                .collect();
            (helper.clone(), rest)
        })
        .collect();
    refs.sort();
    refs
}

fn contract_difference(
    contract: Contract,
    ours: &ModuleFacts,
    vue: &ModuleFacts,
) -> Option<String> {
    match contract {
        Contract::GeneratedLocals => None,
        Contract::LoopKeys => {
            let keys = |facts: &ModuleFacts| {
                facts
                    .arguments("createFor", 2)
                    .into_iter()
                    .map(|argument| snippet(&argument.text))
                    .collect::<Vec<_>>()
            };
            assert_eq!(keys(vue).len(), 3, "oracle loop key inventory");
            (keys(ours) != keys(vue)).then(|| "loop key callbacks differ".to_string())
        }
        Contract::ComponentAssets => {
            let assets = |facts: &ModuleFacts| {
                facts
                    .arguments("resolveComponent", 0)
                    .into_iter()
                    .map(|argument| argument.string.clone().expect("asset name"))
                    .collect::<Vec<_>>()
            };
            assert_eq!(assets(vue), ["Badge"], "oracle asset inventory");
            (assets(ours) != assets(vue)).then(|| format!("assets {:?}", assets(ours)))
        }
        Contract::ComponentEvents => {
            assert_eq!(vue.handlers().len(), 9, "oracle event inventory");
            (ours.handlers() != vue.handlers()).then(|| "event handler values differ".to_string())
        }
        Contract::Templates => {
            let flags = |facts: &ModuleFacts| {
                facts
                    .arguments("createIf", 3)
                    .into_iter()
                    .map(|argument| argument.text.clone())
                    .collect::<Vec<_>>()
            };
            let ours_shapes = template_shapes(ours);
            let vue_shapes = template_shapes(vue);
            if ours_shapes != vue_shapes {
                Some(format!("templates {ours_shapes:?} against {vue_shapes:?}"))
            } else if flags(ours) != flags(vue) {
                Some(format!(
                    "conditional flags {:?} against {:?}",
                    flags(ours),
                    flags(vue)
                ))
            } else {
                None
            }
        }
        Contract::ComponentSlots => component_slots_difference(ours, vue),
        Contract::TemplateRefs => {
            assert_eq!(template_refs(vue).len(), 3, "oracle template ref inventory");
            (template_refs(ours) != template_refs(vue)).then(|| {
                format!(
                    "template refs {:?} against {:?}",
                    template_refs(ours),
                    template_refs(vue)
                )
            })
        }
        Contract::SlotRoots => {
            let flags = |facts: &ModuleFacts| {
                let mut flags: Vec<_> = facts
                    .arguments("extend", 1)
                    .into_iter()
                    .map(|argument| snippet(&argument.text))
                    .collect();
                flags.sort_by_key(|flag| format!("{flag:?}"));
                flags
            };
            assert_eq!(flags(vue).len(), 3, "oracle NON_STABLE inventory");
            if flags(ours) != flags(vue) {
                Some(format!(
                    "slot flags {:?} against {:?}",
                    flags(ours),
                    flags(vue)
                ))
            } else if template_refs(ours) != template_refs(vue) {
                Some(format!(
                    "template refs {:?} against {:?}",
                    template_refs(ours),
                    template_refs(vue)
                ))
            } else {
                component_slots_difference(ours, vue)
            }
        }
        Contract::Setters => {
            assert!(!setters(vue).is_empty(), "oracle setter inventory");
            (setters(ours) != setters(vue))
                .then(|| format!("setters {:?} against {:?}", setters(ours), setters(vue)))
        }
        Contract::TemplateNamespaces => {
            assert!(
                template_namespaces(vue)
                    .iter()
                    .any(|(_, namespace)| !namespace.is_empty()),
                "oracle namespace inventory"
            );
            (template_namespaces(ours) != template_namespaces(vue)).then(|| {
                format!(
                    "template namespaces {:?} against {:?}",
                    template_namespaces(ours),
                    template_namespaces(vue)
                )
            })
        }
    }
}

#[test]
fn vapor_render_contracts_follow_pinned_vue() {
    let root = workspace_root().join("crates/verter_vue_conformance/vapor");
    let manifest: serde_json::Value =
        serde_json::from_str(&read_text_normalized(&root.join("oracle.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], "3.6.0-rc.15");
    let rows = manifest["rows"].as_array().unwrap();
    let mut differences = Vec::new();
    for (id, contract) in CASES {
        let source = read_text_normalized(&root.join(format!("cases/{id}.vue"))).unwrap();
        let golden =
            read_text_normalized(&root.join(format!("goldens/3.6.0-rc.15/{id}.js"))).unwrap();
        let row = rows
            .iter()
            .find(|row| row["file"] == format!("{id}.vue"))
            .unwrap_or_else(|| panic!("{id} has an oracle receipt"));
        assert_eq!(row["sourceSha256"], sha256_hex(&source), "{id} source");
        assert_eq!(row["outputSha256"], sha256_hex(&golden), "{id} golden");
        assert!(row["diagnostics"].as_array().unwrap().is_empty());
        let vue = facts(&golden).unwrap_or_else(|error| panic!("{id} golden: {error}"));
        let case = Case {
            name: id.to_string(),
            source,
        };
        let RouteResult::Compiled(view) = run_route(
            RenderRoute::Targets,
            &case,
            Target::FineGrained,
            Mode::Separate,
        ) else {
            panic!("target compiles {id}")
        };
        assert!(view.diagnostics.is_empty(), "{id}: {:?}", view.diagnostics);
        let code = &view.module.as_ref().expect("published module").0;
        let difference = match facts(code) {
            Err(error) => Some(error),
            Ok(ours) => {
                let unresolved: Vec<_> = ours.unresolved.difference(&vue.unresolved).collect();
                let operations = node_operations(code).expect("module parses");
                let native = node_operations(&golden).expect("golden parses");
                if !unresolved.is_empty() {
                    Some(format!("unresolved names {unresolved:?}"))
                } else if operations != native {
                    Some(format!(
                        "operations reach other nodes: {operations:?} against {native:?}"
                    ))
                } else {
                    contract_difference(contract, &ours, &vue)
                }
            }
        };
        if let Some(difference) = difference {
            differences.push(format!(
                "{id} ({contract:?}): {difference}\n{code}\nVue:\n{golden}"
            ));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn template_shapes_ignore_attributes_and_omitted_end_tags() {
    assert_eq!(
        template_shape("<section class=a><!><div class=\"b c\">"),
        template_shape("<section><!><div></div></section>")
    );
    assert_eq!(template_shape("<!-- a --><!><b> </b>"), "C,A,b(T)");
    assert_ne!(
        template_shape("<section><div></div><!>"),
        template_shape("<section><!><div>")
    );
    assert_eq!(template_shape("<p><input>x</p>"), "p(input(),T)");
}
