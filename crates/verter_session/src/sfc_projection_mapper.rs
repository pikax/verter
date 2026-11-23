//! Lowers a checking module onto the content-mapper plane and its wire shape.
//!
//! The adapter composes one carrier at a time from the full content it is
//! given, never from a file it reads, and keeps every setting per project
//! handle. Composition runs behind an unwind boundary: a refusal or a defect
//! in the projection is a diagnostic on that carrier, never a failure of the
//! project or of the next request.
//!
//! The wire half of this module is the message vocabulary of the TypeScript
//! content-mapper protocol (`initialize`, `openProject`, `transform`,
//! `closeProject`), so a transport that serves the adapter only frames JSON.

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};
#[cfg(any(test, feature = "test-util"))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use verter_compiler::assembly::source_unit::source_unit_id;
use verter_compiler::code_transform::{MappingSpan, ProjectedClass};
use verter_compiler::diagnostics::CompilerErrorCode;
use verter_compiler::framework_common::projection_plan::origin::ObservationRole;
use verter_compiler::framework_common::registered_carrier_projection::project_registered_accepted;
use verter_compiler::framework_common::vue_projection_backend::{
    compose_checking_module, CheckingDialect, CheckingModule, CheckingModuleInput,
    CheckingModuleRefusal, ExternalLegality, SetupProjectionRefusal,
};
use verter_compiler::framework_common::FrameworkParseArtifact;
use verter_identity::encoding::CanonicalEncoder;
use verter_identity::identity::ContentId;
use verter_language::carrier_grammar::CarrierGrammarAuthority;
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::{
    ExternalLinkKind, FileLanguage, LanguageDiagnostic, LanguageDiagnosticSeverity, SyntaxReject,
};
use verter_span::path::canonicalize_path;

use crate::content_mapper::ContentMapper;
use crate::framework::descriptor::{vue_descriptor, FrameworkAdapterDescriptor};
use crate::framework::registry::FrameworkCapabilityCatalog;
use crate::sfc_projection_cache::{
    authored_revision, revisions_of, ProjectionFragmentCache, ProjectionReadSet,
};

/// Text of a module that could not be composed: an empty module, so the
/// engine still sees a well-formed file at the carrier's virtual path.
const REFUSED_MODULE_TEXT: &str = "export {};";

/// The logical role the checking module's lineage is minted under.
const CHECKING_UNIT_ROLE: &str = "checking";

/// Opaque handle the engine gives a project. All mapper state is keyed by it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MapperProjectHandle(pub Arc<str>);

impl MapperProjectHandle {
    /// The handle of the project whose tsconfig is `config_file`: its
    /// canonical path, so two spellings of one config name one project.
    #[must_use]
    pub fn of_config(config_file: &str) -> Self {
        Self(Arc::from(canonicalize_path(config_file)))
    }
}

/// Settings of one opened project. Two projects never share settings or results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapperProjectSettings {
    /// The project's tsconfig path.
    pub config_file: Arc<str>,
    /// Custom-element prefixes for this project.
    pub custom_elements: Vec<String>,
    /// Compiler options the transform depends on, as `name=value`, sorted.
    pub declared_option_dependencies: Vec<String>,
    /// Files whose change invalidates every transform of the project.
    pub watched_configuration: Vec<Arc<str>>,
    /// Whether each transform also builds the carrier's declaration product
    /// (`CheckingModule::declarations`). Off unless the project publishes
    /// declarations.
    pub declarations: bool,
    /// Digest over all fields above.
    pub identity: [u8; 32],
}

impl MapperProjectSettings {
    /// Settings over these fields, with the option dependencies sorted and
    /// de-duplicated and `identity` computed over all of them, so a change to
    /// any field is a different identity. No declaration product is built.
    #[must_use]
    pub fn new(
        config_file: Arc<str>,
        custom_elements: Vec<String>,
        mut declared_option_dependencies: Vec<String>,
        watched_configuration: Vec<Arc<str>>,
    ) -> Self {
        declared_option_dependencies.sort_unstable();
        declared_option_dependencies.dedup();
        let mut settings = Self {
            config_file,
            custom_elements,
            declared_option_dependencies,
            watched_configuration,
            declarations: false,
            identity: [0; 32],
        };
        settings.identity = settings.digest();
        settings
    }

    /// The same settings with the declaration product built or not; the
    /// identity follows, so declaring and checking-only results never share
    /// a cache entry.
    #[must_use]
    pub fn with_declarations(mut self, declarations: bool) -> Self {
        self.declarations = declarations;
        self.identity = self.digest();
        self
    }

