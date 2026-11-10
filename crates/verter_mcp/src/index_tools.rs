//! Index-backed bodies of the project-wide legacy tools.
//!
//! Each body finds its population with bounded workspace-index queries and
//! reads only the candidate units' owner analyses. It never walks the host's
//! file list, and nothing it builds outlives the request. Every query runs
//! under the server's configured budgets. When one cannot certify its
//! population, the answer carries an `enumeration` object that says it is
//! partial and why. It is never presented as complete.
//!
//! The judgments stay the tools' own, unchanged: prop validation, event flow,
//! provide/inject and store registrations, CSS bleed, scoring, migration, SSR
//! and route health. The index only says where to look.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use rmcp::model::{CallToolResult, ContentBlock, ErrorData};
use serde_json::Value;
use verter_diagnostics::Severity;
use verter_session::workspace_index::{
    registration_domain, registration_domain_lookup, registration_posting_key, Candidate,
    EnumerationStatus, FactCoverage, IndexDemand, IndexRow, IndexSurface, KeyFamily, Lookup,
    PartialCause, PartitionFamily, QueryBudgets, QueryWork, RegistrationKey, RegistrationRow,
    StyleRole, StyleScope, StyleSurface, StyleSymbolKind, StylesheetRow,
};
use verter_session::VerterHost;
use verter_session_query::analysis::file_analysis::FileAnalysisSnapshot;
use verter_session_query::analysis::types::{
    AnalysisFlags, AnalyzedMacroKind, VueApiClassification,
};

use verter_session::participation::{
    self, CapabilityOperation, DemandPurpose, ParticipationPermit,
};

use crate::helpers::mcp_err;
use crate::server::{
    build_script_snapshot, compute_ssr_readiness, registered_block_facts, ComponentGraphParams,
    OptionalPathParams, TraceEventFlowParams, TraceStoreFlowParams, VerterMcpServer,
};
use crate::tools::scoring;

/// Counted work of one tool answer: its index queries' work counters and the
/// owner analyses it read. Deterministic, so a test can assert it exactly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiscoveryWork {
    pub index_queries: u32,
    pub key_lookups: u32,
    pub posting_visits: u32,
    pub records_returned: u32,
    pub owner_reads: u32,
}

/// One tool answer and the work it took.
#[derive(Debug, Clone)]
pub struct ToolReport {
    pub body: Value,
    pub work: DiscoveryWork,
    /// The typed refusal answered instead of `body`, when participation
    /// refused the query or superseded it while it ran.
    pub refusal: Option<ErrorData>,
}

impl ToolReport {
    /// A refused query: no index or owner work, the typed refusal as answer.
    fn refused(error: ErrorData) -> Self {
        Self {
            body: Value::Null,
            work: DiscoveryWork::default(),
            refusal: Some(error),
        }
    }

    pub(crate) fn into_result(self) -> Result<CallToolResult, ErrorData> {
        if let Some(refusal) = self.refusal {
            return Err(refusal);
        }
        let json = serde_json::to_string_pretty(&self.body).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }
}

/// The index reads of one tool request.
pub struct Discovery<'h> {
    host: &'h VerterHost,
    budgets: QueryBudgets,
    work: DiscoveryWork,
    causes: BTreeSet<&'static str>,
    unavailable: BTreeSet<Arc<str>>,
}

impl<'h> Discovery<'h> {
    #[must_use]
    pub fn new(host: &'h VerterHost, budgets: QueryBudgets) -> Self {
        Self {
            host,
            budgets,
            work: DiscoveryWork::default(),
            causes: BTreeSet::new(),
            unavailable: BTreeSet::new(),
        }
    }

    fn absorb(&mut self, work: QueryWork, status: &EnumerationStatus) {
        self.work.index_queries += 1;
        self.work.key_lookups += work.key_lookups;
        self.work.posting_visits += work.posting_visits;
        self.work.records_returned += work.records_returned;
        match status {
            EnumerationStatus::Complete => {}
            EnumerationStatus::Partial { cause, covered, .. } => {
                self.causes.insert(cause_name(cause));
                self.unavailable
                    .extend(covered.unavailable_units.iter().cloned());
            }
            EnumerationStatus::Cancelled => {
                self.causes.insert("cancelled");
            }
        }
    }

    /// The candidate rows of one lookup over one demanded surface, in
    /// canonical order.
    pub fn candidates(&mut self, surface: IndexSurface, lookup: Lookup) -> Vec<Candidate> {
        let demand = IndexDemand::of([surface]);
        let (page, _) =
            self.host
                .workspace_index_candidates(&demand, lookup, self.budgets, None, &|| false);
        self.absorb(page.work, &page.status);
        page.candidates
    }

    /// The units one surface applies to, by locator, in path order.
    pub fn units(&mut self, surface: IndexSurface) -> Vec<Arc<str>> {
        let demand = IndexDemand::of([surface]);
        let (page, _) = self
            .host
            .workspace_index_units(&demand, surface, self.budgets, &|| false);
        self.absorb(page.work, &page.status);
        let units: BTreeSet<Arc<str>> = page
            .units
            .iter()
            .map(|slot| Arc::clone(&slot.basis.locator))
            .collect();
        units.into_iter().collect()
    }

