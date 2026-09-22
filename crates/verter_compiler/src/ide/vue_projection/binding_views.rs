//! Live template read/write views and actual usage accounting.
//!
//! Three named products:
//!
//! - [`TemplateReadView`]: the template-visible read shape of each setup or
//!   Options binding. Top-level `ref` (and `toRefs`) locals read unwrapped
//!   in the template while the script side still sees the `Ref` (`.value`)
//!   wrapper; nested refs (a `ref` inside an object literal) stay wrapped.
//!   `reactive` arrays/collections read directly; `readonly` objects and
//!   `defineProps` members (including destructured reactive props) read
//!   directly and are never writable through the write product.
//! - [`TemplateWriteTarget`]: the write shape, deliberately distinct from
//!   the read shape. Mutable refs, writable-computed setters (typed by the
//!   declared setter domain, not the read type), reactive members and
//!   `defineModel` refs are writable; getter-only computed values and
//!   readonly props are refused by [`TemplateWriteTarget::write_target`].
//!   There is no universal mutable alias: every write resolves through that
//!   one checked entry point.
//! - [`BindingUsageSet`]: actual usage only. A binding counts as used when
//!   an authored template, script or style reference names it; nothing
//!   seeds synthetic `void` reads, so `noUnusedLocals` stays meaningful and
//!   a binding unused in every region is reported by
//!   [`BindingUsageSet::unused`] instead of being hidden.
//!
//! Classification is syntax-only over one parse of the setup block under
//! its own grammar (plus the [`CombinedScriptProjection`] facts for Options
//! members, owned by [`project_options_pair`](super::options_api::project_options_pair)).
//! Vue factory calls (`ref`, `computed`, `reactive`, `readonly`,
//! `defineModel`, `toRefs`) resolve by binding, not by bare spelling: the
//! local name must be a runtime import of the matching export from `'vue'`,
//! or a free reference. A setup-local value binding of the same name makes
//! the call ordinary, so a local `function ref()` never unwraps anything.
//! `defineProps` follows the same rule (free reference or runtime `'vue'`
//! import). TypeScript stays the type-answer owner: the setter domain is
//! the authored annotation text slice, never an evaluated type.
//!
//! Views are live: [`TemplateReadView::snapshot_kind`] is always
//! [`ViewSnapshotKind::Live`]. No immutable snapshot type is generated, so
//! an authored mutation cannot be masked by a narrowing-preserving copy.
//!
//! Dormant relative to the live IDE route: Vue IDE routing stays on
//! [`super::super::script`] until STP58 atomic activation. Qualification
//! harnesses reach this through
//! [`VueProjectionBackend::binding_views`](crate::framework_common::vue_projection_backend::VueProjectionBackend::binding_views).

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingPattern, Expression, ImportDeclarationSpecifier, ObjectPropertyKind, Program,
    PropertyKey, Statement, TSSignature, TSType,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::cursor::ScriptLanguage;

use super::options_api::{project_options_pair, CombinedScriptProjection, OptionsMemberKind};
use super::script_setup::{value_bindings, ScriptBlockInput, SetupProjectionRefusal, SourceRange};

/// Syntax classification of one template-visible binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingKind {
    /// Top-level `ref(...)` (or one `toRefs(...)` member): the template
    /// reads the value unwrapped; the script side keeps the wrapper.
    Ref,
    /// `computed(...)`: `setter` is true for the `{ get, set }` form.
    Computed {
        /// Whether a setter was declared.
        setter: bool,
    },
    /// `reactive(...)` object/array/collection.
    Reactive,
    /// `readonly(...)` wrap or a `defineProps` member (destructured or
    /// whole-object): direct reads, never writable.
    Readonly,
    /// `defineModel(...)`: a two-way model ref, writable with its value.
    Model,
    /// Direct lexical read: plain locals, functions, imports, object
    /// literals holding nested refs, reactive destructuring copies.
    Plain,
}

/// One template-visible read binding with its source link.
///
/// The `range` is the authored declarator/property name in carrier
/// coordinates, so hover and rename keep pointing at the source binding
/// even though TypeScript checks the generated view property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateReadBinding {
    /// Binding name.
    pub name: String,
    /// Syntax classification.
    pub kind: BindingKind,
    /// True when the template reads this binding unwrapped while the
    /// script side keeps the wrapper (top-level refs only).
    pub unwrapped: bool,
    /// True when the script-side declaration keeps the `Ref` wrapper
    /// (`.value` access in `<script setup>`).
    pub script_wraps_ref: bool,
    /// Authored name span in the carrier.
    pub range: SourceRange,
}

/// The template-visible read shape: one live row per binding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateReadView {
    /// Read rows in authored order.
    pub bindings: Vec<TemplateReadBinding>,
}

/// What kind of generated view the read rows describe. Only one variant
/// exists: views are live bindings, never immutable snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewSnapshotKind {
    /// The view names the live binding; authored mutations stay visible.
    Live,
}