    fn digest(&self) -> [u8; 32] {
        let mut encoder = CanonicalEncoder::new("verter.session.sfc_projection_mapper.settings.v1");
        encoder.field_str(1, &self.config_file);
        encoder.field_u64(2, self.custom_elements.len() as u64);
        for prefix in &self.custom_elements {
            encoder.field_str(3, prefix);
        }
        encoder.field_u64(4, self.declared_option_dependencies.len() as u64);
        for option in &self.declared_option_dependencies {
            encoder.field_str(5, option);
        }
        encoder.field_u64(6, self.watched_configuration.len() as u64);
        for file in &self.watched_configuration {
            encoder.field_str(7, file);
        }
        // Absent when off, so checking-only identities are unchanged.
        if self.declarations {
            encoder.field_u64(8, 1);
        }
        *encoder.digest().as_bytes()
    }
}

/// Span kinds of the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WireSpanKind {
    Verbatim = 0,
    Atom = 1,
    Alias = 2,
}

/// One wire mapping. Offsets are UTF-8 bytes. Synthesized text has no mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireSpanMapping {
    pub virtual_start: u32,
    pub virtual_length: u32,
    pub original_start: u32,
    pub original_length: u32,
    pub kind: WireSpanKind,
    /// Feature bitmask, lowered from observation roles.
    pub features: u32,
}

impl WireSpanMapping {
    /// The protocol's tuple:
    /// `[virtualStart, virtualLength, originalStart, originalLength, kind, features]`.
    #[must_use]
    pub fn to_wire(&self) -> [u32; 6] {
        [
            self.virtual_start,
            self.virtual_length,
            self.original_start,
            self.original_length,
            self.kind as u32,
            self.features,
        ]
    }
}

/// The protocol's feature bits a mapping can carry.
pub mod wire_features {
    pub const HOVER: u32 = 1 << 0;
    pub const SIGNATURE_HELP: u32 = 1 << 1;
    pub const COMPLETION: u32 = 1 << 2;
    pub const DEFINITION: u32 = 1 << 3;
    pub const TYPE_DEFINITION: u32 = 1 << 4;
    pub const REFERENCES: u32 = 1 << 6;
    pub const DOCUMENT_HIGHLIGHTS: u32 = 1 << 7;
    pub const RENAME: u32 = 1 << 8;
}

/// A diagnostic the mapper itself reports for a carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapperDiagnostic {
    pub start: u32,
    pub end: u32,
    pub code: MapperDiagnosticCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapperDiagnosticCode {
    /// Syntax error from recovery.
    Syntax,
    /// The carrier parser's own error that the `<script>` and `<script setup>`
    /// blocks declare different `lang`s. The wire carries it as a syntax error.
    ScriptLangConflict,
    /// The carrier could not be projected. The message says why.
    ProjectionRefused,
    /// A defect in the projection itself. Never filtered.
    ProjectionInternal,
    /// Two branches of one `v-if` chain carry the same `key`. A warning:
    /// the chain is well formed, so the carrier is still fully checked.
    RepeatedBranchKey,
}

impl MapperDiagnosticCode {
    /// The numeric code the wire carries; the engine prints it after the
    /// mapper's diagnostic source.
    #[must_use]
    pub fn wire_code(self) -> u32 {
        match self {
            Self::Syntax | Self::ScriptLangConflict => 1001,
            Self::ProjectionRefused => 1002,
            Self::ProjectionInternal => 1003,
            Self::RepeatedBranchKey => 1004,
        }
    }

    /// True for a diagnostic that reports advice, not a failure: it neither
    /// makes the result incomplete nor fails a run.
    #[must_use]
    pub fn is_warning(self) -> bool {
        match self {
            Self::RepeatedBranchKey => true,
            Self::Syntax
            | Self::ScriptLangConflict
            | Self::ProjectionRefused
            | Self::ProjectionInternal => false,
        }
    }
}