    /// The owner's analysis of one candidate unit. A template the host's
    /// scope did not produce eagerly is produced on demand. A unit whose
    /// analysis or template cannot be read leaves the answer partial and is
    /// named.
    pub fn analysis(&mut self, canonical: &str) -> Option<FileAnalysisSnapshot> {
        self.work.owner_reads += 1;
        let Some(mut analysis) = self.host.get_analysis(canonical) else {
            self.causes.insert("unavailable_input");
            self.unavailable.insert(Arc::from(canonical));
            return None;
        };
        if analysis.template.is_none() {
            let receipt = self.host.template_facts(canonical);
            let cause = match &receipt.coverage {
                FactCoverage::Complete | FactCoverage::Absent => None,
                FactCoverage::Unavailable { .. } => Some("unavailable_input"),
                FactCoverage::Unsupported { .. } => Some("unsupported_coverage"),
            };
            if let Some(cause) = cause {
                self.causes.insert(cause);
                self.unavailable.insert(Arc::from(canonical));
            }
            analysis.template = receipt.value;
        }
        Some(analysis)
    }

    /// The answer, marked partial when any query could not certify its
    /// population.
    #[must_use]
    pub fn finish(self, mut body: Value) -> ToolReport {
        if !self.causes.is_empty() {
            let note = serde_json::json!({
                "status": "partial",
                "causes": self.causes.iter().collect::<Vec<_>>(),
                "unavailable_units": self
                    .unavailable
                    .iter()
                    .map(AsRef::as_ref)
                    .collect::<Vec<&str>>(),
            });
            if let Value::Object(map) = &mut body {
                map.insert("enumeration".to_string(), note);
            }
        }
        ToolReport {
            body,
            work: self.work,
            refusal: None,
        }
    }

    /// A unit whose participation cell is withdrawn: the answer is partial
    /// and names it.
    fn withhold(&mut self, unit: &str) {
        self.causes.insert("participation_withdrawn");
        self.unavailable.insert(Arc::from(unit));
    }
}

fn cause_name(cause: &PartialCause) -> &'static str {
    match cause {
        PartialCause::BudgetExhausted => "budget_exhausted",
        PartialCause::UnavailableInput => "unavailable_input",
        PartialCause::UnavailableProducer => "unavailable_producer",
        PartialCause::OpenDiscovery => "open_discovery",
        PartialCause::UnsupportedCoverage => "unsupported_coverage",
        PartialCause::Stale => "stale",
        PartialCause::RetentionPressure => "retention_pressure",
        PartialCause::OutsideDemand => "outside_demand",
    }
}

/// Every template component use, keyed by tag.
fn component_uses() -> Lookup {
    Lookup::Prefix {
        partition: PartitionFamily::Components,
        family: KeyFamily::Exact,
        version: 0,
        prefix: Arc::from(""),
    }
}

/// Every class token markup references, keyed by name.
fn markup_class_references() -> Lookup {
    Lookup::Prefix {
        partition: PartitionFamily::StyleSelectors,
        family: KeyFamily::Reverse,
        version: 0,
        prefix: Arc::from(""),
    }
}

/// Every stylesheet class and ID definition, keyed by name.
fn stylesheet_selectors() -> Lookup {
    Lookup::Prefix {
        partition: PartitionFamily::StyleSelectors,
        family: KeyFamily::Exact,
        version: 0,
        prefix: Arc::from(""),
    }
}

fn locator(candidate: &Candidate) -> Arc<str> {
    Arc::clone(&candidate.contribution.basis.locator)
}

/// The distinct units of `candidates`, in path order.
fn units_of(candidates: &[Candidate]) -> Vec<Arc<str>> {
    let units: BTreeSet<Arc<str>> = candidates.iter().map(locator).collect();
    units.into_iter().collect()
}

/// Candidates in path order, then in each unit's record order.
fn in_path_order(candidates: &[Candidate]) -> Vec<&Candidate> {
    let mut ordered: Vec<&Candidate> = candidates.iter().collect();
    ordered.sort_by(|a, b| {
        (a.contribution.basis.locator.as_ref(), a.record_index)
            .cmp(&(b.contribution.basis.locator.as_ref(), b.record_index))
    });
    ordered
}

fn registration(candidate: &Candidate) -> Option<&RegistrationRow> {
    match candidate.record().map(|record| &record.row) {
        Some(IndexRow::Registration(row)) => Some(row),
        _ => None,
    }
}

fn stylesheet(candidate: &Candidate) -> Option<&StylesheetRow> {
    match candidate.record().map(|record| &record.row) {
        Some(IndexRow::Stylesheet(row)) => Some(row),
        _ => None,
    }
}

fn registration_key(row: &RegistrationRow) -> &str {
    match &row.key {
        RegistrationKey::String(text) | RegistrationKey::Symbol(text) => text,
    }
}

fn file_stem(path: &str) -> &str {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("")
}

fn is_carrier(server: &VerterMcpServer, canonical: &str) -> bool {
    server
        .host
        .language_classifier()
        .classify(canonical)
        .is_framework_carrier()
}

// ── Component relations ───────────────────────────────────────────

