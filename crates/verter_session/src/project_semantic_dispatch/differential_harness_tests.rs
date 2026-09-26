//! The differential lane the `differential_*` tests compare against the
//! checker: one host carrying the four `strictNullChecks` × `noImplicitAny`
//! projects, a probe module upserted verbatim into each, and every row read
//! back through the public audited flow-return boundary, reduced to the
//! altitude the checker prints and spelled as the checker prints it.
//!
//! A row is either a TYPE probe — answered as the return of a generated
//! function whose body returns a local declared as the probe — or a
//! function of the module, answered as its body-derived return. A
//! degraded, refused, partial, panicking or overdue answer is a failure of
//! the row, never a comparable value, and every failure names its class:
//! `WRONG-CLEAN` when the lane published a complete, undegraded answer the
//! checker does not give, `GAP` otherwise.
//!
//! The comparison is over a canonical spelling of both sides: union and
//! intersection members and object members are sorted, `true | false`
//! folds to `boolean`, and an optional tuple element's printed
//! `| undefined` is dropped, so the checker's own print is the expected
//! column.

use std::panic::AssertUnwindSafe;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    LiteralValue, ProjectionMode, ProjectionReductionContext, ReturnProjectionDemand,
    SemanticNodeData, SemanticNodeId, SignatureKind,
};
use crate::types::HostConfig;
use crate::VerterHost;

/// One project of the matrix: its root and its `compilerOptions`.
#[derive(Clone, Copy)]
pub(super) struct Setting {
    pub(super) root: &'static str,
    pub(super) label: &'static str,
    options: &'static str,
}

/// `strict`.
pub(super) const STRICT: Setting = Setting {
    root: "/strict",
    label: "strict",
    options: r#"{ "strict": true }"#,
};
/// `strict` with `strictNullChecks` off.
pub(super) const LOOSE: Setting = Setting {
    root: "/loose",
    label: "strictNullChecks off",
    options: r#"{ "strict": true, "strictNullChecks": false }"#,
};
/// `strict` with `noImplicitAny` off.
pub(super) const STRICT_IMPLICIT: Setting = Setting {
    root: "/strict-implicit",
    label: "noImplicitAny off",
    options: r#"{ "strict": true, "noImplicitAny": false }"#,
};
/// `strict` with both off.
pub(super) const LOOSE_IMPLICIT: Setting = Setting {
    root: "/loose-implicit",
    label: "both off",
    options: r#"{ "strict": true, "strictNullChecks": false, "noImplicitAny": false }"#,
};

/// The four settings, in the order a four-answer row lists them.
pub(super) const ALL: [Setting; 4] = [STRICT, LOOSE, STRICT_IMPLICIT, LOOSE_IMPLICIT];

/// How long one row may take before it is reported overdue.
const ROW_DEADLINE: Duration = Duration::from_secs(60);

/// What a row reads.
#[derive(Clone, Copy)]
pub(super) enum Read<'a> {
    /// A type in TYPE position.
    Type(&'a str),
    /// The body-derived return of a function of the module.
    Return(&'a str),
}

impl Read<'_> {
    fn text(&self) -> &str {
        match self {
            Read::Type(text) | Read::Return(text) => text,
        }
    }
}

/// A probe module checked in the given settings, with an optional ambient
/// library registered against every project.
pub(super) struct Matrix<'a> {
    settings: &'a [Setting],
    source: &'a str,
    lib: Option<&'a str>,
    script: bool,
    files: &'a [(&'a str, &'a str)],
}

impl<'a> Matrix<'a> {
    /// A module of `source` checked in every setting.
    pub(super) fn new(source: &'a str) -> Self {
        Self {
            settings: &ALL,
            source,
            lib: None,
            script: false,
            files: &[],
        }
    }

    /// With `(file name, source)` modules beside the probe module in
    /// each project.
    pub(super) fn files(mut self, files: &'a [(&'a str, &'a str)]) -> Self {
        self.files = files;
        self
    }

    /// The source is a global script: no `export {}` is appended to it.
    pub(super) fn script(mut self) -> Self {
        self.script = true;
        self
    }

    /// With `lib` registered as each project's library.
    pub(super) fn lib(mut self, lib: &'a str) -> Self {
        self.lib = Some(lib);
        self
    }