/// Result of transforming one carrier, always from the full content supplied.
#[derive(Debug, Clone)]
pub struct MapperTransformResult {
    /// Virtual file name: the carrier path, a dot, the dialect extension.
    pub virtual_file: Arc<str>,
    pub dialect: CheckingDialect,
    pub text: Arc<str>,
    pub mappings: Vec<WireSpanMapping>,
    pub diagnostics: Vec<MapperDiagnostic>,
    /// The composed module. `None` when composition was refused or panicked;
    /// then `text` is `export {};` and `diagnostics` holds the reason.
    pub module: Option<Arc<CheckingModule>>,
    /// The mapper plane for position queries. `None` exactly when `module` is.
    pub plane: Option<Arc<ContentMapper>>,
    /// False when the result must not warm a complete cache.
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapperError {
    UnknownProject(MapperProjectHandle),
    ProjectAlreadyOpen(MapperProjectHandle),
}

/// Why one carrier has no composed module.
#[derive(Debug)]
pub(crate) enum CompositionFailure {
    /// The carrier grammar rejected the source.
    Rejected(SyntaxReject),
    /// The source could not be registered or accepted at all.
    Unregistered(String),
    /// The composer refused the parsed carrier.
    Refused(CheckingModuleRefusal),
    /// The carrier names external `src` blocks, which only a host can bind:
    /// the transform reads nothing but the content it is given. Each entry
    /// is the specifier and its range.
    UnboundExternal(Vec<(String, u32, u32)>),
}

/// One composition: the module, or why there is none, and the carrier's own
/// parse errors either way.
pub(crate) struct Composition {
    pub(crate) module: Result<CheckingModule, CompositionFailure>,
    pub(crate) parse_diagnostics: Vec<MapperDiagnostic>,
}

impl From<CompositionFailure> for Composition {
    fn from(failure: CompositionFailure) -> Self {
        Self {
            module: Err(failure),
            parse_diagnostics: Vec::new(),
        }
    }
}

/// Composes one carrier: `(canonical id, content, custom elements, build the
/// declaration product)`.
pub(crate) type Composer = fn(&CarrierGrammarAuthority, &str, &str, &[String], bool) -> Composition;

/// The adapter. It has no transport: the CLI driver calls it in process, and
/// a wire server wraps it. Its per-handle state is private.
pub struct ContentMapperProjectionAdapter {
    /// Settings of every open project, by handle.
    projects: Mutex<FxHashMap<MapperProjectHandle, Arc<MapperProjectSettings>>>,
    cache: ProjectionFragmentCache,
    #[cfg(any(test, feature = "test-util"))]
    compositions: AtomicUsize,
    /// The built-in carrier grammars, read-only after construction.
    grammars: CarrierGrammarAuthority,
    /// The descriptor that names the checking module's virtual file.
    descriptor: FrameworkAdapterDescriptor,
    compose: Composer,
}

impl ContentMapperProjectionAdapter {
    /// # Panics
    ///
    /// When a built-in carrier grammar cannot be registered, which means the
    /// compiler's frontend catalog is inconsistent; a host built over the same
    /// catalog fails the same way.
    #[must_use]
    pub fn new() -> Self {
        Self::with_composer(compose_carrier)
    }

    pub(crate) fn with_composer(compose: Composer) -> Self {
        let grammars = CarrierGrammarAuthority::new().expect("carrier grammar authority");
        FrameworkCapabilityCatalog::built_in()
            .expect("every built-in frontend capability row publishes a carrier grammar fact")
            .register_all(&grammars)
            .unwrap_or_else(|error| panic!("register the built-in carrier grammars: {error:?}"));
        Self {
            projects: Mutex::new(FxHashMap::default()),
            cache: ProjectionFragmentCache::new(128),
            #[cfg(any(test, feature = "test-util"))]
            compositions: AtomicUsize::new(0),
            grammars,
            descriptor: vue_descriptor(),
            compose,
        }
    }

    pub fn open_project(
        &self,
        handle: MapperProjectHandle,
        settings: MapperProjectSettings,
    ) -> Result<(), MapperError> {
        let mut projects = self.projects.lock();
        if projects.contains_key(&handle) {
            return Err(MapperError::ProjectAlreadyOpen(handle));
        }
        projects.insert(handle, Arc::new(settings));
        Ok(())
    }