impl TemplateReadView {
    /// Look up one read binding by name.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&TemplateReadBinding> {
        self.bindings.iter().find(|binding| binding.name == name)
    }

    /// Always [`ViewSnapshotKind::Live`]: this product generates no
    /// immutable snapshot that could preserve narrowing across an authored
    /// mutation.
    #[must_use]
    pub fn snapshot_kind(&self) -> ViewSnapshotKind {
        ViewSnapshotKind::Live
    }
}

/// The declared write domain of one writable binding. Read types and write
/// domains are separate facts: a writable computed accepts its setter
/// domain, not merely its read type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteDomain {
    /// Mutable `ref` / `toRefs` member: written with the wrapped value.
    RefValue,
    /// Writable computed: written with the declared setter parameter type
    /// (authored annotation text, e.g. `"string"`; empty when the setter
    /// is unannotated, never copied from the read type).
    SetterParam(String),
    /// Reactive member.
    ReactiveMember,
    /// `defineModel` ref: two-way model value.
    ModelValue,
    /// Plain lexical assignment target.
    PlainAssign,
}

/// One writable binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritableBinding {
    /// Binding name.
    pub name: String,
    /// Declared write domain.
    pub domain: WriteDomain,
}

/// Refusal for a template assignment that must not typecheck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteRejection {
    /// Assignment to a getter-only `computed(...)`.
    GetterOnlyComputed,
    /// Assignment to a readonly prop or `readonly(...)` object.
    ReadonlyProp,
    /// Assignment to an immutable lexical (`const`) binding.
    ConstBinding,
    /// The name is not a known binding at all.
    UnknownBinding,
}

/// The template-visible write shape. The only way to resolve a write is
/// [`TemplateWriteTarget::write_target`]; there is no universal mutable
/// alias that would let a getter-only computed or readonly prop through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateWriteTarget {
    /// Writable rows in authored order.
    pub writable: Vec<WritableBinding>,
    /// Names refused as getter-only computed writes.
    pub getter_only: Vec<String>,
    /// Names refused as readonly writes.
    pub readonly: Vec<String>,
    /// Names refused as `const` reassignment writes.
    pub immutable: Vec<String>,
}

impl TemplateWriteTarget {
    /// Resolve a template assignment target: `Ok` for writable bindings,
    /// `Err` for getter-only computed values, readonly props/objects,
    /// immutable `const` bindings, and unknown names.
    pub fn write_target(&self, name: &str) -> Result<&WritableBinding, WriteRejection> {
        if let Some(binding) = self.writable.iter().find(|binding| binding.name == name) {
            return Ok(binding);
        }
        if self.getter_only.iter().any(|known| known == name) {
            return Err(WriteRejection::GetterOnlyComputed);
        }
        if self.readonly.iter().any(|known| known == name) {
            return Err(WriteRejection::ReadonlyProp);
        }
        if self.immutable.iter().any(|known| known == name) {
            return Err(WriteRejection::ConstBinding);
        }
        Err(WriteRejection::UnknownBinding)
    }
}

/// Which authored regions reference a binding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageRegions {
    /// Named by an authored template reference.
    pub template: bool,
    /// Named by an authored script reference.
    pub script: bool,
    /// Named by an authored style reference (`v-bind`).
    pub style: bool,
}

/// One actually-used binding and where it was referenced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsedBinding {
    /// Binding name.
    pub name: String,
    /// Regions with an authored reference.
    pub regions: UsageRegions,
}

/// Lexical identifier mentions in template markup: ASCII word scan over
/// authored bytes. Words that cannot start an identifier (leading digit)
/// are dropped; everything else is a candidate the accounting core matches
/// against declared names.
fn template_mentions(template: &str) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    for word in template.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$')) {
        if word.is_empty() || word.as_bytes()[0].is_ascii_digit() {
            continue;
        }
        names.insert(word.to_string());
    }
    names
}

/// Identifier references (never declarations) in one setup script.
struct ScriptReferenceScan {
    names: FxHashSet<String>,
}

impl<'a> Visit<'a> for ScriptReferenceScan {
    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.names.insert(it.name.to_string());
        walk::walk_identifier_reference(self, it);
    }
}

/// Script identifier references by name, or empty when the script does not
/// parse (no synthetic references are invented for broken input).
fn script_reference_names(script: &str) -> FxHashSet<String> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, script, SourceType::ts().with_module(true)).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return FxHashSet::default();
    }
    let mut scan = ScriptReferenceScan {
        names: FxHashSet::default(),
    };
    scan.visit_program(&parsed.program);
    scan.names
}