/// The template component uses of every file that has any.
pub fn get_component_graph(server: &VerterMcpServer, params: &ComponentGraphParams) -> ToolReport {
    let mut discovery = server.discovery();
    let uses = discovery.candidates(IndexSurface::ComponentUses, component_uses());
    let root = params.root.as_ref().map(|root| server.resolve(root));
    let mut graph = serde_json::Map::new();
    for unit in units_of(&uses) {
        if root
            .as_ref()
            .is_some_and(|root| !unit.starts_with(root.as_str()))
        {
            continue;
        }
        let Some(analysis) = discovery.analysis(&unit) else {
            continue;
        };
        let Some(tpl) = &analysis.template else {
            continue;
        };
        let components: Vec<Value> = tpl
            .components
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "import_source": c.import_source,
                    "is_dynamic": c.is_dynamic,
                    "props_count": c.props.len(),
                })
            })
            .collect();
        if !components.is_empty() {
            graph.insert(unit.to_string(), Value::Array(components));
        }
    }
    discovery.finish(Value::Object(graph))
}

/// Unknown and missing required props on the components templates use.
///
/// A used tag's definition is the component whose file stem matches it, the
/// legacy join. The index keys no view by name, so that join reads the
/// component population and opens only the files whose stem a template uses.
pub fn check_component_props(server: &VerterMcpServer, params: &OptionalPathParams) -> ToolReport {
    let mut discovery = server.discovery();
    let uses = discovery.candidates(IndexSurface::ComponentUses, component_uses());
    let filter = params.path.as_ref().map(|path| server.resolve(path));

    let mut users: Vec<(Arc<str>, FileAnalysisSnapshot)> = Vec::new();
    for unit in units_of(&uses) {
        if filter
            .as_ref()
            .is_some_and(|filter| unit.as_ref() != filter.as_str())
        {
            continue;
        }
        if let Some(analysis) = discovery.analysis(&unit) {
            users.push((unit, analysis));
        }
    }
    let used: BTreeSet<&str> = users
        .iter()
        .filter_map(|(_, analysis)| analysis.template.as_ref())
        .flat_map(|tpl| tpl.components.iter().map(|c| c.name.as_str()))
        .collect();

    let mut component_props: HashMap<String, Vec<String>> = HashMap::new();
    let mut component_required: HashMap<String, Vec<String>> = HashMap::new();
    if !used.is_empty() {
        for unit in discovery.units(IndexSurface::ComponentInventory) {
            let comp_name = file_stem(&unit).to_string();
            if !used.contains(comp_name.as_str()) {
                continue;
            }
            let Some(analysis) = discovery.analysis(&unit) else {
                continue;
            };
            // Primary source: macro prop_fields
            let mut prop_names: Vec<String> = Vec::new();
            let mut required: Vec<String> = Vec::new();
            for m in analysis.macros.iter() {
                if matches!(
                    m.kind,
                    AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults
                ) {
                    for f in &m.prop_fields {
                        prop_names.push(f.name.clone());
                    }
                }
            }
            // Fallback: template prop_definitions (if macros had no prop_fields)
            if prop_names.is_empty() {
                if let Some(tpl) = &analysis.template {
                    for p in &tpl.prop_definitions {
                        prop_names.push(p.name.clone());
                        if p.is_required {
                            required.push(p.name.clone());
                        }
                    }
                }
            }
            if !prop_names.is_empty() {
                component_props.insert(comp_name.clone(), prop_names);
            }
            if !required.is_empty() {
                component_required.insert(comp_name, required);
            }
        }
    }

    let mut issues: Vec<Value> = Vec::new();
    for (id, analysis) in &users {
        let Some(tpl) = &analysis.template else {
            continue;
        };
        for comp in &tpl.components {
            let Some(known_props) = component_props.get(&comp.name) else {
                continue;
            };
            for prop in &comp.props {
                if !known_props.contains(&prop.name) && !prop.from_spread {
                    issues.push(serde_json::json!({
                        "kind": "unknown_prop",
                        "file": id.as_ref(),
                        "component": comp.name,
                        "prop": prop.name,
                    }));
                }
            }
            if comp.has_spread {
                continue;
            }
            if let Some(required) = component_required.get(&comp.name) {
                let passed: HashSet<_> = comp.props.iter().map(|p| &p.name).collect();
                for req in required {
                    if !passed.contains(req) {
                        issues.push(serde_json::json!({
                            "kind": "missing_required_prop",
                            "file": id.as_ref(),
                            "component": comp.name,
                            "prop": req,
                        }));
                    }
                }
            }
        }
    }

    let total = issues.len();
    discovery.finish(serde_json::json!({
        "issues": issues,
        "total": total,
    }))
}

/// Props a component receives and passes straight on to a child.
pub fn detect_prop_drilling(server: &VerterMcpServer, params: &OptionalPathParams) -> ToolReport {
    let mut discovery = server.discovery();
    let uses = discovery.candidates(IndexSurface::ComponentUses, component_uses());
    let mut drilled: Vec<Value> = Vec::new();
    for unit in units_of(&uses) {
        let Some(analysis) = discovery.analysis(&unit) else {
            continue;
        };
        let Some(tpl) = &analysis.template else {
            continue;
        };
        let comp_name = file_stem(&unit);
        let received: HashSet<&str> = tpl
            .prop_definitions
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        for child in &tpl.components {
            let passed: BTreeSet<&str> = child.props.iter().map(|p| p.name.as_str()).collect();
            let forwarded: Vec<&str> = passed
                .into_iter()
                .filter(|name| received.contains(name))
                .collect();
            if !forwarded.is_empty() {
                drilled.push(serde_json::json!({
                    "parent": comp_name,
                    "child": child.name,
                    "drilled_props": forwarded,
                    "suggestion": "Consider using provide/inject instead of passing through intermediate components",
                }));
            }
        }
    }

    if let Some(path) = &params.path {
        let comp_name = file_stem(path);
        drilled.retain(|d| {
            d["parent"].as_str() == Some(comp_name) || d["child"].as_str() == Some(comp_name)
        });
    }

    let total = drilled.len();
    discovery.finish(serde_json::json!({
        "drilled_props": drilled,
        "total": total,
    }))
}