    /// Compose and lower one carrier. Never panics and never fails the
    /// project: a refusal or a caught panic becomes a diagnostic on the carrier.
    pub fn transform(
        &self,
        handle: &MapperProjectHandle,
        file_name: &str,
        content: &str,
    ) -> Result<MapperTransformResult, MapperError> {
        // The settings are taken out of the lock before composing, so a
        // composition that unwinds never holds it.
        let settings = self
            .projects
            .lock()
            .get(handle)
            .cloned()
            .ok_or_else(|| MapperError::UnknownProject(handle.clone()))?;
        let canonical = canonicalize_path(file_name);
        let source_len = content.len() as u32;
        // A carrier of another framework is refused before it is parsed: the
        // descriptor, not its extension, says which files this adapter owns.
        if !self.owns(&canonical) {
            let refused = not_a_carrier(&canonical, CheckingDialect::Ts, source_len);
            return Ok(self.withheld(&canonical, vec![refused]));
        }
        if let Some(cached) = self.cache.get(
            handle,
            &canonical,
            authored_revision(content),
            settings.identity,
        ) {
            return Ok((*cached).clone());
        }
        #[cfg(any(test, feature = "test-util"))]
        self.compositions.fetch_add(1, Ordering::Relaxed);
        let composed = catch_unwind(AssertUnwindSafe(|| {
            (self.compose)(
                &self.grammars,
                &canonical,
                content,
                &settings.custom_elements,
                settings.declarations,
            )
        }));
        let (module, mut diagnostics) = match composed {
            Ok(Composition {
                module: Ok(module),
                parse_diagnostics,
            }) => (module, parse_diagnostics),
            Ok(Composition {
                module: Err(failure),
                mut parse_diagnostics,
            }) => {
                // A refusal the carrier parser already reported under the
                // same code is that one fact again (a `<script>` and
                // `<script setup>` language conflict): it is not reported
                // twice.
                for refused in failure_diagnostics(failure, source_len) {
                    if refused.code == MapperDiagnosticCode::ScriptLangConflict
                        && parse_diagnostics
                            .iter()
                            .any(|parsed| parsed.code == refused.code)
                    {
                        continue;
                    }
                    parse_diagnostics.push(refused);
                }
                return Ok(self.withheld(&canonical, parse_diagnostics));
            }
            Err(payload) => {
                let diagnostic = MapperDiagnostic {
                    start: 0,
                    end: source_len,
                    code: MapperDiagnosticCode::ProjectionInternal,
                    message: format!(
                        "the projection of this carrier failed: {}",
                        panic_message(payload.as_ref())
                    ),
                };
                return Ok(self.withheld(&canonical, vec![diagnostic]));
            }
        };
        let Some(virtual_file) = self.virtual_file(&canonical, module.dialect) else {
            diagnostics.push(not_a_carrier(&canonical, module.dialect, source_len));
            return Ok(self.withheld(&canonical, diagnostics));
        };
        // Recovery's syntax errors, after the carrier's own parse errors; a
        // range the carrier parser already reported is not reported twice.
        for recovered in module.recovery.syntax_diagnostics() {
            if diagnostics.iter().any(|reported| {
                !reported.code.is_warning()
                    && (reported.start, reported.end) == (recovered.start, recovered.end)
            }) {
                continue;
            }
            diagnostics.push(MapperDiagnostic {
                start: recovered.start,
                end: recovered.end,
                code: MapperDiagnosticCode::Syntax,
                message: recovered.message.clone(),
            });
        }
        let plane = ContentMapper::project(
            source_unit_id(&canonical, CHECKING_UNIT_ROLE),
            &ContentId::from_content_bytes(content.as_bytes()),
            module.emission.mapping().clone(),
        );
        let result = MapperTransformResult {
            virtual_file: Arc::from(virtual_file),
            dialect: module.dialect,
            text: Arc::from(module.checking_text()),
            mappings: lower_wire_mappings(&module),
            // A warning is advice on a fully composed module; only an error
            // keeps the result from warming the complete cache.
            complete: module.is_complete()
                && diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.code.is_warning()),
            diagnostics,
            module: Some(Arc::new(module)),
            plane: Some(Arc::new(plane)),
        };
        if result.complete {
            let read_set = ProjectionReadSet {
                carrier: Arc::from(canonical),
                // No external source is bound by this content-only adapter.
                external_sources: Vec::new(),
                settings_identity: settings.identity,
            };
            // This transform produces checking text only. Declaration consumers
            // supply their separate product's source to `revisions_of`.
            let revisions = revisions_of(content, &result, None, &read_set);
            let projects = self.projects.lock();
            // A project can close and reopen while composition is outside the
            // lock. Only the exact settings captured above may admit its result.
            if projects
                .get(handle)
                .is_some_and(|current| Arc::ptr_eq(current, &settings))
            {
                self.cache
                    .admit(handle, read_set, revisions, Arc::new(result.clone()));
            }
        }
        Ok(result)
    }

    /// Drop every result and setting of the project.
    pub fn close_project(&self, handle: &MapperProjectHandle) -> Result<(), MapperError> {
        let mut projects = self.projects.lock();
        projects
            .remove(handle)
            .ok_or_else(|| MapperError::UnknownProject(handle.clone()))?;
        self.cache.close_project(handle);
        Ok(())
    }