/// Style `v-bind()` names: the only way authored style references script
/// bindings. Handles `v-bind(name)` and quoted `v-bind('name')` forms.
fn style_vbind_names(style: &str) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    let mut rest = style;
    while let Some(open) = rest.find("v-bind(") {
        rest = &rest[open + "v-bind(".len()..];
        let Some(close) = rest.find(')') else {
            break;
        };
        let candidate = rest[..close].trim().trim_matches(|c| c == '\'' || c == '"');
        if !candidate.is_empty()
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
            && !candidate.as_bytes()[0].is_ascii_digit()
        {
            names.insert(candidate.to_string());
        }
        rest = &rest[close + 1..];
    }
    names
}

/// Actual usage accounting over one declared name set.
///
/// Built only from authored references; the constructor never seeds
/// synthetic `void` reads, so a binding unused in every region stays
/// visible in [`BindingUsageSet::unused`] and `noUnusedLocals` /
/// `noUnusedParameters` keep firing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BindingUsageSet {
    /// Declared names in authored order.
    pub declared: Vec<String>,
    /// Actually referenced names in declared order.
    pub used: Vec<UsedBinding>,
}

impl BindingUsageSet {
    /// Account usage from authored references only. Reference names outside
    /// `declared` are ignored, never invented into the population.
    #[must_use]
    pub fn from_authored_references(
        declared: &[String],
        template: &[&str],
        script: &[&str],
        style: &[&str],
    ) -> Self {
        let in_region = |name: &str, region: &[&str]| region.contains(&name);
        let mut used = Vec::new();
        for name in declared {
            let regions = UsageRegions {
                template: in_region(name, template),
                script: in_region(name, script),
                style: in_region(name, style),
            };
            if regions.template || regions.script || regions.style {
                used.push(UsedBinding {
                    name: name.clone(),
                    regions,
                });
            }
        }
        Self {
            declared: declared.to_vec(),
            used,
        }
    }

    /// True exactly when `name` has an authored reference in any region.
    #[must_use]
    pub fn is_used(&self, name: &str) -> bool {
        self.used.iter().any(|binding| binding.name == name)
    }

    /// Account usage from authored region text instead of caller-supplied
    /// slices: template markup mentions, script identifier references, and
    /// style `v-bind()` names. Template and style collection is a lexical
    /// mention scan over authored bytes (strings and comments may
    /// over-approximate; undeclared words are still ignored, never
    /// invented into the population). Script collection walks the parsed
    /// AST for identifier references, so declarations never count as uses.
    /// An unparseable script contributes no script references rather than
    /// synthetic ones.
    #[must_use]
    pub fn from_region_text(
        declared: &[String],
        template: &str,
        script: &str,
        style: &str,
    ) -> Self {
        let template_owned = template_mentions(template);
        let script_owned = script_reference_names(script);
        let style_owned = style_vbind_names(style);
        let template_refs: Vec<&str> = template_owned.iter().map(String::as_str).collect();
        let script_refs: Vec<&str> = script_owned.iter().map(String::as_str).collect();
        let style_refs: Vec<&str> = style_owned.iter().map(String::as_str).collect();
        Self::from_authored_references(declared, &template_refs, &script_refs, &style_refs)
    }

    /// Declared names with no authored reference in any region, in
    /// declared order.
    #[must_use]
    pub fn unused(&self) -> Vec<&str> {
        let used: FxHashSet<&str> = self
            .used
            .iter()
            .map(|binding| binding.name.as_str())
            .collect();
        self.declared
            .iter()
            .filter(|name| !used.contains(name.as_str()))
            .map(String::as_str)
            .collect()
    }
}

/// The owned STP15 product pair: live read rows plus checked write rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BindingViewsProjection {
    /// Template-visible read shape.
    pub read: TemplateReadView,
    /// Template-visible write shape.
    pub write: TemplateWriteTarget,
}

/// Vue reactivity factories recognised at setup scope by binding (not by
/// bare spelling). The membership is Vue's public Composition API surface
/// (`ref`, `computed`, `reactive`, `readonly`, `toRefs`) plus the
/// `<script setup>` compile-time macros (`defineProps`, `defineModel`,
/// `withDefaults`); each entry is checked against an actual `from 'vue'`
/// import or a free reference by [`factory_export`], so the table is
/// verified against the parsed imports rather than trusted on spelling.
const FACTORY_EXPORTS: [&str; 8] = [
    "ref",
    "computed",
    "reactive",
    "readonly",
    "defineModel",
    "toRefs",
    "defineProps",
    "withDefaults",
];

/// Runtime (non-type-only) `'vue'` imports as local name to exported symbol.
fn vue_runtime_imports<'a>(program: &'a Program<'a>) -> FxHashMap<&'a str, &'a str> {
    let mut names = FxHashMap::default();
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else {
            continue;
        };
        if import.import_kind.is_type() || import.source.value != "vue" {
            continue;
        }
        let Some(specifiers) = &import.specifiers else {
            continue;
        };
        for specifier in specifiers {
            if let ImportDeclarationSpecifier::ImportSpecifier(spec) = specifier {
                if spec.import_kind.is_type() {
                    continue;
                }
                names.insert(spec.local.name.as_str(), imported_name(spec));
            }
        }
    }
    names
}