/// The components that declare one event and the templates that listen to it.
pub fn trace_event_flow(server: &VerterMcpServer, params: &TraceEventFlowParams) -> ToolReport {
    let mut discovery = server.discovery();
    let rows = discovery.candidates(
        IndexSurface::Registrations,
        Lookup::Keys(vec![registration_posting_key(
            registration_domain::EVENT,
            &params.event_name,
        )]),
    );
    let mut emitters: BTreeSet<Arc<str>> = BTreeSet::new();
    let mut listeners: BTreeSet<Arc<str>> = BTreeSet::new();
    for candidate in &rows {
        let Some(row) = registration(candidate) else {
            continue;
        };
        if row.domain.as_ref() != registration_domain::EVENT
            || registration_key(row) != params.event_name
        {
            continue;
        }
        if row.is_usage {
            listeners.insert(locator(candidate));
        } else {
            emitters.insert(locator(candidate));
        }
    }
    let names = |set: BTreeSet<Arc<str>>| set.iter().map(|p| p.to_string()).collect::<Vec<_>>();
    discovery.finish(serde_json::json!({
        "event_name": params.event_name,
        "emitters": names(emitters),
        "listeners": names(listeners),
    }))
}

/// One query over classified component-API facts (lifecycle hooks,
/// `provide`/`inject`, `defineComponent`, stores), admitted in this
/// invocation's own participation scope (COX0; UAK0 `D11`) before any index
/// or owner read, never through a framework identity.
struct ComponentApiQuery {
    operation: CapabilityOperation,
    purpose: DemandPurpose,
    permit: ParticipationPermit,
}

impl ComponentApiQuery {
    /// A bounded workspace query over classified registrations.
    fn registrations(server: &VerterMcpServer) -> Result<Self, ToolReport> {
        Self::admit(
            server,
            CapabilityOperation::WorkspaceContribution,
            DemandPurpose::IndexContribution,
        )
    }

    /// An explained per-component analysis report.
    fn analysis_report(server: &VerterMcpServer) -> Result<Self, ToolReport> {
        Self::admit(
            server,
            CapabilityOperation::AnalysisInspection,
            DemandPurpose::Explain,
        )
    }

    /// Admit the workspace-wide demand; a refusal is the typed answer.
    fn admit(
        server: &VerterMcpServer,
        operation: CapabilityOperation,
        purpose: DemandPurpose,
    ) -> Result<Self, ToolReport> {
        participation::admit_component_api_query(
            &server.participation,
            &server.host,
            operation,
            purpose,
        )
        .map(|permit| Self {
            operation,
            purpose,
            permit,
        })
        .map_err(|refused| {
            ToolReport::refused(participation_error(
                format!(
                    "verter does not serve this query: {}",
                    refused.primary.key()
                ),
                serde_json::json!({
                    "primary": refused.primary.key(),
                    "restrictions": refused
                        .restrictions
                        .iter()
                        .map(|reason| reason.key())
                        .collect::<Vec<_>>(),
                }),
            ))
        })
    }

    /// Whether `unit` belongs to this query's population. A unit whose
    /// adapter does not classify these facts is outside it. A unit whose
    /// own cell is withdrawn is left out and named, so the answer is never
    /// presented as complete.
    fn unit(&self, server: &VerterMcpServer, discovery: &mut Discovery<'_>, unit: &str) -> bool {
        let catalog = participation::component_api_support(&server.host, unit);
        match participation::admit_file(
            &server.participation,
            &server.host,
            unit,
            self.operation,
            self.purpose,
            catalog,
        ) {
            Ok(_) => true,
            Err(_) if catalog == participation::CatalogSupport::UnsupportedCapability => false,
            Err(_) => {
                discovery.withhold(unit);
                false
            }
        }
    }

    /// The answer, served only while the demand epoch it was admitted under
    /// is current.
    fn finish(self, server: &VerterMcpServer, discovery: Discovery<'_>, body: Value) -> ToolReport {
        if server.participation.is_current(&self.permit) {
            discovery.finish(body)
        } else {
            ToolReport::refused(participation_error(
                "participation changed while the query ran; retry it".to_string(),
                serde_json::json!({ "primary": "superseded" }),
            ))
        }
    }
}

/// The typed refusal of a participation decision: never an empty answer.
fn participation_error(message: String, detail: Value) -> ErrorData {
    ErrorData::invalid_request(
        message,
        Some(serde_json::json!({ "verterParticipation": detail })),
    )
}

