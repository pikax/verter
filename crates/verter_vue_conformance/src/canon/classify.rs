//! Exact-vs-Alpha identifier classification and binding-key construction
//! (extracted from `canon.rs` — see `mod.rs` for the canonicalizer overview).

use std::collections::{BTreeSet, HashMap};

use oxc_ast::ast::*;
use oxc_ast::AstKind;
use oxc_semantic::{
    AstNodes, NodeId, ReferenceId, ScopeId, Scoping, Semantic, SymbolFlags, SymbolId,
};
use oxc_span::{GetSpan, Span};

use super::canonize::{kind_name_of_declaration, module_export_name, refused};
use super::{BindingKey, BindingKind, Canon};

// ---------------------------------------------------------------------------
// Identifier classification (Exact vs Alpha).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentClass {
    Exact,
    Alpha,
}

pub(crate) struct Classifier {
    classes: HashMap<SymbolId, IdentClass>,
    keys: HashMap<SymbolId, BindingKey>,
    /// Import bindings → family identity `(source, imported)`.
    imports: HashMap<SymbolId, (String, String)>,
}

impl Classifier {
    pub(crate) fn build(semantic: &Semantic, authored: &BTreeSet<String>) -> Classifier {
        let scoping = semantic.scoping();
        let nodes = semantic.nodes();

        // Import binding family identities (alias spellings are waived).
        let mut imports: HashMap<SymbolId, (String, String)> = HashMap::new();
        for statement in &semantic.nodes().program().body {
            let Statement::ImportDeclaration(import) = statement else {
                continue;
            };
            let source = import.source.value.to_string();
            if let Some(specifiers) = &import.specifiers {
                for specifier in specifiers {
                    match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                            if let Some(symbol_id) = specifier.local.symbol_id.get() {
                                imports.insert(
                                    symbol_id,
                                    (source.clone(), module_export_name(&specifier.imported)),
                                );
                            }
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                            if let Some(symbol_id) = specifier.local.symbol_id.get() {
                                imports.insert(symbol_id, (source.clone(), "default".to_string()));
                            }
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                            if let Some(symbol_id) = specifier.local.symbol_id.get() {
                                imports.insert(symbol_id, (source.clone(), "*".to_string()));
                            }
                        }
                    }
                }
            }
        }

        // Ultra-conservative: any direct eval / `with` disables alpha for the
        // whole module (bindings become observable at runtime).
        let has_eval_or_with = nodes.iter().any(|node| match node.kind() {
            AstKind::WithStatement(_) => true,
            AstKind::CallExpression(call) => {
                matches!(&call.callee, Expression::Identifier(id)
                    if id.name.as_str() == "eval"
                        && semantic.is_unresolved_reference(node.id()))
            }
            _ => false,
        });

        let exported = collect_exported_symbols(semantic);

        // Declaration ordinal per scope: rank bindings by (span start, id).
        let mut per_scope: HashMap<ScopeId, Vec<SymbolId>> = HashMap::new();
        for symbol_id in scoping.symbol_ids() {
            per_scope
                .entry(scoping.symbol_scope_id(symbol_id))
                .or_default()
                .push(symbol_id);
        }
        let mut declaration_ordinals: HashMap<SymbolId, u32> = HashMap::new();
        for (_scope, mut symbols) in per_scope {
            symbols.sort_by_key(|s| {
                let span = scoping.symbol_span(*s);
                (span.start, s.index() as u32)
            });
            for (rank, symbol) in symbols.into_iter().enumerate() {
                declaration_ordinals.insert(symbol, rank as u32);
            }
        }

        let mut classes = HashMap::new();
        let mut keys = HashMap::new();
        for symbol_id in scoping.symbol_ids() {
            let name = scoping.symbol_name(symbol_id);
            let flags = scoping.symbol_flags(symbol_id);
            let exact = has_eval_or_with
                || authored.contains(name)
                || exported.contains(&symbol_id)
                || used_in_name_bearing_position(semantic, symbol_id);
            let class = if exact {
                IdentClass::Exact
            } else {
                IdentClass::Alpha
            };
            classes.insert(symbol_id, class);
            if class == IdentClass::Alpha {
                let scope_id = scoping.symbol_scope_id(symbol_id);
                let key = BindingKey {
                    scope_ordinal: scoping.get_node_id(scope_id).index() as u32,
                    declaration_ordinal: *declaration_ordinals.get(&symbol_id).unwrap_or(&u32::MAX),
                    pattern_slot: binding_pattern_slot(scoping, nodes, symbol_id),
                    kind: binding_kind(scoping, nodes, symbol_id, flags),
                };
                keys.insert(symbol_id, key);
            }
        }
        Classifier {
            classes,
            keys,
            imports,
        }
    }

    fn classify_symbol(&self, symbol_id: SymbolId, fallback_name: &str) -> Canon {
        if let Some((source, imported)) = self.imports.get(&symbol_id) {
            return Canon::ImportBinding {
                source: source.clone(),
                imported: imported.clone(),
            };
        }
        match self.classes.get(&symbol_id) {
            Some(IdentClass::Alpha) => {
                Canon::Alpha(self.keys.get(&symbol_id).expect("alpha key").clone())
            }
            _ => Canon::leaf("ident", fallback_name),
        }
    }

    pub(crate) fn classify_binding(&self, ident: &BindingIdentifier) -> Canon {
        match ident.symbol_id.get() {
            Some(symbol_id) => self.classify_symbol(symbol_id, ident.name.as_str()),
            None => Canon::leaf("ident", ident.name.as_str()),
        }
    }

    pub(crate) fn classify_reference(
        &self,
        scoping: &Scoping,
        ident: &IdentifierReference,
    ) -> Canon {
        let resolved: Option<SymbolId> = ident
            .reference_id
            .get()
            .and_then(|rid: ReferenceId| scoping.get_reference(rid).symbol_id());
        match resolved {
            Some(symbol_id) => self.classify_symbol(symbol_id, ident.name.as_str()),
            // Unresolved/global reference — the NAME is the contract.
            None => Canon::leaf("ident", ident.name.as_str()),
        }
    }
}