/// Exported name a runtime (non-type-only) `'vue'` import specifier binds.
fn imported_name<'a>(spec: &'a oxc_ast::ast::ImportSpecifier<'a>) -> &'a str {
    match &spec.imported {
        oxc_ast::ast::ModuleExportName::IdentifierName(name) => name.name.as_str(),
        oxc_ast::ast::ModuleExportName::IdentifierReference(name) => name.name.as_str(),
        oxc_ast::ast::ModuleExportName::StringLiteral(literal) => literal.value.as_str(),
    }
}

/// Resolve a call callee to the Vue factory/macro export it names, or
/// `None` when the callee is not a recognised export or is shadowed by a
/// setup-local value binding (which makes the call ordinary).
fn factory_export<'a>(
    callee: &'a Expression<'a>,
    values: &FxHashSet<String>,
    vue_imports: &FxHashMap<&'a str, &'a str>,
) -> Option<&'a str> {
    let Expression::Identifier(identifier) = callee else {
        return None;
    };
    let local = identifier.name.as_str();
    match vue_imports.get(local) {
        Some(exported) if FACTORY_EXPORTS.contains(exported) => Some(exported),
        Some(_) => None,
        None if !values.contains(local) && FACTORY_EXPORTS.contains(&local) => Some(local),
        None => None,
    }
}

fn first_argument<'a>(call: &'a oxc_ast::ast::CallExpression<'a>) -> Option<&'a Expression<'a>> {
    call.arguments.first().and_then(|argument| {
        if matches!(argument, Argument::SpreadElement(_)) {
            None
        } else {
            argument.as_expression()
        }
    })
}

/// Property value for `key` in an object expression, when the property is
/// a plain (non-computed) entry.
fn object_property<'a>(
    object: &'a oxc_ast::ast::ObjectExpression<'a>,
    key: &str,
) -> Option<&'a Expression<'a>> {
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            continue;
        };
        let matches = match &property.key {
            PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str() == key,
            PropertyKey::StringLiteral(literal) => literal.value.as_str() == key,
            _ => false,
        };
        if matches && !property.computed {
            return Some(&property.value);
        }
    }
    None
}

/// Slice the setter parameter annotation of a writable-computed options
/// object: `computed({ get, set(v: Domain) {} })` yields `Some("Domain")`.
/// Unannotated setters yield `None`; the domain is then recorded empty,
/// never defaulted to the read type.
fn setter_domain(content: &str, object: &oxc_ast::ast::ObjectExpression<'_>) -> Option<String> {
    let set = object_property(object, "set")?;
    let params = match set {
        Expression::ArrowFunctionExpression(arrow) => Some(arrow.params.as_ref()),
        Expression::FunctionExpression(function) => Some(function.params.as_ref()),
        _ => return None,
    };
    let first = params?.items.first()?;
    let annotation = first.type_annotation.as_ref()?;
    let span = annotation.type_annotation.span();
    let (start, end) = (span.start as usize, span.end as usize);
    // The span comes from this same parse of `content`, so an
    // out-of-bounds slice is an internal invariant break, never a
    // plausible empty domain: assert instead of clamping to `""`.
    assert!(
        start <= end && end <= content.len(),
        "setter annotation span {start}..{end} escapes the parsed setup block"
    );
    Some(content[start..end].to_string())
}

/// Names bound by a binding pattern, with each name's span relative to
/// `base` (the carrier offset of the block content).
fn pattern_names(pattern: &BindingPattern<'_>, base: u32) -> Vec<(String, SourceRange)> {
    let mut names = Vec::new();
    match pattern {
        BindingPattern::BindingIdentifier(identifier) => names.push((
            identifier.name.to_string(),
            SourceRange {
                start: base + identifier.span.start,
                end: base + identifier.span.end,
            },
        )),
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                names.extend(pattern_names(&property.value, base));
            }
            if let Some(rest) = &object.rest {
                names.extend(pattern_names(&rest.argument, base));
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                names.extend(pattern_names(element, base));
            }
            if let Some(rest) = &array.rest {
                names.extend(pattern_names(&rest.argument, base));
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            names.extend(pattern_names(&assignment.left, base));
        }
    }
    names
}