    /// Compositions run so far, for tests that prove projection reuse.
    #[cfg(any(test, feature = "test-util"))]
    #[doc(hidden)]
    pub fn composition_count(&self) -> usize {
        self.compositions.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn retained_projections(&self) -> usize {
        self.cache.retained()
    }

    /// True when this adapter composes `file_name`: its descriptor's naming
    /// column gives the file a checking module. A carrier of another
    /// framework is not owned, and [`Self::transform`] refuses it unparsed.
    #[must_use]
    pub fn owns(&self, file_name: &str) -> bool {
        self.virtual_file(&canonicalize_path(file_name), CheckingDialect::Ts)
            .is_some()
    }

    /// The carrier whose checking module, in some dialect, sits at `path`:
    /// what an engine looking for `path` resolves an authored import of that
    /// carrier to. `None` for any other path, the carrier's other companions
    /// included.
    #[must_use]
    pub fn carrier_of_checking_module(&self, path: &str) -> Option<String> {
        let path = canonicalize_path(path);
        let carrier = self
            .descriptor
            .virtual_file_naming
            .as_ref()?
            .carrier_source_for_companion(&path, self.descriptor.carrier_extension().as_deref())?
            .source;
        [
            CheckingDialect::Ts,
            CheckingDialect::Tsx,
            CheckingDialect::Js,
            CheckingDialect::Jsx,
        ]
        .into_iter()
        .any(|dialect| self.virtual_file(&carrier, dialect).as_deref() == Some(path.as_str()))
        .then_some(carrier)
    }

    #[must_use]
    pub fn open_projects(&self) -> usize {
        self.projects.lock().len()
    }

    /// The suffix, with its dot, of a checking module in `dialect`, as the
    /// wire's `extension` names it.
    #[must_use]
    pub fn virtual_extension(&self, dialect: CheckingDialect) -> Option<&'static str> {
        self.descriptor
            .virtual_file_naming
            .as_ref()?
            .checking_module_suffix(dialect.extension())
    }

    fn virtual_file(&self, canonical: &str, dialect: CheckingDialect) -> Option<String> {
        self.descriptor
            .virtual_file_naming
            .as_ref()?
            .checking_module_identity(
                canonical,
                self.descriptor.carrier_extension().as_deref(),
                dialect.extension(),
            )
    }

    /// The result of a carrier with no composed module: an empty module at
    /// the TypeScript checking path, carrying `diagnostics`.
    fn withheld(
        &self,
        canonical: &str,
        diagnostics: Vec<MapperDiagnostic>,
    ) -> MapperTransformResult {
        let dialect = CheckingDialect::Ts;
        let virtual_file = self
            .virtual_file(canonical, dialect)
            .unwrap_or_else(|| canonical.to_string());
        MapperTransformResult {
            virtual_file: Arc::from(virtual_file),
            dialect,
            text: Arc::from(REFUSED_MODULE_TEXT),
            mappings: Vec::new(),
            diagnostics,
            module: None,
            plane: None,
            complete: false,
        }
    }
}

impl Default for ContentMapperProjectionAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Register `content` as the Vue carrier `canonical`, accept it under the
/// registered Vue grammar and compose its checking module. The registration
/// lives in an authority of its own, so nothing of it outlives the call.
pub(crate) fn compose_carrier(
    grammars: &CarrierGrammarAuthority,
    canonical: &str,
    content: &str,
    custom_elements: &[String],
    declarations: bool,
) -> Composition {
    let artifact = match accepted_artifact(grammars, canonical, content) {
        Ok(artifact) => artifact,
        Err(failure) => return failure.into(),
    };
    // The carrier parser's own errors (a missing entry block, a malformed
    // template) are reported whether or not a module composes.
    let parse_diagnostics = artifact
        .common()
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == LanguageDiagnosticSeverity::Error)
        .map(carrier_parse_diagnostic)
        .collect();
    // A `src` script or template has its text in another file, which only a
    // host binds; composing without it would check a carrier that is not
    // there. Style blocks do not enter the checking module.
    let unbound: Vec<(String, u32, u32)> = artifact
        .common()
        .external_links()
        .into_iter()
        .filter(|link| link.kind != ExternalLinkKind::Style && !link.specifier.is_empty())
        .map(|link| {
            let (start, end) = link.span.map_or((0, 0), |span| (span.start, span.end));
            (link.specifier, start, end)
        })
        .collect();
    let module = if unbound.is_empty() {
        compose_checking_module(&CheckingModuleInput {
            source: content,
            artifact: &artifact,
            canonical_id: canonical,
            custom_elements: Some(custom_elements),
            declarations,
            external: None,
        })
        .map_err(CompositionFailure::Refused)
    } else {
        Err(CompositionFailure::UnboundExternal(unbound))
    };
    Composition {
        module,
        parse_diagnostics,
    }
}