/// Injected keys nobody provides and provided keys nobody injects.
pub fn validate_provide_inject(server: &VerterMcpServer) -> ToolReport {
    let query = match ComponentApiQuery::registrations(server) {
        Ok(query) => query,
        Err(refused) => return refused,
    };
    let mut discovery = server.discovery();
    let rows = discovery.candidates(
        IndexSurface::Registrations,
        registration_domain_lookup(registration_domain::PROVIDE_INJECT),
    );
    let mut provides: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut injects: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for candidate in in_path_order(&rows) {
        let Some(row) = registration(candidate) else {
            continue;
        };
        if row.domain.as_ref() != registration_domain::PROVIDE_INJECT {
            continue;
        }
        let unit = locator(candidate);
        if !query.unit(server, &mut discovery, &unit) {
            continue;
        }
        let side = if row.is_usage {
            &mut injects
        } else {
            &mut provides
        };
        side.entry(registration_key(row).to_string())
            .or_default()
            .push(unit.to_string());
    }

    let missing_providers: Vec<Value> = injects
        .iter()
        .filter(|(key, _)| !provides.contains_key(*key))
        .map(|(key, inject_files)| {
            serde_json::json!({
                "key": key,
                "inject_files": inject_files,
            })
        })
        .collect();
    let unused_provides: Vec<Value> = provides
        .iter()
        .filter(|(key, _)| !injects.contains_key(*key))
        .map(|(key, provide_files)| {
            serde_json::json!({
                "key": key,
                "provide_files": provide_files,
            })
        })
        .collect();

    let body = serde_json::json!({
        "total_provide_keys": provides.len(),
        "total_inject_keys": injects.len(),
        "missing_providers": missing_providers,
        "unused_provides": unused_provides,
    });
    query.finish(server, discovery, body)
}

// ── Stores ────────────────────────────────────────────────────────

/// The units holding any store definition or use, in path order.
fn store_units(discovery: &mut Discovery<'_>) -> Vec<Arc<str>> {
    let rows = discovery.candidates(
        IndexSurface::Registrations,
        registration_domain_lookup(registration_domain::STORE),
    );
    let stores: Vec<Candidate> = rows
        .into_iter()
        .filter(|candidate| {
            registration(candidate)
                .is_some_and(|row| row.domain.as_ref() == registration_domain::STORE)
        })
        .collect();
    units_of(&stores)
}

/// Per-file store uses and definitions.
pub fn get_store_usage(server: &VerterMcpServer, params: &OptionalPathParams) -> ToolReport {
    let query = match ComponentApiQuery::registrations(server) {
        Ok(query) => query,
        Err(refused) => return refused,
    };
    let mut discovery = server.discovery();
    let filter_id = params.path.as_ref().map(|p| server.resolve(p));
    let mut results: Vec<Value> = Vec::new();
    for id in store_units(&mut discovery) {
        if filter_id
            .as_ref()
            .is_some_and(|filter| id.as_ref() != filter.as_str())
        {
            continue;
        }
        if !query.unit(server, &mut discovery, &id) {
            continue;
        }
        let Some(analysis) = discovery.analysis(&id) else {
            continue;
        };
        if analysis.store_usages.is_empty() && analysis.store_definitions.is_empty() {
            continue;
        }
        let usages: Vec<Value> = analysis
            .store_usages
            .iter()
            .map(|u| {
                serde_json::json!({
                    "binding_name": u.binding_name,
                    "callee": u.callee,
                    "import_source": u.import_source,
                    "store_api": format!("{:?}", u.store_api),
                    "has_store_to_refs": u.has_store_to_refs,
                    "destructured_props": u.destructured_props,
                    "destructured_without_store_to_refs": u.destructured_without_store_to_refs,
                })
            })
            .collect();
        let definitions: Vec<Value> = analysis
            .store_definitions
            .iter()
            .map(|d| {
                serde_json::json!({
                    "store_id": d.store_id,
                    "export_name": d.export_name,
                    "store_api": format!("{:?}", d.store_api),
                    "state_properties": d.state_properties,
                    "getters": d.getters,
                    "actions": d.actions,
                })
            })
            .collect();
        results.push(serde_json::json!({
            "file": id.as_ref(),
            "store_usages": usages,
            "store_definitions": definitions,
        }));
    }
    let files_with_stores = results.len();
    let body = serde_json::json!({
        "files_with_stores": files_with_stores,
        "results": results,
    });
    query.finish(server, discovery, body)
}

/// Components to stores, store to store dependencies, and unused stores.
pub fn get_store_graph(server: &VerterMcpServer) -> ToolReport {
    let query = match ComponentApiQuery::registrations(server) {
        Ok(query) => query,
        Err(refused) => return refused,
    };
    let mut discovery = server.discovery();
    let mut store_definitions: Vec<Value> = Vec::new();
    let mut store_usages_by_file: Vec<Value> = Vec::new();
    let mut all_store_ids: Vec<String> = Vec::new();
    let mut used_callees: HashSet<String> = HashSet::new();
    for id in store_units(&mut discovery) {
        if !query.unit(server, &mut discovery, &id) {
            continue;
        }
        let Some(analysis) = discovery.analysis(&id) else {
            continue;
        };
        for def in analysis.store_definitions.iter() {
            if let Some(store_id) = &def.store_id {
                all_store_ids.push(store_id.clone());
            }
            store_definitions.push(serde_json::json!({
                "file": id.as_ref(),
                "store_id": def.store_id,
                "export_name": def.export_name,
                "state_properties": def.state_properties,
                "getters": def.getters,
                "actions": def.actions,
                "store_dependencies": def.store_dependencies,
            }));
        }
        if !analysis.store_usages.is_empty() {
            let callees: Vec<&str> = analysis
                .store_usages
                .iter()
                .map(|u| u.callee.as_str())
                .collect();
            for c in &callees {
                used_callees.insert((*c).to_string());
            }
            store_usages_by_file.push(serde_json::json!({
                "file": id.as_ref(),
                "stores_used": callees,
            }));
        }
    }

    let unused_stores: Vec<&String> = all_store_ids
        .iter()
        .filter(|id| !used_callees.iter().any(|c| c.contains(id.as_str())))
        .collect();

    let body = serde_json::json!({
        "total_store_definitions": store_definitions.len(),
        "total_files_using_stores": store_usages_by_file.len(),
        "store_definitions": store_definitions,
        "store_usages_by_file": store_usages_by_file,
        "all_store_ids": all_store_ids,
        "unused_stores": unused_stores,
    });
    query.finish(server, discovery, body)
}