/// Push one read row plus its write shape. `computed_domain` carries the
/// declared setter domain for writable computed values; other kinds ignore
/// it. Writable computed rows always record the declared domain (empty
/// when unannotated), never the read type. `mutable` is false for `const`
/// declarators: an immutable lexical plain (object literals holding nested
/// refs, reactive destructuring copies, other non-call initializers)
/// keeps its direct read row but is refused as a write target, so template
/// reassignment of a `const` name cannot typecheck. A name that already
/// has a read row is left untouched: the earliest row wins, so a later
/// partially bound destructuring pattern cannot duplicate or override an
/// earlier readonly classification with a conflicting writable row.
fn push_binding(
    projection: &mut BindingViewsProjection,
    name: String,
    kind: BindingKind,
    range: SourceRange,
    computed_domain: Option<String>,
    mutable: bool,
) {
    if projection.read.lookup(&name).is_some() {
        return;
    }
    let (unwrapped, script_wraps_ref) = match &kind {
        BindingKind::Ref | BindingKind::Model => (true, true),
        BindingKind::Computed { .. }
        | BindingKind::Reactive
        | BindingKind::Readonly
        | BindingKind::Plain => (false, false),
    };
    projection.read.bindings.push(TemplateReadBinding {
        name: name.clone(),
        kind: kind.clone(),
        unwrapped,
        script_wraps_ref,
        range,
    });
    match kind {
        BindingKind::Ref => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::RefValue,
        }),
        BindingKind::Computed { setter: true } => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::SetterParam(computed_domain.unwrap_or_default()),
        }),
        BindingKind::Computed { setter: false } => projection.write.getter_only.push(name),
        BindingKind::Reactive => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::ReactiveMember,
        }),
        BindingKind::Model => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::ModelValue,
        }),
        BindingKind::Readonly => projection.write.readonly.push(name),
        BindingKind::Plain if mutable => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::PlainAssign,
        }),
        // Immutable lexical plains keep the direct read row above but are
        // refused here: reassigning a `const` name must not typecheck.
        BindingKind::Plain => projection.write.immutable.push(name),
    }
}

/// Push one read row with no write shape. Function declarations and ES
/// module imports are template-visible reads but immutable in scope
/// (TS2588/TS2630): they resolve through [`TemplateWriteTarget`] as
/// [`WriteRejection::UnknownBinding`], never as a writable assignment
/// target.
fn push_read_only(projection: &mut BindingViewsProjection, name: String, range: SourceRange) {
    if projection.read.lookup(&name).is_some() {
        return;
    }
    projection.read.bindings.push(TemplateReadBinding {
        name,
        kind: BindingKind::Plain,
        unwrapped: false,
        script_wraps_ref: false,
        range,
    });
}

/// Resolve a `defineProps` call, unwrapping the `withDefaults(...)`
/// outer call to the inner `defineProps<Props>()` call that carries the
/// type arguments. Returns `None` for any other initializer.
fn resolve_props_call<'a>(
    call: &'a oxc_ast::ast::CallExpression<'a>,
    values: &FxHashSet<String>,
    vue_imports: &FxHashMap<&str, &str>,
) -> Option<&'a oxc_ast::ast::CallExpression<'a>> {
    if factory_export(&call.callee, values, vue_imports) == Some("withDefaults") {
        // `withDefaults(defineProps<Props>(), ...)` binds props: return
        // the inner call so its type arguments stay visible.
        first_argument(call).and_then(|first| match first {
            Expression::CallExpression(inner)
                if factory_export(&inner.callee, values, vue_imports) == Some("defineProps") =>
            {
                Some(inner.as_ref())
            }
            _ => None,
        })
    } else if factory_export(&call.callee, values, vue_imports) == Some("defineProps") {
        Some(call)
    } else {
        None
    }
}

/// Shared context for [`classify_declarator`]: the parsed block text,
/// its carrier base offset, the setup value bindings, the runtime `'vue'`
/// imports, and whether the declarator is mutable (`let`/`var`, not `const`).
struct DeclaratorCtx<'a> {
    content: &'a str,
    base: u32,
    values: &'a FxHashSet<String>,
    vue_imports: &'a FxHashMap<&'a str, &'a str>,
    mutable: bool,
}