pub(crate) fn accepted_artifact(
    grammars: &CarrierGrammarAuthority,
    canonical: &str,
    content: &str,
) -> Result<FrameworkParseArtifact, CompositionFailure> {
    let unregistered = |what: &str| CompositionFailure::Unregistered(what.to_string());
    let language = FileLanguage::vue();
    let grammar = language
        .adapter_id()
        .zip(language.carrier_language_id())
        .and_then(|(adapter, carrier)| {
            crate::carrier_publication_store::registered_carrier_grammar(adapter, carrier)
        })
        .ok_or_else(|| unregistered("no registered Vue carrier grammar"))?;
    let sources = RegisteredSourceAuthority::new()
        .map_err(|_| unregistered("the source authority is unavailable"))?;
    let snapshot = sources
        .register_source(
            CanonicalFileId::new(canonical),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            language,
            Arc::from(content),
        )
        .map_err(|error| CompositionFailure::Unregistered(format!("{error:?}")))?;
    let accepted = grammars
        .accept_registered_source(&sources, &snapshot, grammar)
        .map_err(|error| CompositionFailure::Unregistered(format!("{error:?}")))?;
    Ok(project_registered_accepted(&accepted)
        .map_err(CompositionFailure::Rejected)?
        .into_framework_parse_artifact())
}

/// One carrier parser error, classified by the parser's stable code. A key
/// repeated within one `v-if` chain is an error Vue's compiler refuses, but
/// the chain is well formed and the template is checked through it, so the
/// checker reports it under a warning code of its own.
fn carrier_parse_diagnostic(diagnostic: &LanguageDiagnostic) -> MapperDiagnostic {
    MapperDiagnostic {
        start: diagnostic.span.start,
        end: diagnostic.span.end,
        code: if diagnostic.code == CompilerErrorCode::ScriptLangMismatch.code_str() {
            MapperDiagnosticCode::ScriptLangConflict
        } else if diagnostic.code == CompilerErrorCode::XVIfSameKey.code_str() {
            MapperDiagnosticCode::RepeatedBranchKey
        } else {
            MapperDiagnosticCode::Syntax
        },
        message: diagnostic.message.clone(),
    }
}

/// The refusal of a file that has no checking module in `dialect`.
fn not_a_carrier(canonical: &str, dialect: CheckingDialect, source_len: u32) -> MapperDiagnostic {
    MapperDiagnostic {
        start: 0,
        end: source_len,
        code: MapperDiagnosticCode::ProjectionRefused,
        message: format!(
            "`{canonical}` is not a carrier with a {} checking module",
            dialect.extension()
        ),
    }
}

