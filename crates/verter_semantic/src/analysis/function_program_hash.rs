//! Whole-function `flow_body_stable_hash` — the structural content fold of
//! one function body.
//!
//! Hash rules: the fold preserves observable property / destructuring /
//! computed keys, operators, literals, calls, writes, control structure,
//! authored type annotations, and type-affecting JSDoc (`@param` /
//! `@returns` / `@return` / `@type` payloads). Only binding/reference
//! identifier positions are alpha-normalized — a local rename that
//! preserves structure keeps the hash; a property key, free name, literal,
//! operator, or control edit changes it.

use oxc_ast::ast::{ArrowFunctionExpression, Expression, Function, PropertyKey, Statement};
use oxc_ast_visit::{walk, Visit};

use crate::analysis::function_program::FunctionParamRecord;
use crate::analysis::types::{hash_16, Hash16};

// ---------------------------------------------------------------------------
// Whole-function stable hash
// ---------------------------------------------------------------------------

const HASH_SALT: &[u8] = b"verter-flow-body-stable-hash:v1";
const HASH_SEP: u8 = 0;

pub(super) fn hash_function_body(
    source: &str,
    statements: &[Statement<'_>],
    params: &[FunctionParamRecord],
    function_start: u32,
    node: crate::analysis::function_program::FunctionNode<'_>,
) -> Hash16 {
    let mut visitor = HashVisitor {
        buf: Vec::with_capacity(512),
        scopes: vec![rustc_hash::FxHashMap::default()],
        next_ordinal: 0,
    };
    visitor.buf.extend_from_slice(HASH_SALT);
    visitor.tag(HASH_SEP);
    // The function's own name is bound for direct recursion (function
    // declarations / named function expressions); arrows bind no self name.
    match node {
        crate::analysis::function_program::FunctionNode::Function(func) => {
            if let Some(id) = func.id.as_ref() {
                visitor.bind(id.name.as_str());
            }
            visitor.fold_u8(u8::from(func.r#async));
            visitor.fold_u8(u8::from(func.generator));
            visitor.fold_ts_type_annotation(func.return_type.as_ref());
            visitor.fold_ts_type_parameters(func.type_parameters.as_ref());
        }
        crate::analysis::function_program::FunctionNode::Arrow(arrow) => {
            visitor.fold_u8(u8::from(arrow.r#async));
            visitor.fold_u8(u8::from(arrow.expression));
            visitor.fold_ts_type_annotation(arrow.return_type.as_ref());
            visitor.fold_ts_type_parameters(arrow.type_parameters.as_ref());
        }
    }
    // The parameter's CONTENT is whole-body identity: its authored
    // annotation and its default initializer lower into the served
    // parameter type, so an edit to either changes the hash.
    let ast_params: &[oxc_ast::ast::FormalParameter<'_>] = match node {
        crate::analysis::function_program::FunctionNode::Function(func) => &func.params.items,
        crate::analysis::function_program::FunctionNode::Arrow(arrow) => &arrow.params.items,
    };
    for (index, param) in params.iter().enumerate() {
        visitor.tag(0x50);
        visitor.fold_u8(u8::from(param.optional));
        visitor.fold_u8(u8::from(param.rest));
        visitor.fold_u8(u8::from(param.has_ts_annotation));
        if let Some(name) = param.name.as_ref() {
            visitor.bind(name);
        }
        if let Some(ast_param) = ast_params.get(index) {
            visitor.fold_ts_type_annotation(ast_param.type_annotation.as_ref());
            visitor.fold_u8(u8::from(ast_param.initializer.is_some()));
            if let Some(initializer) = ast_param.initializer.as_ref() {
                visitor.visit_expression(initializer);
            }
        }
    }
    let ast_rest_ty: Option<&oxc_allocator::Box<'_, oxc_ast::ast::TSTypeAnnotation<'_>>> =
        match node {
            crate::analysis::function_program::FunctionNode::Function(func) => func
                .params
                .rest
                .as_ref()
                .and_then(|rest| rest.type_annotation.as_ref()),
            crate::analysis::function_program::FunctionNode::Arrow(arrow) => arrow
                .params
                .rest
                .as_ref()
                .and_then(|rest| rest.type_annotation.as_ref()),
        };
    visitor.fold_ts_type_annotation(ast_rest_ty);
    // Type-affecting JSDoc payloads (@param / @returns / @return / @type):
    // folded as payload text; descriptions and other tags are cosmetic.
    visitor.fold_type_affecting_jsdoc(source, function_start);
    for stmt in statements {
        visitor.visit_statement(stmt);
    }
    hash_16(&visitor.buf)
}

/// The structural body folder. Bound identifiers (parameters, locals,
/// nested function names, the function's own name) fold as scope-resolved
/// ordinals (alpha-normalization); free names fold their bytes. Everything
/// observable — node kinds in order, operators, literals, property keys
/// (shorthand / keyed / computed), calls, writes, control, annotations,
/// template text — enters the fold.
struct HashVisitor {
    buf: Vec<u8>,
    scopes: Vec<rustc_hash::FxHashMap<String, u32>>,
    next_ordinal: u32,
}

impl HashVisitor {
    fn tag(&mut self, tag: u8) {
        self.buf.push(tag);
        self.buf.push(HASH_SEP);
    }

    fn fold_u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    fn fold_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    fn fold_bytes(&mut self, bytes: &[u8]) {
        self.fold_u32(u32::try_from(bytes.len()).unwrap_or(u32::MAX));
        self.buf.extend_from_slice(bytes);
    }

    fn fold_str(&mut self, value: &str) {
        self.fold_bytes(value.as_bytes());
    }

    fn fold_debug<T: std::fmt::Debug>(&mut self, value: &T) {
        self.fold_str(&format!("{value:?}"));
    }

    fn bind(&mut self, name: &str) -> u32 {
        let ordinal = self.next_ordinal;
        self.next_ordinal += 1;
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), ordinal);
        }
        ordinal
    }

    fn resolve(&self, name: &str) -> Option<u32> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn push_scope(&mut self) {
        self.scopes.push(rustc_hash::FxHashMap::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn fold_identifier_use(&mut self, name: &str) {
        match self.resolve(name) {
            Some(ordinal) => {
                self.tag(0xB0);
                self.fold_u32(ordinal);
            }
            None => {
                self.tag(0xB1);
                self.fold_str(name);
            }
        }
    }

    fn fold_ts_type_annotation(
        &mut self,
        annotation: Option<&oxc_allocator::Box<'_, oxc_ast::ast::TSTypeAnnotation<'_>>>,
    ) {
        self.fold_u8(u8::from(annotation.is_some()));
        if let Some(annotation) = annotation {
            self.visit_ts_type(&annotation.type_annotation);
        }
    }

    fn fold_ts_type_parameters(
        &mut self,
        parameters: Option<&oxc_allocator::Box<'_, oxc_ast::ast::TSTypeParameterDeclaration<'_>>>,
    ) {
        self.fold_u8(u8::from(parameters.is_some()));
        if let Some(parameters) = parameters {
            for param in &parameters.params {
                self.fold_str(&param.name.name);
                if let Some(constraint) = param.constraint.as_ref() {
                    self.tag(0xC1);
                    self.visit_ts_type(constraint);
                }
                if let Some(default) = param.default.as_ref() {
                    self.tag(0xC2);
                    self.visit_ts_type(default);
                }
            }
        }
    }

    /// Fold the type-affecting payloads of the function's leading JSDoc
    /// block (`@param` / `@returns` / `@return` / `@type` — the tags the
    /// signature-recovery path consumes). Descriptions, other tags, and
    /// non-JSDoc comments are cosmetic and never enter the fold.
    fn fold_type_affecting_jsdoc(&mut self, source: &str, function_start: u32) {
        let Some((start, end)) =
            crate::analysis::jsdoc::find_leading_jsdoc_block_offsets(source, function_start)
        else {
            self.fold_u8(0);
            return;
        };
        self.fold_u8(1);
        let Some(block) = source.get(start..end) else {
            return;
        };
        for (tag, payload) in type_affecting_jsdoc_tag_payloads(block) {
            self.tag(0xC8);
            self.fold_str(tag);
            // Only the TYPED payload is type-affecting: the trailing
            // parameter name and description text are cosmetic and never
            // enter the fold.
            self.fold_str(jsdoc_type_payload(payload));
        }
    }
}

/// The type-affecting JSDoc tags whose payloads feed signature recovery.
const TYPE_AFFECTING_JSDOC_TAGS: &[&str] = &["param", "returns", "return", "type"];

/// The TYPED payload of one tag payload string: the balanced `{T}` group
/// when present, else the first whitespace-delimited token. The parameter
/// name and any description text after the type are cosmetic.
fn jsdoc_type_payload(payload: &str) -> &str {
    let trimmed = payload.trim();
    let bytes = trimmed.as_bytes();
    let mut depth = 0usize;
    let mut open = None;
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'{' => {
                if depth == 0 {
                    open = Some(index);
                }
                depth += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some(start) = open {
                        return &trimmed[start..=index];
                    }
                }
            }
            _ => {}
        }
    }
    trimmed.split_whitespace().next().unwrap_or(trimmed)
}

/// `(tag_name, payload)` pairs of a JSDoc block, in order. A payload runs
/// from after the tag name to the next `@tag` line or the block end.
fn type_affecting_jsdoc_tag_payloads(block: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let bytes = block.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] == b'@'
            && (cursor == 0 || bytes[cursor - 1] == b'\n' || bytes[cursor - 1] == b' ')
        {
            let name_start = cursor + 1;
            let mut name_end = name_start;
            while name_end < bytes.len()
                && (bytes[name_end].is_ascii_alphanumeric() || bytes[name_end] == b'_')
            {
                name_end += 1;
            }
            let name = &block[name_start..name_end];
            if TYPE_AFFECTING_JSDOC_TAGS.contains(&name) {
                let mut payload_end = block.len();
                let mut scan = name_end;
                while scan < block.len() {
                    if block.as_bytes()[scan] == b'\n' {
                        let mut look = scan + 1;
                        while look < block.len()
                            && (block.as_bytes()[look] == b' '
                                || block.as_bytes()[look] == b'*'
                                || block.as_bytes()[look] == b'\t')
                        {
                            look += 1;
                        }
                        if look < block.len() && block.as_bytes()[look] == b'@' {
                            payload_end = scan;
                            break;
                        }
                    }
                    scan += 1;
                }
                out.push((name, &block[name_end..payload_end]));
            }
            cursor = name_end.max(cursor + 1);
        } else {
            cursor += 1;
        }
    }
    out
}