/// Classify one setup declarator initializer into read/write rows.
/// `ctx.mutable` is false for `const` declarators (only the
/// [`BindingKind::Plain`] write shape consults it; refs, computed values,
/// reactive objects, models and readonly rows carry their own mutability).
fn classify_declarator(
    projection: &mut BindingViewsProjection,
    ctx: &DeclaratorCtx<'_>,
    id: &BindingPattern<'_>,
    init: Option<&Expression<'_>>,
) {
    // Names already carrying a read row keep it: a partially bound
    // destructuring pattern only adds its unbound members.
    let names: Vec<(String, SourceRange)> = pattern_names(id, ctx.base)
        .into_iter()
        .filter(|(name, _)| projection.read.lookup(name).is_none())
        .collect();
    if names.is_empty() {
        return;
    }
    let Some(Expression::CallExpression(call)) = init else {
        // Object literals (possibly holding nested refs), arrays and every
        // other initializer read directly: only top-level `ref` unwraps.
        for (name, range) in names {
            push_binding(
                projection,
                name,
                BindingKind::Plain,
                range,
                None,
                ctx.mutable,
            );
        }
        return;
    };
    // `withDefaults(defineProps<...>(), ...)` binds props, not a plain.
    if let Some(props_call) = resolve_props_call(call, ctx.values, ctx.vue_imports) {
        collect_props_call(projection, ctx.base, props_call, &names, id);
        return;
    }
    match factory_export(&call.callee, ctx.values, ctx.vue_imports) {
        Some("ref") => {
            // Top-level `ref` stays a ref: the template unwraps the member.
            for (name, range) in names {
                push_binding(projection, name, BindingKind::Ref, range, None, ctx.mutable);
            }
        }
        Some("toRefs") => {
            // `toRefs(state)` returns an object whose members are refs: only
            // destructured members (`const { a } = toRefs(state)`) are
            // template-unwrapped refs. The whole returned object
            // (`const refs = toRefs(state)`) reads directly like any other
            // object holding nested refs.
            let whole = matches!(id, BindingPattern::BindingIdentifier(_));
            for (name, range) in names {
                let kind = if whole {
                    BindingKind::Plain
                } else {
                    BindingKind::Ref
                };
                push_binding(projection, name, kind, range, None, ctx.mutable);
            }
        }
        Some("computed") => match first_argument(call) {
            Some(Expression::ObjectExpression(object)) => {
                let has_set = object_property(object, "set").is_some();
                let domain = setter_domain(ctx.content, object);
                for (name, range) in names {
                    push_binding(
                        projection,
                        name,
                        BindingKind::Computed { setter: has_set },
                        range,
                        domain.clone(),
                        ctx.mutable,
                    );
                }
            }
            // Any `computed(...)` without an options object is a
            // getter-only `ComputedRef`: non-literal getters (identifier
            // references, call results) are never plain writable rows.
            _ => {
                for (name, range) in names {
                    push_binding(
                        projection,
                        name,
                        BindingKind::Computed { setter: false },
                        range,
                        None,
                        ctx.mutable,
                    );
                }
            }
        },
        Some("reactive") => {
            for (name, range) in names {
                // Plain destructuring out of `reactive(...)` copies values;
                // only the whole object keeps reactive member writes.
                let kind = match id {
                    BindingPattern::BindingIdentifier(_) => BindingKind::Reactive,
                    _ => BindingKind::Plain,
                };
                push_binding(projection, name, kind, range, None, ctx.mutable);
            }
        }
        Some("readonly") => {
            for (name, range) in names {
                push_binding(
                    projection,
                    name,
                    BindingKind::Readonly,
                    range,
                    None,
                    ctx.mutable,
                );
            }
        }
        Some("defineModel") => {
            for (name, range) in names {
                push_binding(
                    projection,
                    name,
                    BindingKind::Model,
                    range,
                    None,
                    ctx.mutable,
                );
            }
        }
        _ => {
            for (name, range) in names {
                push_binding(
                    projection,
                    name,
                    BindingKind::Plain,
                    range,
                    None,
                    ctx.mutable,
                );
            }
        }
    }
}

/// Declared prop names with their source ranges: runtime object entries,
/// array-form string literals, plus type-literal members. Empty for an
/// aliased props type (`defineProps<Props>()`), whose members TypeScript
/// owns.
fn extract_props(base: u32, call: &oxc_ast::ast::CallExpression<'_>) -> Vec<(String, SourceRange)> {
    let mut props: Vec<(String, SourceRange)> = Vec::new();
    // Runtime object syntax (`defineProps({ title: String, ... })`):
    // each entry key is a declared prop, mirroring the Options
    // `props: {...}` object entries.
    if let Some(Expression::ObjectExpression(object)) = first_argument(call) {
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            if property.computed {
                continue;
            }
            let name = match &property.key {
                PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.to_string()),
                PropertyKey::StringLiteral(literal) => Some(literal.value.to_string()),
                _ => None,
            };
            if let Some(name) = name {
                let span = property.key.span();
                props.push((
                    name,
                    SourceRange {
                        start: base + span.start,
                        end: base + span.end,
                    },
                ));
            }
        }
    }
    if let Some(Expression::ArrayExpression(array)) = first_argument(call) {
        for element in &array.elements {
            let Some(element) = element.as_expression() else {
                continue;
            };
            if let Expression::StringLiteral(literal) = element {
                props.push((
                    literal.value.to_string(),
                    SourceRange {
                        start: base + literal.span.start + 1,
                        end: base + literal.span.end - 1,
                    },
                ));
            }
        }
    }
    if let Some(type_arguments) = &call.type_arguments {
        for argument in &type_arguments.params {
            if let TSType::TSTypeLiteral(literal) = argument {
                for member in &literal.members {
                    let TSSignature::TSPropertySignature(signature) = member else {
                        continue;
                    };
                    let name = match &signature.key {
                        PropertyKey::StaticIdentifier(identifier) => {
                            Some(identifier.name.to_string())
                        }
                        PropertyKey::StringLiteral(literal) => Some(literal.value.to_string()),
                        _ => None,
                    };
                    if let Some(name) = name {
                        let span = signature.key.span();
                        props.push((
                            name,
                            SourceRange {
                                start: base + span.start,
                                end: base + span.end,
                            },
                        ));
                    }
                }
            }
        }
    }
    props
}