fn collect_exported_symbols(semantic: &Semantic) -> BTreeSet<SymbolId> {
    let scoping = semantic.scoping();
    let mut out = BTreeSet::new();
    for stmt in &semantic.nodes().program().body {
        match stmt {
            Statement::ExportNamedDeclaration(export) => {
                if let Some(declaration) = &export.declaration {
                    collect_declaration_bindings(declaration, &mut |symbol_id| {
                        out.insert(symbol_id);
                    });
                }
                for specifier in &export.specifiers {
                    match &specifier.local {
                        ModuleExportName::IdentifierReference(reference) => {
                            if let Some(symbol_id) = reference
                                .reference_id
                                .get()
                                .and_then(|rid| scoping.get_reference(rid).symbol_id())
                            {
                                out.insert(symbol_id);
                            }
                        }
                        ModuleExportName::IdentifierName(name) => {
                            // `export { foo }` where `foo` never had a
                            // reference_id-bearing node — resolve by name in
                            // the root scope.
                            let root = scoping.root_scope_id();
                            for (ident, symbol_id) in scoping.get_bindings(root).iter() {
                                if ident.as_str() == name.name.as_str() {
                                    out.insert(*symbol_id);
                                }
                            }
                        }
                        ModuleExportName::StringLiteral(_) => {}
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                    if let Some(id) = &function.id {
                        if let Some(symbol_id) = id.symbol_id.get() {
                            out.insert(symbol_id);
                        }
                    }
                }
                ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                    if let Some(id) = &class.id {
                        if let Some(symbol_id) = id.symbol_id.get() {
                            out.insert(symbol_id);
                        }
                    }
                }
                ExportDefaultDeclarationKind::Identifier(reference) => {
                    if let Some(symbol_id) = reference
                        .reference_id
                        .get()
                        .and_then(|rid| scoping.get_reference(rid).symbol_id())
                    {
                        out.insert(symbol_id);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    out
}

fn collect_declaration_bindings(declaration: &Declaration, out: &mut dyn FnMut(SymbolId)) {
    match declaration {
        Declaration::VariableDeclaration(variable) => {
            for declarator in &variable.declarations {
                collect_pattern_symbols(&declarator.id, out);
            }
        }
        Declaration::FunctionDeclaration(function) => {
            if let Some(id) = &function.id {
                if let Some(symbol_id) = id.symbol_id.get() {
                    out(symbol_id);
                }
            }
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                if let Some(symbol_id) = id.symbol_id.get() {
                    out(symbol_id);
                }
            }
        }
        other => refused("Declaration", kind_name_of_declaration(other)),
    }
}

fn collect_pattern_symbols(pattern: &BindingPattern, out: &mut dyn FnMut(SymbolId)) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            if let Some(symbol_id) = ident.symbol_id.get() {
                out(symbol_id);
            }
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                collect_pattern_symbols(&property.value, out);
            }
            if let Some(rest) = &object.rest {
                collect_pattern_symbols(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for pattern in array.elements.iter().flatten() {
                collect_pattern_symbols(pattern, out);
            }
            if let Some(rest) = &array.rest {
                collect_pattern_symbols(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            collect_pattern_symbols(&assignment.left, out);
        }
    }
}

/// A reference in a name-bearing position keeps the binding's spelling exact:
/// object-shorthand properties, destructuring-assignment shorthand, and
/// `.name` inferred-name reads.
fn used_in_name_bearing_position(semantic: &Semantic, symbol_id: SymbolId) -> bool {
    let nodes = semantic.nodes();
    for reference in semantic.symbol_references(symbol_id) {
        let parent = nodes.parent_kind(reference.node_id());
        match parent {
            AstKind::ObjectProperty(property) => {
                if property.shorthand {
                    return true;
                }
            }
            AstKind::AssignmentTargetPropertyIdentifier(_) => return true,
            AstKind::StaticMemberExpression(member)
                if member.property.name.as_str() == "name"
                    && member.object.span() == reference_span(nodes, reference.node_id()) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn reference_span(nodes: &AstNodes, node_id: NodeId) -> Span {
    nodes.kind(node_id).span()
}

fn binding_kind(
    scoping: &Scoping,
    nodes: &AstNodes,
    symbol_id: SymbolId,
    flags: SymbolFlags,
) -> BindingKind {
    if flags.intersects(SymbolFlags::Import | SymbolFlags::TypeImport) {
        return BindingKind::Import;
    }
    if flags.contains(SymbolFlags::CatchVariable) {
        return BindingKind::Catch;
    }
    if flags.contains(SymbolFlags::Function) {
        return BindingKind::Function;
    }
    if flags.contains(SymbolFlags::Class) {
        return BindingKind::Class;
    }
    let declaration = scoping.symbol_declaration(symbol_id);
    if matches!(
        nodes.kind(declaration),
        AstKind::FormalParameter(_) | AstKind::FormalParameterRest(_)
    ) {
        return BindingKind::Param;
    }
    if flags.contains(SymbolFlags::ConstVariable) {
        return BindingKind::Const;
    }
    if flags.contains(SymbolFlags::BlockScopedVariable) {
        return BindingKind::Let;
    }
    BindingKind::Var
}

/// Child-index path of the binding's identifier within its root pattern.
fn binding_pattern_slot(scoping: &Scoping, nodes: &AstNodes, symbol_id: SymbolId) -> Vec<u8> {
    let declaration = scoping.symbol_declaration(symbol_id);
    let span = scoping.symbol_span(symbol_id);
    match nodes.kind(declaration) {
        AstKind::VariableDeclarator(declarator) => slot_in_pattern(&declarator.id, span),
        AstKind::FormalParameter(parameter) => slot_in_pattern(&parameter.pattern, span),
        AstKind::FormalParameterRest(rest) => {
            let mut slot = vec![255];
            slot.extend(slot_in_pattern(&rest.rest.argument, span));
            slot
        }
        AstKind::CatchParameter(parameter) => slot_in_pattern(&parameter.pattern, span),
        // Function/class ids and import specifiers are plain single bindings.
        _ => Vec::new(),
    }
}

fn slot_in_pattern(pattern: &BindingPattern, span: Span) -> Vec<u8> {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            if ident.span == span {
                Vec::new()
            } else {
                vec![254]
            }
        }
        BindingPattern::ObjectPattern(object) => {
            for (index, property) in object.properties.iter().enumerate() {
                if property.value.span().contains_inclusive(span) {
                    let mut slot = vec![index.min(253) as u8];
                    slot.extend(slot_in_pattern(&property.value, span));
                    return slot;
                }
            }
            if let Some(rest) = &object.rest {
                if rest.argument.span().contains_inclusive(span) {
                    let mut slot = vec![255];
                    slot.extend(slot_in_pattern(&rest.argument, span));
                    return slot;
                }
            }
            vec![254]
        }
        BindingPattern::ArrayPattern(array) => {
            for (index, element) in array.elements.iter().enumerate() {
                if let Some(pattern) = element {
                    if pattern.span().contains_inclusive(span) {
                        let mut slot = vec![index.min(253) as u8];
                        slot.extend(slot_in_pattern(pattern, span));
                        return slot;
                    }
                }
            }
            if let Some(rest) = &array.rest {
                if rest.argument.span().contains_inclusive(span) {
                    let mut slot = vec![255];
                    slot.extend(slot_in_pattern(&rest.argument, span));
                    return slot;
                }
            }
            vec![254]
        }
        BindingPattern::AssignmentPattern(assignment) => {
            let mut slot = vec![254];
            slot.extend(slot_in_pattern(&assignment.left, span));
            slot
        }
    }
}