fn failure_diagnostics(failure: CompositionFailure, source_len: u32) -> Vec<MapperDiagnostic> {
    match failure {
        CompositionFailure::Refused(refusal) => vec![refusal_diagnostic(&refusal, source_len)],
        CompositionFailure::Rejected(SyntaxReject::RejectedSyntax {
            primary, related, ..
        }) => std::iter::once(primary.as_ref())
            .chain(related.iter())
            .map(carrier_parse_diagnostic)
            .collect(),
        CompositionFailure::Rejected(reject) => vec![MapperDiagnostic {
            start: 0,
            end: source_len,
            code: MapperDiagnosticCode::ProjectionRefused,
            message: format!("the carrier could not be parsed: {reject:?}"),
        }],
        CompositionFailure::UnboundExternal(links) => links
            .into_iter()
            .map(|(specifier, start, end)| MapperDiagnostic {
                start,
                end,
                code: MapperDiagnosticCode::ProjectionRefused,
                message: format!(
                    "`{specifier}` is an external block, which this mapper cannot bind: \
                     it reads only the carrier's own content, so the carrier is not checked"
                ),
            })
            .collect(),
        CompositionFailure::Unregistered(reason) => vec![MapperDiagnostic {
            start: 0,
            end: source_len,
            code: MapperDiagnosticCode::ProjectionInternal,
            message: format!("the carrier could not be registered: {reason}"),
        }],
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic with no message")
}

/// Lower a module's emission to wire mappings.
///
/// Each projected region of the emission's mapping product with a carrier
/// preimage becomes one mapping: exact authored bytes (`Identity`,
/// `Relocated`) are `Verbatim`, the only kind edits may be written back
/// through; rewritten text is `Atom`, answered region to region, including a
/// region that stands at an authored point and so names no authored extent.
/// Synthesized text has no mapping. The product's declared insertion anchors
/// and the end of the module, which no region covers, are answered by
/// zero-width anchors (the end of the module stands at the end of the
/// carrier), never by stretching the last region. Features come from the
/// emission's role-qualified observations: a region gets a role's features
/// only when an observation of that role covers it whole.
#[must_use]
pub fn lower_wire_mappings(module: &CheckingModule) -> Vec<WireSpanMapping> {
    let product = module.emission.mapping();
    let observations = module.emission.observations();
    let mut mappings = Vec::with_capacity(product.projected().len() + 1);
    for region in product.projected() {
        let (Some(carrier), kind) = (region.carrier, region.class) else {
            continue;
        };
        let kind = match kind {
            ProjectedClass::Identity | ProjectedClass::Relocated => WireSpanKind::Verbatim,
            ProjectedClass::Rewritten => WireSpanKind::Atom,
            ProjectedClass::Synthesized => continue,
        };
        let features = observations
            .iter()
            .filter(|observation| covers(observation.generated(), region.generated))
            .fold(0, |features, observation| {
                features | role_features(observation.role(), kind)
            });
        mappings.push(WireSpanMapping {
            virtual_start: region.generated.start,
            virtual_length: region.generated.len(),
            original_start: carrier.start,
            original_length: carrier.len(),
            kind,
            features,
        });
    }
    let anchor = |projected: u32, carrier: u32| WireSpanMapping {
        virtual_start: projected,
        virtual_length: 0,
        original_start: carrier,
        original_length: 0,
        kind: WireSpanKind::Atom,
        features: 0,
    };
    mappings.extend(
        product
            .insertion_anchors()
            .iter()
            .map(|declared| anchor(declared.projected, declared.carrier)),
    );
    let end = product.projected_len();
    if !product
        .insertion_anchors()
        .iter()
        .any(|declared| declared.projected == end)
    {
        mappings.push(anchor(end, product.carrier_len()));
    }
    mappings.sort_by_key(|mapping| (mapping.virtual_start, mapping.virtual_length));
    mappings
}

fn covers(outer: MappingSpan, inner: MappingSpan) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// The features an observation role grants a mapping of `kind`. Edits are
/// granted only through `Verbatim` mappings, the length-preserving kind;
/// a diagnostic observation is not a feature, since a feature mask never
/// suppresses diagnostics.
fn role_features(role: ObservationRole, kind: WireSpanKind) -> u32 {
    use wire_features::*;
    match role {
        ObservationRole::Hover => HOVER,
        ObservationRole::Definition => DEFINITION | TYPE_DEFINITION,
        ObservationRole::References => REFERENCES | DOCUMENT_HIGHLIGHTS,
        ObservationRole::Edits if kind == WireSpanKind::Verbatim => RENAME,
        ObservationRole::Edits | ObservationRole::Diagnostic => 0,
        ObservationRole::Feature => COMPLETION | SIGNATURE_HELP,
    }
}

/// Refusal to diagnostic text. One place, so every consumer words it the same.
/// A language conflict between the script blocks keeps the code the carrier
/// parser reports it under, so the one fact has one code.
#[must_use]
pub fn refusal_diagnostic(refusal: &CheckingModuleRefusal, source_len: u32) -> MapperDiagnostic {
    let (code, message) = match refusal {
        CheckingModuleRefusal::Script(script) => (
            match script {
                SetupProjectionRefusal::ScriptLangConflict => {
                    MapperDiagnosticCode::ScriptLangConflict
                }
                _ => MapperDiagnosticCode::ProjectionRefused,
            },
            match script {
                SetupProjectionRefusal::NotTypeScript => {
                    "the script language is neither JavaScript nor TypeScript, so the carrier has no checking module"
                        .to_string()
                }
                SetupProjectionRefusal::ScriptLangConflict => {
                    "`<script>` and `<script setup>` declare different languages".to_string()
                }
                SetupProjectionRefusal::SyntaxErrors { setup: true } => {
                    "the `<script setup>` block has syntax errors".to_string()
                }
                SetupProjectionRefusal::SyntaxErrors { setup: false } => {
                    "the `<script>` block has syntax errors".to_string()
                }
                SetupProjectionRefusal::InvalidGeneric => {
                    "the `generic` attribute does not parse as type parameters".to_string()
                }
                SetupProjectionRefusal::MissingParse => {
                    "no parse of this carrier's content backs the projection".to_string()
                }
            },
        ),
        CheckingModuleRefusal::External(legality) => (
            MapperDiagnosticCode::ProjectionRefused,
            match legality {
                ExternalLegality::Admitted => "an external block was refused".to_string(),
                ExternalLegality::ScriptSetupSrc => {
                    "`<script setup>` cannot take a `src` attribute".to_string()
                }
                ExternalLegality::DialectMismatch => {
                    "an external block's language differs from its sibling block's".to_string()
                }
                ExternalLegality::UnsupportedLanguage(language) => {
                    format!("the external block's language `{language}` is not processed")
                }
                ExternalLegality::Unreadable => {
                    "an external block's source could not be read".to_string()
                }
            },
        ),
        CheckingModuleRefusal::FragmentInvariant { family, detail } => (
            MapperDiagnosticCode::ProjectionInternal,
            format!("the {family:?} projection of this carrier is defective: {detail}"),
        ),
        CheckingModuleRefusal::Emission(refusal) => (
            MapperDiagnosticCode::ProjectionInternal,
            format!("the checking module could not be emitted: {refusal:?}"),
        ),
        CheckingModuleRefusal::Transform(error) => (
            MapperDiagnosticCode::ProjectionInternal,
            format!("the checking module's edits could not be applied: {error:?}"),
        ),
    };
    MapperDiagnostic {
        start: 0,
        end: source_len,
        code,
        message,
    }
}

// ── Wire messages ────────────────────────────────────────────────────

/// The diagnostic source the mapper declares: the engine prints mapper
/// diagnostics as `verter<code>`. Not a name the protocol reserves.
pub const MAPPER_DIAGNOSTIC_SOURCE: &str = "verter";

/// The one position encoding the mapper speaks: every offset here is a
/// UTF-8 byte offset.
pub const MAPPER_POSITION_ENCODING: &str = "utf-8";

/// `initialize` parameters.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// The encodings the engine offers, in preference order.
    #[serde(default)]
    pub position_encodings: Vec<String>,
    #[serde(default)]
    pub locale: Option<String>,
}