/// Declared prop key to bound local name: `const { title: heading }`
/// maps `title` to the local `heading`; shorthand members map to
/// themselves. Non-object patterns contribute identity pairs.
fn props_aliases(id: &BindingPattern<'_>, base: u32) -> Vec<(String, String, SourceRange)> {
    let BindingPattern::ObjectPattern(object) = id else {
        return pattern_names(id, base)
            .into_iter()
            .map(|(name, range)| (name.clone(), name, range))
            .collect();
    };
    let mut aliases = Vec::new();
    for property in &object.properties {
        if property.computed {
            continue;
        }
        let key = match &property.key {
            PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.to_string()),
            PropertyKey::StringLiteral(literal) => Some(literal.value.to_string()),
            _ => None,
        };
        let Some(key) = key else {
            continue;
        };
        // Renamed members with defaults (`{ title: heading = "x" }`) nest
        // the local inside an assignment pattern: unwrap to the target so
        // the declared prop key still maps to the bound local.
        let mut target = &property.value;
        while let BindingPattern::AssignmentPattern(assignment) = target {
            target = &assignment.left;
        }
        if let BindingPattern::BindingIdentifier(local) = target {
            aliases.push((
                key,
                local.name.to_string(),
                SourceRange {
                    start: base + local.span.start,
                    end: base + local.span.end,
                },
            ));
        } else {
            for (name, range) in pattern_names(&property.value, base) {
                aliases.push((name.clone(), name, range));
            }
        }
    }
    if let Some(rest) = &object.rest {
        for (name, range) in pattern_names(&rest.argument, base) {
            aliases.push((name.clone(), name, range));
        }
    }
    aliases
}

/// Record `defineProps` rows: array-form and type-literal members as
/// readonly prop rows under their bound local names (renamed members
/// like `title: heading` register `heading`); a whole-object `props`
/// binding as one readonly object row; an aliased props type
/// (`defineProps<Props>()`) as direct readonly reads of the destructured
/// names (TypeScript owns the members).
fn collect_props_call(
    projection: &mut BindingViewsProjection,
    base: u32,
    call: &oxc_ast::ast::CallExpression<'_>,
    bound: &[(String, SourceRange)],
    id: &BindingPattern<'_>,
) {
    let props = extract_props(base, call);
    if props.is_empty() {
        for (name, range) in bound {
            push_binding(
                projection,
                name.clone(),
                BindingKind::Readonly,
                *range,
                None,
                true,
            );
        }
        return;
    }
    if bound.len() == 1 && !props.iter().any(|(name, _)| name == &bound[0].0) {
        push_binding(
            projection,
            bound[0].0.clone(),
            BindingKind::Readonly,
            bound[0].1,
            None,
            true,
        );
        return;
    }
    let aliases = props_aliases(id, base);
    for (name, _) in props {
        if let Some((_, local, range)) = aliases.iter().find(|(key, _, _)| key == &name) {
            push_binding(
                projection,
                local.clone(),
                BindingKind::Readonly,
                *range,
                None,
                true,
            );
        }
    }
}

/// Record a standalone `defineProps<...>();` (or
/// `withDefaults(defineProps<...>(), ...);`) expression statement: with
/// no declarator the declared props themselves are the template rows.
fn collect_standalone_props_call(
    projection: &mut BindingViewsProjection,
    base: u32,
    call: &oxc_ast::ast::CallExpression<'_>,
) {
    for (name, range) in extract_props(base, call) {
        push_binding(projection, name, BindingKind::Readonly, range, None, true);
    }
}

/// Source type for one setup block; absent `lang` is Vue's JavaScript
/// default and is refused, matching the setup authority.
fn setup_source_type(lang: Option<ScriptLanguage>) -> Result<SourceType, SetupProjectionRefusal> {
    match lang {
        Some(ScriptLanguage::TypeScript) => Ok(SourceType::ts().with_module(true)),
        Some(ScriptLanguage::TSX) => Ok(SourceType::tsx().with_module(true)),
        None | Some(_) => Err(SetupProjectionRefusal::NotTypeScript),
    }
}