/// One store's definition, consumers and dependencies.
pub fn trace_store_flow(server: &VerterMcpServer, params: &TraceStoreFlowParams) -> ToolReport {
    let query = match ComponentApiQuery::registrations(server) {
        Ok(query) => query,
        Err(refused) => return refused,
    };
    let mut discovery = server.discovery();
    let mut definition_file: Option<String> = None;
    let mut definition_info: Option<Value> = None;
    let mut consumer_files: Vec<Value> = Vec::new();
    for id in store_units(&mut discovery) {
        if !query.unit(server, &mut discovery, &id) {
            continue;
        }
        let Some(analysis) = discovery.analysis(&id) else {
            continue;
        };
        for def in analysis.store_definitions.iter() {
            if def.store_id.as_deref() == Some(params.store_id.as_str())
                || def.export_name == params.store_id
            {
                definition_file = Some(id.to_string());
                definition_info = Some(serde_json::json!({
                    "store_id": def.store_id,
                    "export_name": def.export_name,
                    "state_properties": def.state_properties,
                    "getters": def.getters,
                    "actions": def.actions,
                    "store_dependencies": def.store_dependencies,
                }));
            }
        }
        let matching_usages: Vec<Value> = analysis
            .store_usages
            .iter()
            .filter(|u| {
                u.callee.contains(&params.store_id) || u.import_source.contains(&params.store_id)
            })
            .map(|u| {
                serde_json::json!({
                    "binding_name": u.binding_name,
                    "callee": u.callee,
                    "has_store_to_refs": u.has_store_to_refs,
                    "destructured_props": u.destructured_props,
                    "destructured_without_store_to_refs": u.destructured_without_store_to_refs,
                })
            })
            .collect();
        if !matching_usages.is_empty() {
            consumer_files.push(serde_json::json!({
                "file": id.as_ref(),
                "usages": matching_usages,
            }));
        }
    }

    let consumer_count = consumer_files.len();
    let body = serde_json::json!({
        "store_id": params.store_id,
        "definition_file": definition_file,
        "definition": definition_info,
        "consumer_count": consumer_count,
        "consumers": consumer_files,
    });
    query.finish(server, discovery, body)
}

// ── CSS ───────────────────────────────────────────────────────────

/// Global classes one component defines and another's template uses, and
/// global classes several components define.
pub fn detect_css_bleed(server: &VerterMcpServer, params: &OptionalPathParams) -> ToolReport {
    let mut discovery = server.discovery();
    let definitions = discovery.candidates(IndexSurface::StyleDefinitions, stylesheet_selectors());
    let references = discovery.candidates(
        IndexSurface::MarkupClassReferences,
        markup_class_references(),
    );

    // Class name -> files that define it outside a scoped block, one entry
    // per definition.
    let mut global_class_files: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for candidate in in_path_order(&definitions) {
        let Some(row) = stylesheet(candidate) else {
            continue;
        };
        let defines_global_class = row.kind == StyleSymbolKind::Class
            && row.role == StyleRole::Definition
            && matches!(row.surface, StyleSurface::Style { .. })
            && row.scope != StyleScope::Scoped;
        let unit = locator(candidate);
        if !defines_global_class || !is_carrier(server, &unit) {
            continue;
        }
        global_class_files
            .entry(row.name.to_string())
            .or_default()
            .push(unit.to_string());
    }
    // File -> classes its template uses.
    let mut file_template_classes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for candidate in &references {
        let Some(row) = stylesheet(candidate) else {
            continue;
        };
        let uses_class = row.kind == StyleSymbolKind::Class
            && row.role == StyleRole::Reference
            && row.surface == StyleSurface::Template;
        let unit = locator(candidate);
        if !uses_class || !is_carrier(server, &unit) {
            continue;
        }
        file_template_classes
            .entry(unit.to_string())
            .or_default()
            .insert(row.name.to_string());
    }

    let mut bleeds: Vec<Value> = Vec::new();
    for (class_name, defining_files) in &global_class_files {
        for (file_id, template_classes) in &file_template_classes {
            if !template_classes.contains(class_name) {
                continue;
            }
            for def_file in defining_files {
                if def_file != file_id {
                    bleeds.push(serde_json::json!({
                        "kind": "global_class_bleed",
                        "class": class_name,
                        "source_file": def_file,
                        "affected_file": file_id,
                        "severity": "warning",
                    }));
                }
            }
        }
        if defining_files.len() > 1 {
            bleeds.push(serde_json::json!({
                "kind": "shared_class_collision",
                "class": class_name,
                "files": defining_files,
                "severity": "info",
            }));
        }
    }

    if let Some(path) = &params.path {
        let resolved = server.resolve(path);
        bleeds.retain(|b| {
            b["source_file"].as_str() == Some(resolved.as_str())
                || b["affected_file"].as_str() == Some(resolved.as_str())
                || b["files"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|f| f.as_str() == Some(resolved.as_str())))
        });
    }

    let total_bleed_count = bleeds.len();
    discovery.finish(serde_json::json!({
        "bleeds": bleeds,
        "total_bleed_count": total_bleed_count,
    }))
}