impl<'a> Visit<'a> for HashVisitor {
    fn visit_statement(&mut self, it: &Statement<'a>) {
        let tag = match it {
            Statement::BlockStatement(_) => 0x10,
            Statement::BreakStatement(_) => 0x11,
            Statement::ContinueStatement(_) => 0x12,
            Statement::DebuggerStatement(_) => 0x13,
            Statement::DoWhileStatement(_) => 0x14,
            Statement::EmptyStatement(_) => 0x15,
            Statement::ExpressionStatement(_) => 0x16,
            Statement::ForInStatement(_) => 0x17,
            Statement::ForOfStatement(for_of) => {
                self.tag(0x18);
                self.fold_u8(u8::from(for_of.r#await));
                walk::walk_statement(self, it);
                return;
            }
            Statement::ForStatement(_) => 0x19,
            Statement::IfStatement(_) => 0x1A,
            Statement::LabeledStatement(_) => 0x1B,
            Statement::ReturnStatement(_) => 0x1C,
            Statement::SwitchStatement(_) => 0x1D,
            Statement::ThrowStatement(_) => 0x1E,
            Statement::TryStatement(_) => 0x1F,
            Statement::WhileStatement(_) => 0x20,
            Statement::WithStatement(_) => 0x21,
            Statement::FunctionDeclaration(_) => 0x22,
            Statement::ClassDeclaration(_) => 0x23,
            Statement::VariableDeclaration(var_decl) => {
                self.tag(0x24);
                self.fold_debug(&var_decl.kind);
                walk::walk_statement(self, it);
                return;
            }
            Statement::TSTypeAliasDeclaration(_) => 0x25,
            Statement::TSInterfaceDeclaration(_) => 0x26,
            Statement::TSEnumDeclaration(_) => 0x27,
            Statement::TSModuleDeclaration(_) => 0x28,
            Statement::TSGlobalDeclaration(_) => 0x29,
            Statement::TSImportEqualsDeclaration(_) => 0x2A,
            Statement::ImportDeclaration(_) => 0x2B,
            Statement::ExportAllDeclaration(_) => 0x2C,
            Statement::ExportDefaultDeclaration(_) => 0x2D,
            Statement::ExportNamedDeclaration(_) => 0x2E,
            Statement::TSExportAssignment(_) => 0x2F,
            Statement::TSNamespaceExportDeclaration(_) => 0x30,
        };
        self.tag(tag);
        walk::walk_statement(self, it);
    }

    fn visit_expression(&mut self, it: &Expression<'a>) {
        let tag = match it {
            Expression::BooleanLiteral(_) => 0x40,
            Expression::NullLiteral(_) => 0x41,
            Expression::NumericLiteral(_) => 0x42,
            Expression::BigIntLiteral(_) => 0x43,
            Expression::RegExpLiteral(_) => 0x44,
            Expression::StringLiteral(_) => 0x45,
            Expression::TemplateLiteral(_) => 0x46,
            Expression::Identifier(_) => 0x47,
            Expression::MetaProperty(_) => 0x48,
            Expression::Super(_) => 0x49,
            Expression::ArrayExpression(_) => 0x4A,
            Expression::ArrowFunctionExpression(_) => 0x4B,
            Expression::AssignmentExpression(assign) => {
                self.tag(0x4C);
                self.fold_debug(&assign.operator);
                walk::walk_expression(self, it);
                return;
            }
            Expression::AwaitExpression(_) => 0x4D,
            Expression::BinaryExpression(binary) => {
                self.tag(0x4E);
                self.fold_debug(&binary.operator);
                walk::walk_expression(self, it);
                return;
            }
            Expression::CallExpression(call) => {
                self.tag(0x4F);
                self.fold_u8(u8::from(call.optional));
                walk::walk_expression(self, it);
                return;
            }
            Expression::ChainExpression(_) => 0x60,
            Expression::ClassExpression(_) => 0x61,
            Expression::ConditionalExpression(_) => 0x62,
            Expression::FunctionExpression(_) => 0x63,
            Expression::ImportExpression(_) => 0x64,
            Expression::LogicalExpression(logical) => {
                self.tag(0x65);
                self.fold_debug(&logical.operator);
                walk::walk_expression(self, it);
                return;
            }
            Expression::NewExpression(_) => 0x66,
            Expression::ObjectExpression(_) => 0x67,
            Expression::ParenthesizedExpression(_) => 0x68,
            Expression::SequenceExpression(_) => 0x69,
            Expression::TaggedTemplateExpression(_) => 0x6A,
            Expression::ThisExpression(_) => 0x6B,
            Expression::UnaryExpression(unary) => {
                self.tag(0x6C);
                self.fold_debug(&unary.operator);
                walk::walk_expression(self, it);
                return;
            }
            Expression::UpdateExpression(update) => {
                self.tag(0x6D);
                self.fold_debug(&update.operator);
                self.fold_u8(u8::from(update.prefix));
                walk::walk_expression(self, it);
                return;
            }
            Expression::YieldExpression(_) => 0x6E,
            Expression::PrivateInExpression(_) => 0x6F,
            Expression::JSXElement(_) => 0x70,
            Expression::JSXFragment(_) => 0x71,
            Expression::TSAsExpression(_) => 0x72,
            Expression::TSSatisfiesExpression(_) => 0x73,
            Expression::TSTypeAssertion(_) => 0x74,
            Expression::TSNonNullExpression(_) => 0x75,
            Expression::TSInstantiationExpression(_) => 0x76,
            Expression::V8IntrinsicExpression(_) => 0x77,
            Expression::StaticMemberExpression(member) => {
                self.tag(0x78);
                self.fold_u8(u8::from(member.optional));
                self.fold_str(&member.property.name);
                self.visit_expression(&member.object);
                return;
            }
            Expression::ComputedMemberExpression(member) => {
                self.tag(0x79);
                self.fold_u8(u8::from(member.optional));
                self.visit_expression(&member.object);
                self.visit_expression(&member.expression);
                return;
            }
            Expression::PrivateFieldExpression(member) => {
                self.tag(0x7A);
                self.fold_str(&member.field.name);
                self.visit_expression(&member.object);
                return;
            }
        };
        self.tag(tag);
        walk::walk_expression(self, it);
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.fold_identifier_use(it.name.as_str());
    }

    fn visit_binding_identifier(&mut self, it: &oxc_ast::ast::BindingIdentifier<'a>) {
        self.tag(0xB2);
        let ordinal = self.bind(it.name.as_str());
        self.fold_u32(ordinal);
    }

    fn visit_string_literal(&mut self, it: &oxc_ast::ast::StringLiteral<'a>) {
        self.fold_str(&it.value);
    }

    fn visit_numeric_literal(&mut self, it: &oxc_ast::ast::NumericLiteral<'a>) {
        self.fold_str(&format!("{:?}", it.value.to_bits()));
    }

    fn visit_boolean_literal(&mut self, it: &oxc_ast::ast::BooleanLiteral) {
        self.fold_u8(u8::from(it.value));
    }

    fn visit_big_int_literal(&mut self, it: &oxc_ast::ast::BigIntLiteral<'a>) {
        self.fold_str(&it.value);
    }

    fn visit_reg_exp_literal(&mut self, it: &oxc_ast::ast::RegExpLiteral<'a>) {
        self.fold_str(it.regex.pattern.text.as_str());
        self.fold_debug(&it.regex.flags);
    }

    fn visit_template_element(&mut self, it: &oxc_ast::ast::TemplateElement<'a>) {
        self.fold_str(it.value.raw.as_str());
    }

    fn visit_object_property(&mut self, it: &oxc_ast::ast::ObjectProperty<'a>) {
        self.fold_u8(u8::from(it.shorthand));
        self.fold_u8(u8::from(it.computed));
        self.fold_debug(&it.kind);
        walk::walk_object_property(self, it);
    }

    fn visit_property_key(&mut self, it: &PropertyKey<'a>) {
        // Observable key identity: shorthand vs keyed vs computed is folded
        // by visit_object_property; the key's own content folds here. A
        // static identifier key is a NAME, never an alpha-normalized
        // reference.
        match it {
            PropertyKey::StaticIdentifier(id) => {
                self.tag(0x90);
                self.fold_str(&id.name);
            }
            PropertyKey::PrivateIdentifier(id) => {
                self.tag(0x91);
                self.fold_str(&id.name);
            }
            key => {
                self.tag(0x92);
                if let Some(expr) = key.as_expression() {
                    self.visit_expression(expr);
                }
            }
        }
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.push_scope();
        if let Some(id) = it.id.as_ref() {
            self.bind(id.name.as_str());
        }
        walk::walk_function(self, it, flags);
        self.pop_scope();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.push_scope();
        walk::walk_arrow_function_expression(self, it);
        self.pop_scope();
    }

    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'a>) {
        self.push_scope();
        walk::walk_block_statement(self, it);
        self.pop_scope();
    }

    fn visit_ts_type(&mut self, it: &oxc_ast::ast::TSType<'a>) {
        use oxc_ast::ast::TSType;
        let tag: u16 = match it {
            TSType::TSAnyKeyword(_) => 0x100,
            TSType::TSBigIntKeyword(_) => 0x101,
            TSType::TSBooleanKeyword(_) => 0x102,
            TSType::TSIntrinsicKeyword(_) => 0x103,
            TSType::TSNeverKeyword(_) => 0x104,
            TSType::TSNullKeyword(_) => 0x105,
            TSType::TSNumberKeyword(_) => 0x106,
            TSType::TSObjectKeyword(_) => 0x107,
            TSType::TSStringKeyword(_) => 0x108,
            TSType::TSSymbolKeyword(_) => 0x109,
            TSType::TSThisType(_) => 0x10A,
            TSType::TSUndefinedKeyword(_) => 0x10B,
            TSType::TSUnknownKeyword(_) => 0x10C,
            TSType::TSVoidKeyword(_) => 0x10D,
            TSType::TSArrayType(_) => 0x10E,
            TSType::TSConditionalType(_) => 0x10F,
            TSType::TSConstructorType(_) => 0x110,
            TSType::TSFunctionType(_) => 0x111,
            TSType::TSImportType(_) => 0x112,
            TSType::TSIndexedAccessType(_) => 0x113,
            TSType::TSInferType(_) => 0x114,
            TSType::TSIntersectionType(_) => 0x115,
            TSType::TSLiteralType(_) => 0x116,
            TSType::TSMappedType(_) => 0x117,
            TSType::TSNamedTupleMember(_) => 0x118,
            TSType::TSTemplateLiteralType(_) => 0x119,
            TSType::TSTupleType(_) => 0x11A,
            TSType::TSTypeLiteral(_) => 0x11B,
            TSType::TSTypeOperatorType(operator) => {
                self.fold_u32(0x11C);
                self.fold_debug(&operator.operator);
                walk::walk_ts_type(self, it);
                return;
            }
            TSType::TSTypePredicate(_) => 0x11D,
            TSType::TSTypeQuery(_) => 0x11E,
            TSType::TSTypeReference(_) => 0x11F,
            TSType::TSUnionType(_) => 0x120,
            TSType::TSParenthesizedType(_) => 0x121,
            TSType::JSDocNullableType(_) => 0x122,
            TSType::JSDocNonNullableType(_) => 0x123,
            TSType::JSDocUnknownType(_) => 0x124,
        };
        self.fold_u32(u32::from(tag));
        walk::walk_ts_type(self, it);
    }

    fn visit_ts_type_name(&mut self, it: &oxc_ast::ast::TSTypeName<'a>) {
        // Type references are free names (resolved through the shared
        // resolver, never alpha-normalized).
        match it {
            oxc_ast::ast::TSTypeName::IdentifierReference(id) => self.fold_str(&id.name),
            oxc_ast::ast::TSTypeName::QualifiedName(name) => {
                self.visit_ts_type_name(&name.left);
                self.tag(0x93);
                self.fold_str(&name.right.name);
            }
            oxc_ast::ast::TSTypeName::ThisExpression(_) => {
                self.tag(0x94);
            }
        }
    }
}