/// Project live read/write views for one SFC script pair. Options members
/// come from the single combined authority; setup initializer detail comes
/// from one parse of the setup block under its own grammar.
pub fn project_binding_views(
    normal: Option<ScriptBlockInput<'_>>,
    setup: Option<ScriptBlockInput<'_>>,
    generic: Option<&str>,
) -> Result<BindingViewsProjection, SetupProjectionRefusal> {
    if let (Some(n), Some(s)) = (&normal, &setup) {
        if n.lang != s.lang {
            return Err(SetupProjectionRefusal::ScriptLangConflict);
        }
    }
    let combined = project_options_pair(normal, setup, generic)?;
    let mut projection = BindingViewsProjection::default();
    // Setup rows win over Options rows for the same name (setup-first
    // order, earliest row kept): classify `<script setup>` before
    // recording Options members, which skip names already bound.
    if let Some(block) = setup {
        let source_type = setup_source_type(block.lang)?;
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, block.content, source_type).parse();
        if parsed.panicked || !parsed.errors.is_empty() {
            return Err(SetupProjectionRefusal::SyntaxErrors { setup: true });
        }
        let program = &parsed.program;
        let semantic = oxc_semantic::SemanticBuilder::new().build(program).semantic;
        let values = value_bindings(semantic.scoping());
        let vue_imports = vue_runtime_imports(program);
        classify_setup_body(
            &mut projection,
            block.content,
            block.content_start,
            program,
            &values,
            &vue_imports,
        );
    }
    project_options_members(&mut projection, &combined);
    Ok(projection)
}

/// Record Options members (props, computed, methods) as template rows.
/// Emits stay events, not template identifiers; `data()`/`setup()`/
/// mixins/extends members are runtime-known and stay opaque.
fn project_options_members(
    projection: &mut BindingViewsProjection,
    combined: &CombinedScriptProjection,
) {
    let Some(options) = &combined.options else {
        return;
    };
    for member in &options.members {
        if projection.read.lookup(&member.name).is_some() {
            continue;
        }
        let range = member.range;
        match member.kind {
            OptionsMemberKind::Prop => {
                push_binding(
                    projection,
                    member.name.clone(),
                    BindingKind::Readonly,
                    range,
                    None,
                    true,
                );
            }
            OptionsMemberKind::Computed { setter } => {
                push_binding(
                    projection,
                    member.name.clone(),
                    BindingKind::Computed { setter },
                    range,
                    None,
                    true,
                );
            }
            // Options methods are callable reads, never writable
            // assignment targets (mirrors setup function declarations).
            OptionsMemberKind::Method => {
                push_read_only(projection, member.name.clone(), range);
            }
            OptionsMemberKind::Emit
            | OptionsMemberKind::Component
            | OptionsMemberKind::Directive => {}
        }
    }
}

/// Walk top-level setup statements once, classifying declarators and
/// `defineProps` calls. Nested function bodies are never entered: only
/// top-level bindings are template-visible. Setup rows win over Options
/// rows for the same name (setup-first order, earliest row kept).
fn classify_setup_body(
    projection: &mut BindingViewsProjection,
    content: &str,
    base: u32,
    program: &Program<'_>,
    values: &FxHashSet<String>,
    vue_imports: &FxHashMap<&str, &str>,
) {
    for statement in &program.body {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                // Only `let`/`var` declarators bind mutable plains; `const`
                // plains keep their read row but refuse writes.
                let ctx = DeclaratorCtx {
                    content,
                    base,
                    values,
                    vue_imports,
                    mutable: !matches!(
                        declaration.kind,
                        oxc_ast::ast::VariableDeclarationKind::Const
                    ),
                };
                for declarator in &declaration.declarations {
                    if declarator_bound(projection, &declarator.id) {
                        continue;
                    }
                    classify_declarator(projection, &ctx, &declarator.id, declarator.init.as_ref());
                }
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(id) = &function.id {
                    push_read_only(
                        projection,
                        id.name.to_string(),
                        SourceRange {
                            start: base + id.span.start,
                            end: base + id.span.end,
                        },
                    );
                }
            }
            Statement::ImportDeclaration(import) => {
                let Some(specifiers) = &import.specifiers else {
                    continue;
                };
                for specifier in specifiers {
                    let (local, span) = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(spec) => {
                            (spec.local.name.as_str(), spec.local.span)
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(spec) => {
                            (spec.local.name.as_str(), spec.local.span)
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(spec) => {
                            (spec.local.name.as_str(), spec.local.span)
                        }
                    };
                    push_read_only(
                        projection,
                        local.to_string(),
                        SourceRange {
                            start: base + span.start,
                            end: base + span.end,
                        },
                    );
                }
            }
            Statement::ExpressionStatement(statement) => {
                // Idiomatic `<script setup>` declares props without a
                // script-side reference: a standalone
                // `defineProps<...>();` still binds template rows.
                if let Expression::CallExpression(call) = &statement.expression {
                    if let Some(props_call) = resolve_props_call(call, values, vue_imports) {
                        collect_standalone_props_call(projection, base, props_call);
                    }
                }
            }
            _ => {}
        }
    }
}

/// True when every name bound by `id` already has a read row.
fn declarator_bound(projection: &BindingViewsProjection, id: &BindingPattern<'_>) -> bool {
    let names = pattern_names(id, 0);
    !names.is_empty()
        && names
            .iter()
            .all(|(name, _)| projection.read.lookup(name).is_some())
}