// ── Populations ───────────────────────────────────────────────────

/// Aggregate scores, Vue API use, style and diagnostics health over every
/// component.
pub fn get_project_stats(server: &VerterMcpServer) -> ToolReport {
    let mut discovery = server.discovery();
    let files = discovery.units(IndexSurface::Imports);
    let component_files = discovery.units(IndexSurface::ComponentInventory);
    let components: BTreeSet<&str> = component_files.iter().map(AsRef::as_ref).collect();

    let mut quality_scores: Vec<(String, u32)> = Vec::new();
    let mut total_errors = 0usize;
    let mut total_warnings = 0usize;
    let mut total_info = 0usize;
    let mut vue_api_usage: HashMap<String, usize> = HashMap::new();
    let mut macro_usage: HashMap<String, usize> = HashMap::new();
    let mut total_selectors = 0usize;
    let mut scoped_blocks = 0usize;
    let mut global_blocks = 0usize;
    let mut by_category: HashMap<String, usize> = HashMap::new();
    let mut total_elements = 0usize;
    let mut total_bindings = 0usize;

    for canonical in &component_files {
        let Some(analysis) = discovery.analysis(canonical) else {
            continue;
        };
        let source = server.host.get_source(canonical);
        let script_snapshot = build_script_snapshot(&analysis);

        let block_facts = registered_block_facts(&server.host, canonical);
        let quality = scoring::compute_quality_score(
            Some(&script_snapshot),
            analysis.template.as_deref(),
            &analysis.styles,
            source.as_deref(),
            &block_facts,
        );
        quality_scores.push((canonical.to_string(), quality.score));

        let diags = server.linter.lint_with_source(
            Some(&script_snapshot),
            analysis.template.as_deref(),
            &analysis.styles,
            source.as_deref(),
            &block_facts,
        );
        for d in diags.into_diagnostics() {
            match d.severity {
                Severity::Error => total_errors += 1,
                Severity::Warning => total_warnings += 1,
                _ => total_info += 1,
            }
            *by_category.entry(d.category.clone()).or_default() += 1;
        }

        for call in analysis.vue_api_calls.iter() {
            *vue_api_usage
                .entry(call.api.display_name().to_string())
                .or_default() += 1;
        }

        for m in analysis.macros.iter() {
            *macro_usage.entry(format!("{:?}", m.kind)).or_default() += 1;
        }

        for style in analysis.styles.iter() {
            if style.scoped {
                scoped_blocks += 1;
            } else {
                global_blocks += 1;
            }
            if let Some(css) = &style.css {
                total_selectors += css.selectors.len();
            }
        }

        if let Some(tpl) = &analysis.template {
            total_elements += tpl.elements.len();
        }
        total_bindings += analysis.bindings.len();
    }

    quality_scores.sort_by_key(|score| score.1);
    let avg_score = if quality_scores.is_empty() {
        0
    } else {
        quality_scores.iter().map(|s| s.1 as usize).sum::<usize>() / quality_scores.len()
    };

    let worst: Vec<_> = quality_scores.iter().take(5).collect();
    let best: Vec<_> = quality_scores.iter().rev().take(5).collect();
    let script_deps = files
        .iter()
        .filter(|file| !components.contains(file.as_ref()))
        .count();

    discovery.finish(serde_json::json!({
        "overview": {
            "total_files": files.len(),
            "componentFiles": component_files.len(),
            // deprecated alias: kept for back-compat
            "vue_files": component_files.len(),
            "script_deps": script_deps,
        },
        "component_stats": {
            "avg_quality_score": avg_score,
            "worst_components": worst.iter().map(|(p, s)| serde_json::json!({"path": p, "score": s})).collect::<Vec<_>>(),
            "best_components": best.iter().map(|(p, s)| serde_json::json!({"path": p, "score": s})).collect::<Vec<_>>(),
            "total_elements": total_elements,
            "total_bindings": total_bindings,
        },
        "vue_api_usage": vue_api_usage,
        "macro_usage": macro_usage,
        "style_stats": {
            "scoped_blocks": scoped_blocks,
            "global_blocks": global_blocks,
            "total_selectors": total_selectors,
        },
        "diagnostics_health": {
            "total_errors": total_errors,
            "total_warnings": total_warnings,
            "total_info": total_info,
            "by_category": by_category,
        },
    }))
}