/// `initialize` result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub position_encoding: &'static str,
    pub diagnostic_source: &'static str,
}

/// `openProject` parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectParams {
    pub project_handle: String,
    #[serde(default)]
    pub config_file_name: Option<String>,
    /// The `options` object of the project's `contentMappers` entry.
    #[serde(default)]
    pub options: Option<serde_json::Value>,
    /// The values of the compiler options the mapper's manifest declares.
    #[serde(default)]
    pub compiler_options: Option<serde_json::Map<String, serde_json::Value>>,
}

impl OpenProjectParams {
    /// The project's handle and settings. The declared compiler options are
    /// the settings' option dependencies; no custom elements are configured
    /// through the wire.
    #[must_use]
    pub fn into_project(self) -> (MapperProjectHandle, MapperProjectSettings) {
        let declared = self
            .compiler_options
            .iter()
            .flatten()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        let config_file: Arc<str> = Arc::from(
            self.config_file_name
                .as_deref()
                .map_or_else(String::new, canonicalize_path),
        );
        (
            MapperProjectHandle(Arc::from(self.project_handle)),
            MapperProjectSettings::new(config_file, Vec::new(), declared, Vec::new()),
        )
    }
}

/// `openProject` result. The mapper reads no configuration of its own, so
/// it returns no configuration identity and watches no files.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectResult {
    pub option_diagnostics: Vec<serde_json::Value>,
}

/// `transform` parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformParams {
    pub project_handle: String,
    pub file_name: String,
    pub content: String,
}

/// One mapper diagnostic on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireMapperDiagnostic {
    pub message_text: String,
    pub start: u32,
    pub length: u32,
    pub code: u32,
}

/// `transform` result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformResultMessage {
    pub text: Arc<str>,
    pub extension: &'static str,
    pub mappings: Vec<[u32; 6]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<WireMapperDiagnostic>,
}

/// `closeProject` parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseProjectParams {
    pub project_handle: String,
}

impl InitializeParams {
    /// The encoding to answer with: UTF-8 when the engine offers it, `None`
    /// when it does not, since the mapper speaks no other encoding.
    #[must_use]
    pub fn negotiated_encoding(&self) -> Option<&'static str> {
        self.position_encodings
            .iter()
            .any(|offered| offered == MAPPER_POSITION_ENCODING)
            .then_some(MAPPER_POSITION_ENCODING)
    }
}

impl MapperTransformResult {
    /// The wire result. `extension` is the checking module's suffix as the
    /// adapter's naming column names it.
    #[must_use]
    pub fn to_wire(&self, extension: &'static str) -> TransformResultMessage {
        TransformResultMessage {
            text: Arc::clone(&self.text),
            extension,
            mappings: self.mappings.iter().map(WireSpanMapping::to_wire).collect(),
            diagnostics: self
                .diagnostics
                .iter()
                .map(|diagnostic| WireMapperDiagnostic {
                    message_text: diagnostic.message.clone(),
                    start: diagnostic.start,
                    length: diagnostic.end.saturating_sub(diagnostic.start),
                    code: diagnostic.code.wire_code(),
                })
                .collect(),
        }
    }
}