    /// Every `(read, checker print)` row that answers the same in every
    /// setting, as the failure list.
    pub(super) fn same(&self, rows: &[(Read<'_>, &str)]) -> Vec<String> {
        let rows: Vec<(Read<'_>, Vec<&str>)> = rows
            .iter()
            .map(|(read, answer)| (*read, vec![*answer; self.settings.len()]))
            .collect();
        self.run(&rows)
    }

    /// Every `(type probe, checker print)` row answering the same in every
    /// setting, as the failure list.
    pub(super) fn types(&self, rows: &[(&str, &str)]) -> Vec<String> {
        let rows: Vec<(Read<'_>, &str)> = rows
            .iter()
            .map(|(probe, answer)| (Read::Type(probe), *answer))
            .collect();
        self.same(&rows)
    }

    /// Every `(function, checker print)` row answering the same in every
    /// setting, as the failure list.
    pub(super) fn returns(&self, rows: &[(&str, &str)]) -> Vec<String> {
        let rows: Vec<(Read<'_>, &str)> = rows
            .iter()
            .map(|(function, answer)| (Read::Return(function), *answer))
            .collect();
        self.same(&rows)
    }

    /// Every `(read, strict, strictNullChecks off, noImplicitAny off, both
    /// off)` row, as the failure list (the matrix must hold all four
    /// settings).
    pub(super) fn four(&self, rows: &[(Read<'_>, &str, &str, &str, &str)]) -> Vec<String> {
        assert_eq!(
            self.settings.len(),
            4,
            "a four-answer row needs all four settings"
        );
        let rows: Vec<(Read<'_>, Vec<&str>)> = rows
            .iter()
            .map(|(read, a, b, c, d)| (*read, vec![*a, *b, *c, *d]))
            .collect();
        self.run(&rows)
    }

    /// Every `(read, strict answer, strictNullChecks-off answer)` row, the
    /// `noImplicitAny`-off settings answering as their `strictNullChecks`
    /// sibling.
    pub(super) fn nullness(&self, rows: &[(Read<'_>, &str, &str)]) -> Vec<String> {
        let rows: Vec<(Read<'_>, &str, &str, &str, &str)> = rows
            .iter()
            .map(|(read, strict, loose)| (*read, *strict, *loose, *strict, *loose))
            .collect();
        self.four(&rows)
    }

    fn run(&self, rows: &[(Read<'_>, Vec<&str>)]) -> Vec<String> {
        let mut failures = Vec::new();
        for ((read, answers), verdicts) in rows.iter().zip(self.verdicts(rows)) {
            for ((setting, expected), verdict) in self.settings.iter().zip(answers).zip(verdicts) {
                if !verdict.matched {
                    failures.push(format!(
                        "{} `{}` [{}]: the checker answers `{expected}`, {}",
                        verdict.class,
                        read.text(),
                        setting.label,
                        verdict.lane
                    ));
                }
            }
        }
        failures
    }

    /// Each row's verdict in each setting, in row then setting order.
    pub(super) fn verdicts(&self, rows: &[(Read<'_>, Vec<&str>)]) -> Vec<Vec<Verdict>> {
        let host = Arc::new(matrix_host(self.settings));
        let module = self.module(rows);
        for setting in self.settings {
            let canonical = format!("{}/probe.ts", setting.root);
            if let Some(lib) = self.lib {
                crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::register_lib_environment(
                    &host,
                    &canonical,
                    "lib.probe.d.ts",
                    lib,
                );
            }
            for (name, source) in self.files {
                let path = format!("{}/{name}", setting.root);
                // A sibling is classified by its name, as the host classifies
                // it: a `.d.ts` sibling is a declaration file.
                let language = crate::LanguageRegistry::global()
                    .classify_static(&path)
                    .static_resolution();
                crate::u6_flow_shape_corpus_tests::upsert(&host, &path, source, language);
            }
            crate::u6_flow_shape_corpus_tests::upsert(
                &host,
                &canonical,
                &module,
                crate::FileLanguage::script_ts(),
            );
        }
        rows.iter()
            .enumerate()
            .map(|(index, (read, answers))| {
                let symbol = match read {
                    Read::Type(_) => format!("__probe_{index}"),
                    Read::Return(function) => (*function).to_owned(),
                };
                self.settings
                    .iter()
                    .zip(answers)
                    .map(|(setting, expected)| {
                        let canonical = format!("{}/probe.ts", setting.root);
                        let observed =
                            observe_with_deadline(&host, &canonical, &symbol, self.lib.is_some());
                        let matched = match &observed {
                            Observed::Clean(text) => {
                                canonical_text(text) == canonical_text(expected)
                            }
                            _ => false,
                        };
                        Verdict {
                            matched,
                            class: observed.class(),
                            lane: observed.describe(),
                        }
                    })
                    .collect()
            })
            .collect()
    }

    fn module(&self, rows: &[(Read<'_>, Vec<&str>)]) -> String {
        let mut module = String::from(self.source);
        module.push('\n');
        // A global script's probes are not exported: an export would make it
        // a module.
        let export = if self.script { "" } else { "export " };
        for (index, (read, _)) in rows.iter().enumerate() {
            if let Read::Type(probe) = read {
                module.push_str(&format!(
                    "{export}function __probe_{index}() {{ const __p: {probe} = null as any; return __p; }}\n"
                ));
            }
        }
        if !self.script {
            module.push_str("export {};\n");
        }
        module
    }
}

/// One host carrying a tsconfig-backed project per setting.
fn matrix_host(settings: &[Setting]) -> VerterHost {
    let projects: Vec<(&str, String)> = settings
        .iter()
        .map(|setting| {
            (
                setting.root,
                format!(r#"{{ "compilerOptions": {} }}"#, setting.options),
            )
        })
        .collect();
    let projects: Vec<(&str, &str)> = projects
        .iter()
        .map(|(root, config)| (*root, config.as_str()))
        .collect();
    VerterHost::new_standalone_with_tsconfig_projects(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            audit_enabled: true,
            footprint_capture: false,
            ..HostConfig::default()
        },
        &projects,
    )
}

/// One row's comparison in one setting.
pub(super) struct Verdict {
    /// The lane's answer is the checker's.
    pub(super) matched: bool,
    /// `WRONG-CLEAN`, `GAP`, `PANIC` or `HANG` when it is not.
    pub(super) class: &'static str,
    /// What the lane gave.
    pub(super) lane: String,
}

/// What the lane gave for one row.
enum Observed {
    /// A complete, undegraded answer, spelled as the checker prints it.
    Clean(String),
    /// A value carrying a degradation.
    Degraded(String, String),
    /// A value reducing to a partial demand.
    Partial,
    /// No value.
    NoValue(String),
    /// The evaluation panicked.
    Panicked(String),
    /// The evaluation outran [`ROW_DEADLINE`].
    Overdue,
}

impl Observed {
    fn class(&self) -> &'static str {
        match self {
            Observed::Clean(text) if !text.contains('<') || is_generic_print(text) => "WRONG-CLEAN",
            Observed::Panicked(_) => "PANIC",
            Observed::Overdue => "HANG",
            _ => "GAP",
        }
    }

    fn describe(&self) -> String {
        match self {
            Observed::Clean(text) => format!("the lane measured `{text}`"),
            Observed::Degraded(text, degradation) => {
                format!("the lane measured `{text}` degraded by {degradation}")
            }
            Observed::Partial => "the lane reduced to a partial demand".to_owned(),
            Observed::NoValue(error) => format!("the lane produced no value: {error}"),
            Observed::Panicked(message) => format!("the lane panicked: {message}"),
            Observed::Overdue => format!("the lane took longer than {ROW_DEADLINE:?}"),
        }
    }
}

/// A clean print naming only generic applications (`Name<…>`), not a gap
/// marker (`<opaque …>`).
fn is_generic_print(text: &str) -> bool {
    !text.contains("<opaque")
        && !text.contains("<unreduced")
        && !text.contains("<unrendered")
        && !text.contains("<evicted")
}

fn observe_with_deadline(
    host: &Arc<VerterHost>,
    canonical: &str,
    symbol: &str,
    scoped: bool,
) -> Observed {
    let (sender, receiver) = mpsc::channel();
    let thread_host = Arc::clone(host);
    let canonical = canonical.to_owned();
    let symbol = symbol.to_owned();
    std::thread::spawn(move || {
        let observed = std::panic::catch_unwind(AssertUnwindSafe(|| {
            observe(&thread_host, &canonical, &symbol, scoped)
        }))
        .unwrap_or_else(|payload| {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_owned())
                })
                .unwrap_or_default();
            Observed::Panicked(message)
        });
        let _ = sender.send(observed);
    });
    receiver
        .recv_timeout(ROW_DEADLINE)
        .unwrap_or(Observed::Overdue)
}

fn observe(host: &VerterHost, canonical: &str, symbol: &str, scoped: bool) -> Observed {
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let carrier =
        host.get_flow_return_type_with_audit(&identity, ReturnProjectionDemand::whole_return());
    let result = match carrier.as_result() {
        Ok(result) => Arc::clone(result),
        Err(error) => return Observed::NoValue(format!("{error:?}")),
    };
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let _demand_scope = scoped.then(|| {
        super::LexicalDemandScopeGuard::push(&dispatch.lexical_demand_scope, Arc::from(canonical))
    });
    let Some(node) = dispatch
        .normalize_node_keeping_declaration_refs_for_tests(
            result.return_type(),
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .into_complete_node()
    else {
        return Observed::Partial;
    };
    let text = print(&dispatch, node, 0);
    match result.degradation() {
        None => Observed::Clean(text),
        Some(degradation) => Observed::Degraded(text, format!("{degradation:?}")),
    }
}

/// `node` as the checker prints it.
fn print(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId, depth: usize) -> String {
    if depth > 12 {
        return "<unrendered deep>".to_owned();
    }
    let graph = dispatch.graph();
    let Some(data) = graph.node_data(node) else {
        return "<evicted>".to_owned();
    };
    let at = |node: SemanticNodeId| print(dispatch, node, depth + 1);
    match data.as_ref() {
        SemanticNodeData::Primitive(kind) => match kind {
            crate::semantic_query::PrimitiveKind::BigInt => "bigint".to_owned(),
            other => format!("{other:?}").to_ascii_lowercase(),
        },
        SemanticNodeData::Literal(LiteralValue::String(value)) => format!("\"{value}\""),
        SemanticNodeData::Literal(LiteralValue::Number(value)) => format!("{value}"),
        SemanticNodeData::Literal(LiteralValue::Boolean(value)) => format!("{value}"),
        SemanticNodeData::Literal(LiteralValue::BigInt(value)) => format!("{value}n"),
        SemanticNodeData::EnumLiteral(literal) => literal.printed_name(),
        SemanticNodeData::Alias(inner) => at(*inner),
        SemanticNodeData::Union(members) => {
            crate::semantic_query::printed_union_arms(graph, members)
                .into_iter()
                .map(|arm| match arm {
                    crate::semantic_query::PrintedUnionArm::Node(member) => {
                        operand(dispatch, member, depth, true)
                    }
                    crate::semantic_query::PrintedUnionArm::Enum(decl) => {
                        decl.decl_name.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" | ")
        }
        SemanticNodeData::Intersection(members) => members
            .iter()
            .map(|member| operand(dispatch, *member, depth, false))
            .collect::<Vec<_>>()
            .join(" & "),
        SemanticNodeData::Object(surface) => {
            let mut members: Vec<String> = Vec::new();
            for signature in surface.call_signatures.iter() {
                members.push(format!(
                    "{};",
                    signature_text(dispatch, *signature, depth, ": ")
                ));
            }
            for signature in surface.construct_signatures.iter() {
                members.push(format!(
                    "{};",
                    signature_text(dispatch, *signature, depth, ": ")
                ));
            }
            for index in surface.index_signatures.iter() {
                members.push(format!(
                    "{}[x: {}]: {};",
                    if index.readonly { "readonly " } else { "" },
                    at(index.key_type),
                    at(index.value_type)
                ));
            }
            for member in surface.positive_members().iter() {
                let name = member.key.as_string().unwrap_or("<key>");
                let optional = if member.optional { "?" } else { "" };
                match member.method_kind {
                    Some(verter_type_expr::ObjectMethodKind::Method) => members.push(format!(
                        "{name}{optional}{};",
                        signature_text(dispatch, member.value, depth, ": ")
                    )),
                    _ => members.push(format!(
                        "{}{name}{optional}: {};",
                        if member.readonly { "readonly " } else { "" },
                        at(member.value)
                    )),
                }
            }
            if members.is_empty() {
                "{}".to_owned()
            } else {
                format!("{{ {} }}", members.join(" "))
            }
        }
        SemanticNodeData::Signature { .. } => signature_text(dispatch, node, depth, " => "),
        SemanticNodeData::Array { element, readonly } => format!(
            "{}{}[]",
            if *readonly { "readonly " } else { "" },
            operand(dispatch, *element, depth, false)
        ),
        SemanticNodeData::Tuple { elements, readonly } => {
            let elements: Vec<String> = elements
                .iter()
                .map(|element| {
                    let value = if element.optional && element.label.is_none() {
                        operand(dispatch, element.value, depth, false)
                    } else {
                        at(element.value)
                    };
                    match (&element.label, element.rest, element.optional) {
                        (Some(label), true, _) => format!("...{label}: {value}"),
                        (Some(label), false, true) => format!("{label}?: {value}"),
                        (Some(label), false, false) => format!("{label}: {value}"),
                        (None, true, _) => format!("...{value}"),
                        (None, false, true) => format!("{value}?"),
                        (None, false, false) => value,
                    }
                })
                .collect();
            format!(
                "{}[{}]",
                if *readonly { "readonly " } else { "" },
                elements.join(", ")
            )
        }
        SemanticNodeData::TemplateLiteral {
            quasis,
            expressions,
        } => {
            let mut text = String::from("`");
            for (index, quasi) in quasis.iter().enumerate() {
                text.push_str(quasi);
                if let Some(expression) = expressions.get(index) {
                    text.push_str(&format!("${{{}}}", at(*expression)));
                }
            }
            text.push('`');
            text
        }
        SemanticNodeData::KeyOf { base } => {
            format!("keyof {}", operand(dispatch, *base, depth, false))
        }
        SemanticNodeData::TypeParam { display_name, .. } => display_name.to_string(),
        SemanticNodeData::DeclRef { identity } => identity.decl_name.to_string(),
        SemanticNodeData::InstantiationRef { base, args } => {
            let args: Vec<String> = args.iter().map(|arg| at(*arg)).collect();
            format!("{}<{}>", base.decl_name, args.join(", "))
        }
        SemanticNodeData::ClassExpressionInstance {
            identity,
            type_arguments,
            ..
        } => identity.printed_name_in(graph, type_arguments),
        SemanticNodeData::IntrinsicApplication { op, args } => {
            let args: Vec<String> = args.iter().map(|arg| at(*arg)).collect();
            format!("{}<{}>", op.display_name(), args.join(", "))
        }
        SemanticNodeData::Opaque(error) => {
            let text: String = format!("{error:?}").chars().take(80).collect();
            format!("<opaque {text}>")
        }
        SemanticNodeData::Conditional { .. } => "<unreduced conditional>".to_owned(),
        SemanticNodeData::IndexedAccess { .. } => "<unreduced indexed access>".to_owned(),
        SemanticNodeData::Mapped { .. } => "<unreduced mapped>".to_owned(),
        other => {
            let text: String = format!("{other:?}").chars().take(60).collect();
            format!("<unrendered {text}>")
        }
    }
}

/// `node` printed as an operand of `|`, `&`, `[]` or `keyof`:
/// parenthesized as the checker parenthesizes it.
fn operand(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    depth: usize,
    in_union: bool,
) -> String {
    let text = print(dispatch, node, depth + 1);
    let needs = match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Signature { .. }) => true,
        // A union the checker prints as one enum name needs none.
        Some(SemanticNodeData::Union(members)) => {
            !in_union
                && crate::semantic_query::printed_union_arms(dispatch.graph(), members).len() > 1
        }
        Some(SemanticNodeData::Intersection(_)) => !in_union,
        Some(SemanticNodeData::KeyOf { .. }) => !in_union,
        _ => false,
    };
    if needs {
        format!("({text})")
    } else {
        text
    }
}

/// A signature as `<T>(a: A, b?: B) => R`, the arrow `arrow` (`: ` inside
/// an object type).
fn signature_text(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    depth: usize,
    arrow: &str,
) -> String {
    let graph = dispatch.graph();
    let Some(data) = graph.node_data(node) else {
        return "<evicted>".to_owned();
    };
    let SemanticNodeData::Signature {
        kind,
        params,
        return_type,
        type_parameters,
        predicate,
        ..
    } = data.as_ref()
    else {
        return print(dispatch, node, depth + 1);
    };
    let at = |node: SemanticNodeId| print(dispatch, node, depth + 1);
    let type_parameters = if type_parameters.is_empty() {
        String::new()
    } else {
        let names: Vec<String> = type_parameters
            .iter()
            .map(|parameter| {
                let mut text = String::new();
                if parameter.is_const {
                    text.push_str("const ");
                }
                text.push_str(&parameter.name);
                if let Some(constraint) = parameter.constraint {
                    text.push_str(&format!(" extends {}", at(constraint)));
                }
                if let Some(default) = parameter.default {
                    text.push_str(&format!(" = {}", at(default)));
                }
                text
            })
            .collect();
        format!("<{}>", names.join(", "))
    };
    let rendered: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(index, param)| {
            format!(
                "{}{}{}: {}",
                if param.rest { "..." } else { "" },
                param
                    .name
                    .as_deref()
                    .map_or_else(|| format!("arg{index}"), str::to_owned),
                if param.optional { "?" } else { "" },
                at(param.ty)
            )
        })
        .collect();
    let result = match predicate {
        Some(predicate) => {
            let subject = match predicate.subject {
                crate::semantic_query::PredicateSubject::This => "this".to_owned(),
                crate::semantic_query::PredicateSubject::Parameter(_) => predicate
                    .subject_parameter(params)
                    .and_then(|param| param.name.as_deref())
                    .unwrap_or("?")
                    .to_owned(),
            };
            format!(
                "{}{subject}{}",
                if predicate.asserts { "asserts " } else { "" },
                predicate
                    .ty
                    .map(|target| format!(" is {}", at(target)))
                    .unwrap_or_default()
            )
        }
        None => at(*return_type),
    };
    format!(
        "{}{type_parameters}({}){arrow}{result}",
        if *kind == SignatureKind::Construct {
            "new "
        } else {
            ""
        },
        rendered.join(", ")
    )
}

/// `text` with every union, intersection and object member list sorted,
/// `true | false` folded to `boolean`, and an optional tuple element's
/// `| undefined` dropped — the spelling both sides are compared in.
pub(super) fn canonical_text(text: &str) -> String {
    let text = text.trim();
    canonical_level(text)
}

/// Split `text` on `separator` at bracket depth zero, outside strings.
fn split_top(text: &str, separator: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut start = 0;
    let bytes: Vec<(usize, char)> = text.char_indices().collect();
    let mut index = 0;
    while index < bytes.len() {
        let (position, ch) = bytes[index];
        if let Some(q) = quote {
            if ch == '\\' {
                index += 2;
                continue;
            }
            if ch == q {
                quote = None;
            }
            index += 1;
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '>' if position > 0 && text[..position].ends_with('=') => {}
            '>' => depth -= 1,
            _ => {}
        }
        if depth == 0 && quote.is_none() && text[position..].starts_with(separator) {
            parts.push(text[start..position].trim().to_owned());
            start = position + separator.len();
            index += separator.chars().count();
            continue;
        }
        index += 1;
    }
    parts.push(text[start..].trim().to_owned());
    parts
}

/// The index of the first `=>` at depth zero, outside strings.
fn top_arrow(text: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut previous = '\0';
    for (position, ch) in text.char_indices() {
        if let Some(q) = quote {
            if ch == q && previous != '\\' {
                quote = None;
            }
            previous = ch;
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '>' if previous == '=' => {
                if depth == 0 {
                    return Some(position - 1);
                }
            }
            '>' => depth -= 1,
            _ => {}
        }
        previous = ch;
    }
    None
}

fn canonical_level(text: &str) -> String {
    let text = text.trim();
    // A function type's return extends to the end of the level.
    if let Some(arrow) = top_arrow(text) {
        let head = canonical_atom(text[..arrow].trim());
        let tail = canonical_level(&text[arrow + 2..]);
        return format!("{head} => {tail}");
    }
    let arms = split_top(text, " | ");
    if arms.len() > 1 {
        let mut arms: Vec<String> = arms.iter().map(|arm| canonical_level(arm)).collect();
        if arms.iter().any(|arm| arm == "true") && arms.iter().any(|arm| arm == "false") {
            arms.retain(|arm| arm != "true" && arm != "false");
            arms.push("boolean".to_owned());
        }
        arms.sort();
        arms.dedup();
        return arms.join(" | ");
    }
    let parts = split_top(text, " & ");
    if parts.len() > 1 {
        let mut parts: Vec<String> = parts.iter().map(|part| canonical_level(part)).collect();
        parts.sort();
        return parts.join(" & ");
    }
    canonical_atom(text)
}

fn canonical_atom(text: &str) -> String {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("readonly ") {
        return format!("readonly {}", canonical_atom(rest));
    }
    if let Some(rest) = text.strip_prefix("keyof ") {
        return format!("keyof {}", canonical_atom(rest));
    }
    if let Some(inner) = text.strip_suffix("[]") {
        return format!("{}[]", canonical_atom(inner));
    }
    if text.starts_with('(') && matching_close(text, 0) == Some(text.len() - 1) {
        let inner = &text[1..text.len() - 1];
        // A parameter list or a parenthesized type.
        if inner.contains(':') && !inner.trim_start().starts_with('(') && top_arrow(inner).is_none()
        {
            let params: Vec<String> = split_top(inner, ", ")
                .into_iter()
                .filter(|param| !param.is_empty())
                .map(|param| canonical_member(&param))
                .collect();
            return format!("({})", params.join(", "));
        }
        if inner.is_empty() {
            return "()".to_owned();
        }
        return format!("({})", canonical_level(inner));
    }
    if text.starts_with('[') && matching_close(text, 0) == Some(text.len() - 1) {
        let inner = &text[1..text.len() - 1];
        let elements: Vec<String> = split_top(inner, ", ")
            .into_iter()
            .filter(|element| !element.is_empty())
            .map(|element| canonical_tuple_element(&element))
            .collect();
        return format!("[{}]", elements.join(", "));
    }
    if text.starts_with('{') && matching_close(text, 0) == Some(text.len() - 1) {
        let inner = text[1..text.len() - 1].trim();
        let mut members: Vec<String> = split_top(inner, ";")
            .into_iter()
            .filter(|member| !member.is_empty())
            .map(|member| canonical_member(&member))
            .collect();
        members.sort();
        if members.is_empty() {
            return "{}".to_owned();
        }
        return format!("{{ {}; }}", members.join("; "));
    }
    if let Some(open) = text.find('<') {
        if text.ends_with('>')
            && !text.starts_with('<')
            && matching_close(text, open) == Some(text.len() - 1)
        {
            let args: Vec<String> = split_top(&text[open + 1..text.len() - 1], ", ")
                .into_iter()
                .map(|arg| canonical_level(&arg))
                .collect();
            return format!("{}<{}>", &text[..open], args.join(", "));
        }
    }
    text.to_owned()
}

fn canonical_tuple_element(element: &str) -> String {
    let element = element.trim();
    if let Some(rest) = element.strip_prefix("...") {
        return format!("...{}", canonical_member(rest));
    }
    if let Some(value) = element.strip_suffix('?') {
        // `(T | undefined)?` is the checker's print of the optional `T?`.
        let value = value.trim();
        let value = if value.starts_with('(') && matching_close(value, 0) == Some(value.len() - 1) {
            let arms: Vec<String> = split_top(&value[1..value.len() - 1], " | ")
                .into_iter()
                .filter(|arm| arm != "undefined")
                .collect();
            arms.join(" | ")
        } else {
            value.to_owned()
        };
        return format!("{}?", canonical_level(&value));
    }
    canonical_member(element)
}

/// A `name: T` / `name?: T` / method member, or a bare type.
fn canonical_member(member: &str) -> String {
    let member = member.trim();
    let parts = split_top(member, ": ");
    if parts.len() >= 2 && !parts[0].is_empty() && !parts[0].contains(' ')
        || parts.len() >= 2 && parts[0].starts_with("readonly ")
        || parts.len() >= 2 && parts[0].starts_with('[')
    {
        let name = parts[0].clone();
        // An index signature's parameter name is not part of its type.
        let name = match name
            .strip_prefix('[')
            .and_then(|inner| inner.strip_suffix(']'))
        {
            Some(inner) => match split_top(inner, ": ").get(1) {
                Some(key) => format!("[x: {}]", canonical_level(key)),
                None => canonical_atom(&name),
            },
            None => name,
        };
        let rest = member[parts[0].len() + 2..].to_owned();
        let (name, rest) = if let Some(optional) = name.strip_suffix('?') {
            // `name?: (T | undefined)` prints the optional member's own
            // `undefined`; both sides keep it.
            (format!("{optional}?"), rest)
        } else {
            (name, rest)
        };
        return format!("{name}: {}", canonical_level(&rest));
    }
    canonical_level(member)
}

/// The index of the bracket closing the one at `open`.
fn matching_close(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut previous = '\0';
    for (position, ch) in text
        .char_indices()
        .skip_while(|(position, _)| *position < open)
    {
        if let Some(q) = quote {
            if ch == q && previous != '\\' {
                quote = None;
            }
            previous = ch;
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '(' | '[' | '{' | '<' => depth += 1,
            '>' if previous == '=' => {}
            ')' | ']' | '}' | '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(position);
                }
            }
            _ => {}
        }
        previous = ch;
    }
    None
}

#[test]
fn the_canonical_spelling_sorts_members_and_folds_booleans() {
    assert_eq!(
        canonical_text("string | null"),
        canonical_text("null | string")
    );
    assert_eq!(
        canonical_text("true | false | 1"),
        canonical_text("1 | boolean")
    );
    assert_eq!(
        canonical_text("{ a: string | number; b: [1, (2 | undefined)?]; }"),
        canonical_text("{ b: [1, 2?]; a: number | string; }")
    );
    assert_eq!(
        canonical_text("(x: string | null) => number | undefined"),
        canonical_text("(x: null | string) => undefined | number")
    );
    assert_eq!(
        canonical_text("Map<string, number | null>"),
        canonical_text("Map<string, null | number>")
    );
    assert_ne!(canonical_text("string[]"), canonical_text("number[]"));
    assert_ne!(canonical_text("(a: 1) => 2"), canonical_text("(a: 2) => 1"));
}

/// The library the harness's own library row reads: the global interfaces
/// the checker requires of a `--noLib` program, and one global value.
const HARNESS_LIB: &str = r##"interface Array<T> { length: number; [n: number]: T; }
interface Boolean {}
interface Function {}
interface IArguments {}
interface Number {}
interface Object {}
interface RegExp {}
interface CallableFunction {}
interface NewableFunction {}
interface String {}
declare const LIB_VALUE: "lib";
"##;

/// The lane reads a type probe and a function's return in a module, a
/// function's return in a global script, and a global the registered
/// library declares, each in its own project's settings.
///
/// Measured on TypeScript 7.0.2 (as the module docs describe): `f` is
/// `string | null` (`string` without `strictNullChecks`), the probe is
/// `1`, the script's `h` is `1`; `k` is `"lib"`, read off the emitted
/// `.d.ts` of `tsc --noLib` over the library, where `ReturnType` is not
/// declared.
#[test]
fn the_matrix_reads_modules_scripts_and_library_projects() {
    let mut failures =
        Matrix::new("export function f(x: string | null) { return x; }").nullness(&[
            (Read::Return("f"), "string | null", "string"),
            (Read::Type("[1] extends [number] ? 1 : 2"), "1", "1"),
        ]);
    failures.extend(
        Matrix::new("declare var g: 1;\nfunction h() { return g; }")
            .script()
            .returns(&[("h", "1")]),
    );
    failures.extend(
        Matrix::new("export function k() { return LIB_VALUE; }")
            .lib(HARNESS_LIB)
            .returns(&[("k", "\"lib\"")]),
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