/// Options API components a Composition API rewrite could replace.
pub fn detect_migration_targets(
    server: &VerterMcpServer,
    params: &OptionalPathParams,
) -> ToolReport {
    let query = match ComponentApiQuery::analysis_report(server) {
        Ok(query) => query,
        Err(refused) => return refused,
    };
    let mut discovery = server.discovery();
    let filter = params.path.as_ref().map(|path| server.resolve(path));
    let mut targets: Vec<Value> = Vec::new();
    for id in discovery.units(IndexSurface::ComponentInventory) {
        if filter
            .as_ref()
            .is_some_and(|filter| id.as_ref() != filter.as_str())
        {
            continue;
        }
        if !query.unit(server, &mut discovery, &id) {
            continue;
        }
        let Some(analysis) = discovery.analysis(&id) else {
            continue;
        };
        let flags = AnalysisFlags::from_bits_truncate(analysis.script_flags);
        let has_define_component = analysis
            .vue_api_calls
            .iter()
            .any(|c| c.api == VueApiClassification::DefineComponent);
        let has_setup_macros = analysis.macros.iter().any(|m| {
            matches!(
                m.kind,
                AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::DefineEmits
            )
        });
        if !has_define_component || has_setup_macros {
            continue;
        }
        let complexity = if analysis.bindings.len() > 20 {
            "hard"
        } else if analysis.bindings.len() > 10 {
            "medium"
        } else {
            "easy"
        };
        targets.push(serde_json::json!({
            "path": id.as_ref(),
            "reason": "Uses defineComponent without script setup macros",
            "difficulty": complexity,
            "bindings": analysis.bindings.len(),
            "has_lifecycle": flags.contains(AnalysisFlags::HAS_LIFECYCLE_HOOKS),
            "has_watchers": flags.contains(AnalysisFlags::HAS_WATCHERS),
        }));
    }
    let total = targets.len();
    let body = serde_json::json!({
        "targets": targets,
        "total": total,
    });
    query.finish(server, discovery, body)
}

/// Per-component SSR readiness over every Vue component, with its summary.
pub fn ssr_project_report(server: &VerterMcpServer) -> ToolReport {
    let query = match ComponentApiQuery::analysis_report(server) {
        Ok(query) => query,
        Err(refused) => return refused,
    };
    let mut discovery = server.discovery();
    let mut components = Vec::new();
    let mut total_score: f64 = 0.0;
    let mut blocking = Vec::new();
    for canonical in discovery.units(IndexSurface::ComponentInventory) {
        if !query.unit(server, &mut discovery, &canonical) {
            continue;
        }
        let Some(analysis) = discovery.analysis(&canonical) else {
            continue;
        };
        let result = compute_ssr_readiness(&analysis);
        let score = result["score"].as_u64().unwrap_or(0);
        total_score += score as f64;
        if score < 50 {
            blocking.push(serde_json::json!({
                "file": canonical.as_ref(),
                "score": score,
                "issues": result["issues"],
            }));
        }
        components.push(serde_json::json!({
            "file": canonical.as_ref(),
            "score": score,
        }));
    }

    let count = components.len().max(1);
    let avg_score = (total_score / count as f64).round() as u64;

    // Score distribution buckets
    let mut dist = [0u32; 5]; // 0-19, 20-39, 40-59, 60-79, 80-100
    for c in &components {
        let s = c["score"].as_u64().unwrap_or(0);
        let bucket = (s / 20).min(4) as usize;
        dist[bucket] += 1;
    }

    blocking.sort_by(|a, b| {
        a["score"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&b["score"].as_u64().unwrap_or(0))
    });

    let body = serde_json::json!({
        "total_components": components.len(),
        "average_score": avg_score,
        "score_distribution": {
            "0-19": dist[0],
            "20-39": dist[1],
            "40-59": dist[2],
            "60-79": dist[3],
            "80-100": dist[4],
        },
        "ssr_ready": components.iter().filter(|c| c["score"].as_u64().unwrap_or(0) >= 80).count(),
        "needs_work": components.iter().filter(|c| {
            let s = c["score"].as_u64().unwrap_or(0);
            (50..80).contains(&s)
        }).count(),
        "blocking": blocking.len(),
        "critical_path_blockers": blocking.iter().take(10).collect::<Vec<_>>(),
        "components": components,
    });
    query.finish(server, discovery, body)
}

/// Routes cross-referenced with the tracked files and the templates'
/// navigation.
pub fn analyze_route_health(
    server: &VerterMcpServer,
    project_root: &std::path::Path,
) -> Result<ToolReport, ErrorData> {
    let mut discovery = server.discovery();
    let snapshot = server.build_route_snapshot(project_root, &mut discovery);
    let existing_files: HashSet<String> = discovery
        .units(IndexSurface::Imports)
        .iter()
        .map(|unit| unit.to_string())
        .collect();
    let report = verter_semantic::analysis::analyze_route_health(&snapshot, &existing_files);
    let body = serde_json::to_value(&report).map_err(|e| mcp_err(e.to_string()))?;
    Ok(discovery.finish(body))
}

/// The template component uses of every file that has any, for route
/// analysis.
pub(crate) fn route_template_components(
    discovery: &mut Discovery<'_>,
) -> Vec<(
    String,
    Vec<verter_session_query::analysis::template::TemplateComponentUsage>,
)> {
    let uses = discovery.candidates(IndexSurface::ComponentUses, component_uses());
    let mut template_components = Vec::new();
    for unit in units_of(&uses) {
        let Some(analysis) = discovery.analysis(&unit) else {
            continue;
        };
        if let Some(tpl) = &analysis.template {
            template_components.push((unit.to_string(), tpl.components.clone()));
        }
    }
    template_components
}
