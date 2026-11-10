//! Resolver port reads as retained read-site transitions.
//!
//! A family stage that reads the resolver — a global declaration lookup, a
//! class's declaration reads, a declaration's import/export route — names
//! the read as a typed port ([`PortRead`]) the family stands at
//! ([`FamilyStep::Port`](super::family_continuation::FamilyStep::Port)),
//! holding its position and everything it computed. The driver answers the
//! port in place ([`PortAnswer`]) over the files the build already named
//! as `Load` transitions, whose deliveries and failures the suspended owner
//! retains, and resumes the stage with the answer. A port read therefore
//! loads nothing on its own: every file a port stands on is a `Load` the
//! build named first, and a port that finds its file still loading on a
//! production poll's drive waits on that load with the build retained.
//!
//! Two of the resolver's lookups are written here as continuations of
//! their own, each stepped by the family that holds it:
//!
//! - [`GlobalLookup`]: the first declaration of a global type — the
//!   project's library, then the program's contributors — with the library
//!   file and a `declare global` contributor read as `Load` transitions
//!   instead of in place;
//! - [`ResolveDeclBuild`]: a declaration lookup that follows its scope's
//!   import/export route hop by hop, each barrel a `Load` transition and
//!   each specifier a port read, until it stands at the defining
//!   declaration it then reads as a query need.

use std::sync::Arc;

use verter_session_query::inputs::contributors::SymbolContributors;
use verter_session_query::inputs::indexed::IndexedInputServe;
use verter_session_query::inputs::shallow::{ExportTarget, ShallowInputRecord};
use verter_session_query::type_solver::host::ResolvedRootIdentity;
use verter_type_expr::OwnedStr;

use super::continuation_stages::{ClassStaticAnswer, ClassStaticRead};
use super::family_continuation::{ContinuationStage, FamilyBuild, FamilyStep};
use crate::project_semantic_dispatch::walk::QueryBuildOutput;
use crate::semantic_query::{
    CacheRead, NodeScopeId, QueryError, QueryResult, ResolveDeclKey, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey,
};
use crate::semantic_query_memo::ObservedGraphSelfRoot;

/// A resolver port a family stage stands at. Each names what it reads and
/// nothing of how: the driver answers it in place
/// ([`ProjectSemanticDispatch::answer_port_in_place`](super::ProjectSemanticDispatch::answer_port_in_place)),
/// over the retained deliveries of the files the build loaded.
pub(super) enum PortRead<'b> {
    /// The declaration of the global type `name` the library of `demand`'s
    /// project holds, by the project's symbol index alone: the library
    /// file itself is read as a `Load` that follows.
    GlobalLib { demand: &'b str, name: &'b str },
    /// The contributors recorded at construction or upsert that the
    /// population read would make IndexedReady in place, classified but
    /// not materialized: the lookup reads each as a `Load` first.
    GlobalPending,
    /// The program's contributors to the global type `name`, read for
    /// `demand`: the population index, observed onto the active tracer.
    GlobalContributors { demand: &'b str, name: &'b str },
    /// A class's static surface's declaration read.
    ClassStatic(ClassStaticRead<'b>),
    /// The canonical the specifier `specifier`, authored in `importer`,
    /// resolves to: workspace resolution.
    Dependency {
        importer: &'b str,
        specifier: &'b str,
    },
    /// The bare-name resolver over the declaration scope `(canonical,
    /// owner)`, for a route the hop walk could not settle from the shallow
    /// inventories it read.
    BareName {
        canonical: &'b str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &'b str,
    },
}

impl PortRead<'_> {
    /// The file the port reads: the port stands on a file its build named
    /// as a `Load`, so a frame that finds it still loading waits on the
    /// load first, with the build retained, instead of letting the port
    /// load it in place. `None` for an index or workspace read.
    pub(super) fn reads_file(&self) -> Option<&str> {
        match self {
            Self::GlobalLib { .. }
            | Self::GlobalPending
            | Self::GlobalContributors { .. }
            | Self::Dependency { .. } => None,
            Self::ClassStatic(read) => Some(read.reads_file()),
            Self::BareName { canonical, .. } => Some(canonical),
        }
    }
}

/// The answer to a [`PortRead`], in the port's own domain.
pub(super) enum PortAnswer {
    GlobalLib(Option<ResolvedRootIdentity>),
    GlobalPending(Vec<Arc<str>>),
    GlobalContributors(SymbolContributors),
    ClassStatic(ClassStaticAnswer),
    Dependency(Option<Arc<str>>),
    BareName(Option<ResolvedRootIdentity>),
}

// A port's answer is owned state the frame that answered it resumes its
// build with, on whichever thread polls next.
static_assertions::assert_impl_all!(PortAnswer: Send);

// ---------------------------------------------------------------------------
// Global declaration lookup
// ---------------------------------------------------------------------------

/// The lookup of the first declaration of a global type, as a continuation:
/// the library's declaration (its symbol index, then the declaring library
/// file's headers), else the program's first contributor in declaration
/// precedence order (the population index, then — for a module's `declare
/// global` contribution — the module's own inventory, which decides whether
/// its module scope shadows the name). The library file, each contributor
/// the population read would otherwise index in place, and the selected
/// contributor are `Load` transitions of the family that holds the lookup;
/// the index reads are ports.
pub(super) struct GlobalLookup {
    demand: Arc<str>,
    name: Arc<str>,
    stage: GlobalStage,
}

enum GlobalStage {
    /// The library's symbol index is asked next.
    Lib,
    /// The library file `identity` declares in is being read.
    LibSource(ResolvedRootIdentity),
    /// The contributors still pending ingestion are asked next.
    Pending,
    /// The pending contributors are being read, `remaining` last first, so
    /// the population read that follows indexes none of them in place.
    PendingLoads { remaining: Vec<Arc<str>> },
    /// The program's contributor population is asked next.
    Population,
    /// The module `canonical`'s `declare global` contribution is being
    /// read, to decide whether its module scope shadows the name.
    ContributorSource {
        canonical: OwnedStr,
        owner: verter_type_expr::TopLevelOwnerId,
    },
    /// The lookup answered.
    Done,
}

/// Where a global lookup stands after its latest stage.
pub(super) enum GlobalStep {
    /// The lookup reads the port it names next ([`GlobalLookup::port`]).
    Port,
    /// The lookup reads this file's indexed input next.
    Load(Arc<str>),
    /// The declaration found, or none.
    Found(Option<ResolvedRootIdentity>),
}

impl GlobalLookup {
    /// Begin the lookup of the global type `name` a reference in `demand`
    /// reads: its library's symbol index is asked first.
    pub(super) fn begin(demand: Arc<str>, name: Arc<str>) -> (Self, GlobalStep) {
        (
            Self {
                demand,
                name,
                stage: GlobalStage::Lib,
            },
            GlobalStep::Port,
        )
    }

    /// The port the lookup stands at.
    pub(super) fn port(&self) -> PortRead<'_> {
        match self.stage {
            GlobalStage::Lib => PortRead::GlobalLib {
                demand: &self.demand,
                name: &self.name,
            },
            GlobalStage::Pending => PortRead::GlobalPending,
            GlobalStage::Population => PortRead::GlobalContributors {
                demand: &self.demand,
                name: &self.name,
            },
            GlobalStage::LibSource(_)
            | GlobalStage::PendingLoads { .. }
            | GlobalStage::ContributorSource { .. } => {
                unreachable!("a global lookup reading a file stands at no port")
            }
            GlobalStage::Done => unreachable!("a global lookup that answered reads nothing"),
        }
    }

    /// Resume the lookup with the answer to the port it stood at.
    pub(super) fn resume_port(&mut self, answer: PortAnswer) -> GlobalStep {
        match (
            std::mem::replace(&mut self.stage, GlobalStage::Done),
            answer,
        ) {
            (GlobalStage::Lib, PortAnswer::GlobalLib(Some(identity))) => {
                let file = Arc::from(identity.canonical_id.as_ref());
                self.stage = GlobalStage::LibSource(identity);
                GlobalStep::Load(file)
            }
            (GlobalStage::Lib, PortAnswer::GlobalLib(None)) => {
                self.stage = GlobalStage::Pending;
                GlobalStep::Port
            }
            (GlobalStage::Pending, PortAnswer::GlobalPending(mut pending)) => {
                pending.reverse();
                self.load_pending(pending)
            }
            (GlobalStage::Population, PortAnswer::GlobalContributors(population)) => {
                use verter_session_query::inputs::contributors::ContributorOrigin;
                let Some(first) = population
                    .entries
                    .iter()
                    .find(|entry| super::build::declares_global_type(entry))
                else {
                    return GlobalStep::Found(None);
                };
                let canonical = OwnedStr::from(&first.artifact_key.canonical);
                if first.origin == ContributorOrigin::DeclareGlobal {
                    let file = Arc::from(canonical.as_ref());
                    self.stage = GlobalStage::ContributorSource {
                        canonical,
                        owner: first.owner,
                    };
                    return GlobalStep::Load(file);
                }
                GlobalStep::Found(Some(ResolvedRootIdentity::new_in_owner(
                    canonical,
                    first.owner,
                    self.name.as_ref(),
                )))
            }
            _ => unreachable!("a global lookup's port answers the stage that named it"),
        }
    }

    /// Read the next pending contributor, or — none left — the population.
    fn load_pending(&mut self, mut remaining: Vec<Arc<str>>) -> GlobalStep {
        match remaining.pop() {
            Some(file) => {
                self.stage = GlobalStage::PendingLoads { remaining };
                GlobalStep::Load(file)
            }
            None => {
                self.stage = GlobalStage::Population;
                GlobalStep::Port
            }
        }
    }

    /// Resume the lookup with the indexed input of the file it read (`None`
    /// when the live view serves none).
    pub(super) fn resume_source(&mut self, serve: Option<IndexedInputServe>) -> GlobalStep {
        match std::mem::replace(&mut self.stage, GlobalStage::Done) {
            // A library the view cannot serve is still the declaring file:
            // the read of its declaration that follows fails as that read
            // does.
            GlobalStage::LibSource(identity) => {
                let Some(serve) = serve else {
                    return GlobalStep::Found(Some(identity));
                };
                let declared = serve
                    .indexed
                    .shallow_state
                    .headers
                    .type_header_in(identity.owner, &self.name)
                    .is_some();
                if declared {
                    return GlobalStep::Found(Some(identity));
                }
                self.stage = GlobalStage::Pending;
                GlobalStep::Port
            }
            // A pending contributor is read only so the population read
            // finds it indexed; what it served is the population's to use.
            GlobalStage::PendingLoads { remaining } => {
                drop(serve);
                self.load_pending(remaining)
            }
            // A module's `declare global` contribution is addressed by the
            // owner's global-augmentation scope when the module also binds
            // the name in module scope, which its own references read.
            GlobalStage::ContributorSource { canonical, owner } => {
                let shadowed = serve.is_some_and(|serve| {
                    serve
                        .indexed
                        .shallow_state
                        .binds_module_scope_type_in(owner, &self.name)
                });
                GlobalStep::Found(Some(ResolvedRootIdentity::new_in_owner(
                    canonical,
                    if shadowed {
                        owner.global_augmentation()
                    } else {
                        owner
                    },
                    self.name.as_ref(),
                )))
            }
            _ => unreachable!("a global lookup reads a file only for a declaration"),
        }
    }
}

// ---------------------------------------------------------------------------
// Declaration lookup along its route
// ---------------------------------------------------------------------------

/// The longest import/export route a declaration lookup follows before it
/// asks the in-place resolver instead.
const MAX_ROUTE_HOPS: u32 = 64;

/// A declaration lookup between its stages: the scope it was asked in, the
/// files its route read with the content versions they served, and where
/// along the route it stands.
pub(super) struct ResolveDeclBuild {
    key: ResolveDeclKey,
    /// Every file the route read, which the result roots on: a content edit
    /// to any barrel's re-export clause or to the defining file misses the
    /// warm read.
    roots: Vec<ObservedGraphSelfRoot>,
    /// The `(file, name)` positions the route visited, which bound it.
    visited: rustc_hash::FxHashSet<(Arc<str>, Arc<str>)>,
    /// Whether the scope's own inventory names the symbol as an export or an
    /// import, so an unsettled route answers the scope's placeholder rather
    /// than a miss.
    placeholder_allowed: bool,
    hops: u32,
    stage: ResolveDeclStage,
}

enum ResolveDeclStage {
    /// Reading the scope file.
    Scope,
    /// Reading `file`, where `name` is looked for.
    Hop { file: Arc<str>, name: Arc<str> },
    /// Resolving `specifier`, authored in `importer`, to the file `name` is
    /// looked for in next.
    Specifier {
        importer: Arc<str>,
        specifier: String,
        name: Arc<str>,
    },
    /// Asking the in-place bare-name resolver over the scope.
    Fallback,
    /// Reading the defining declaration the route reached.
    Target,
    /// The lookup answered.
    Done,
}

impl ResolveDeclBuild {
    fn new(key: ResolveDeclKey) -> Self {
        Self {
            key,
            roots: Vec::new(),
            visited: rustc_hash::FxHashSet::default(),
            placeholder_allowed: false,
            hops: 0,
            stage: ResolveDeclStage::Scope,
        }
    }

    /// The port the lookup stands at.
    pub(super) fn port_read(&self) -> PortRead<'_> {
        match &self.stage {
            ResolveDeclStage::Specifier {
                importer,
                specifier,
                ..
            } => PortRead::Dependency {
                importer,
                specifier,
            },
            ResolveDeclStage::Fallback => PortRead::BareName {
                canonical: self.key.scope.canonical_id.as_ref(),
                owner: self.key.scope.owner,
                name: self.key.name.as_ref(),
            },
            ResolveDeclStage::Scope | ResolveDeclStage::Hop { .. } => {
                unreachable!("a declaration lookup reading a file stands at no port")
            }
            ResolveDeclStage::Target => {
                unreachable!("a declaration lookup reading its target stands at no port")
            }
            ResolveDeclStage::Done => {
                unreachable!("a declaration lookup that answered reads nothing")
            }
        }
    }

    /// The key of `name` in `owner`'s file scope of `file`.
    fn key_in_owner(
        file: &Arc<str>,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> ResolveDeclKey {
        ResolveDeclKey {
            scope: crate::semantic_query::ScopeId {
                canonical_id: OwnedStr::from(file),
                owner,
                local_scope: None,
                binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(owner),
            },
            name: OwnedStr::from(name),
        }
    }

    /// The one route step out of a file's inventory for `name` in `owner`,
    /// when the inventory settles it unambiguously: the specifier and name
    /// to follow. In the declaration's own scope (`at_scope`) the name's
    /// import binding is the route; everywhere the export surface is —
    /// `export { X } from`, or `export { A as X }` of a local binding that
    /// is itself an import — and, with no explicit export, the file's
    /// `export *` when it has exactly one. A hop never follows a private
    /// import, and two or more wildcards are a precedence the inventories
    /// alone cannot order: `None`, and the caller asks the in-place
    /// resolver, which applies the full route policy.
    fn unambiguous_route(
        shallow: &ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        at_scope: bool,
    ) -> Option<(String, String)> {
        let follow_import = |binding_owner: verter_type_expr::TopLevelOwnerId, local: &str| {
            let target = shallow.import_target_in(binding_owner, local)?;
            if target.is_namespace {
                return None;
            }
            Some((
                target.source_specifier.clone(),
                target.imported_name.clone(),
            ))
        };
        if at_scope {
            // A local export of the name in the declaration's own scope —
            // `import { X } from './leaf'; export { X }`, or `export { A as
            // X }` — is the scope's own identity to the full resolver,
            // which answers the scope's placeholder: not a route.
            if matches!(
                shallow.export_target(name),
                Some(ExportTarget::Local { .. })
            ) {
                return None;
            }
            if let Some(route) = follow_import(owner, name) {
                return Some(route);
            }
        }
        if owner != verter_type_expr::TopLevelOwnerId::ordinary_file() {
            return None;
        }
        match shallow.export_target(name) {
            Some(ExportTarget::Reexport {
                source_specifier,
                original_name,
                ..
            }) => Some((source_specifier.clone(), original_name.clone())),
            Some(ExportTarget::Local {
                owner: local_owner,
                symbol_name,
            }) => follow_import(*local_owner, symbol_name),
            None => {
                let mut wildcards = shallow
                    .wildcard_reexports
                    .iter()
                    .filter(|wildcard| wildcard.owner == owner);
                let only = wildcards.next()?;
                if wildcards.next().is_some() {
                    return None;
                }
                Some((only.source_specifier.clone(), name.to_owned()))
            }
        }
    }

    /// Follow `specifier` out of `importer` for `name`: the specifier is
    /// resolved next.
    fn follow(mut self, importer: Arc<str>, specifier: String, name: Arc<str>) -> FamilyStep {
        self.hops += 1;
        if self.hops > MAX_ROUTE_HOPS {
            return self.fall_back();
        }
        self.stage = ResolveDeclStage::Specifier {
            importer,
            specifier,
            name,
        };
        FamilyStep::Port(Box::new(FamilyBuild::ResolveDecl(Box::new(self))))
    }

    /// Ask the in-place resolver over the scope: the route the inventories
    /// read could not settle — an unresolved specifier, an unservable hop,
    /// a cycle, or a shape only the full route policy orders.
    fn fall_back(mut self) -> FamilyStep {
        self.stage = ResolveDeclStage::Fallback;
        FamilyStep::Port(Box::new(FamilyBuild::ResolveDecl(Box::new(self))))
    }

    /// Read the defining declaration `target` as a query need, or give the
    /// build back when the route came back to the scope itself.
    fn read_target(mut self, target: ResolveDeclKey) -> Result<FamilyStep, Box<Self>> {
        if target.scope.canonical_id.as_ref() == self.key.scope.canonical_id.as_ref()
            && target.scope.owner == self.key.scope.owner
            && target.name.as_ref() == self.key.name.as_ref()
        {
            return Err(Box::new(self));
        }
        self.stage = ResolveDeclStage::Target;
        Ok(FamilyStep::Need(
            SemanticQueryKey::ResolveDecl(target),
            Box::new(FamilyBuild::ResolveDecl(Box::new(self))),
        ))
    }
}

impl<'s, 'a: 's, C: crate::resolver_core::ResolverCapabilities> ContinuationStage<'s, 'a, C> {
    /// The first stage of a declaration lookup: its scope file's indexed
    /// input is read next.
    #[inline(never)]
    pub(super) fn begin_resolve_decl(&self, key: &ResolveDeclKey) -> FamilyStep {
        self.graph().record_continuation_begun();
        FamilyStep::Load(
            Arc::from(key.scope.canonical_id.as_ref()),
            Box::new(FamilyBuild::ResolveDecl(Box::new(ResolveDeclBuild::new(
                key.clone(),
            )))),
        )
    }

    /// Resume a declaration lookup with the indexed input of the file it
    /// read (`None` when the live view serves none): its scope, whose
    /// inventory answers locally or names the first hop of the route; or a
    /// hop, whose inventory names the defining declaration, the next hop,
    /// or nothing.
    #[inline(never)]
    pub(super) fn resume_resolve_decl_source(
        &self,
        mut build: ResolveDeclBuild,
        serve: Option<IndexedInputServe>,
    ) -> FamilyStep {
        verter_audit::attribute_scope!(ResolveDecl);
        match std::mem::replace(&mut build.stage, ResolveDeclStage::Done) {
            ResolveDeclStage::Scope => {
                let Some(serve) = serve else {
                    return FamilyStep::Done(Box::new(
                        (
                            QueryResult::Error(QueryError::Miss),
                            super::empty_signature(),
                        )
                            .into(),
                    ));
                };
                let shallow: &ShallowInputRecord = &serve.indexed.shallow_state;
                let key = &build.key;
                build
                    .roots
                    .push((key.scope.canonical_id.clone(), serve.indexed.whole_hash));
                if super::build::resolve_decl_is_local(key, shallow) {
                    return FamilyStep::Done(Box::new(self.resolve_decl_placeholder(build)));
                }
                // Re-export fall-through. A barrel reaches the declaration
                // via `export * from './base'`, `export { X } from './base'`
                // or `import { X } ...; export { X }`. In every shape the
                // declaration is authored elsewhere: follow the export graph
                // (direct > aliased > wildcard) to the defining `(canonical,
                // name)` and resolve THAT declaration, so the declaration's
                // scope — and every member's `declaration_origin` — labels
                // the file the author wrote it in, not the barrel hop.
                if !super::build::resolve_decl_needs_route(key, shallow) {
                    return FamilyStep::Done(Box::new(
                        (
                            QueryResult::Error(QueryError::Miss),
                            super::empty_signature(),
                        )
                            .into(),
                    ));
                }
                let owner = key.scope.owner;
                let is_ordinary = owner == verter_type_expr::TopLevelOwnerId::ordinary_file();
                let placeholder_allowed = (is_ordinary
                    && shallow.exports.contains_key(key.name.as_ref()))
                    || shallow.import_target_in(owner, key.name.as_ref()).is_some();
                let scope: Arc<str> = Arc::from(key.scope.canonical_id.as_ref());
                let name: Arc<str> = Arc::from(key.name.as_ref());
                build.placeholder_allowed = placeholder_allowed;
                build
                    .visited
                    .insert((Arc::clone(&scope), Arc::clone(&name)));
                Self::route_out_of(build, shallow, scope, owner, name, true)
            }
            ResolveDeclStage::Hop { file, name } => {
                let Some(serve) = serve else {
                    return build.fall_back();
                };
                let shallow: &ShallowInputRecord = &serve.indexed.shallow_state;
                build
                    .roots
                    .push((OwnedStr::from(&file), serve.indexed.whole_hash));
                let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
                // A hop answers through its export surface: a locally
                // declared export is the defining declaration.
                if let Some(ExportTarget::Local {
                    owner: local_owner,
                    symbol_name,
                }) = shallow.export_target(&name)
                {
                    let target = ResolveDeclBuild::key_in_owner(&file, *local_owner, symbol_name);
                    if super::build::resolve_decl_is_local(&target, shallow) {
                        return match build.read_target(target) {
                            Ok(step) => step,
                            Err(build) => (*build).fall_back(),
                        };
                    }
                }
                Self::route_out_of(build, shallow, file, owner, name, false)
            }
            _ => unreachable!("a declaration lookup reads a file only for its scope or a hop"),
        }
    }

    /// Follow the one route `file`'s inventory names for `name` in `owner`
    /// ([`ResolveDeclBuild::unambiguous_route`]), or ask the in-place
    /// resolver when it names none.
    fn route_out_of(
        build: ResolveDeclBuild,
        shallow: &ShallowInputRecord,
        file: Arc<str>,
        owner: verter_type_expr::TopLevelOwnerId,
        name: Arc<str>,
        at_scope: bool,
    ) -> FamilyStep {
        match ResolveDeclBuild::unambiguous_route(shallow, owner, &name, at_scope) {
            Some((specifier, imported)) => build.follow(file, specifier, Arc::from(imported)),
            None => build.fall_back(),
        }
    }

    /// Resume a declaration lookup with the answer to the port it stood at:
    /// the canonical a hop's specifier resolved to, read next as a hop, or
    /// the in-place resolver's target.
    #[inline(never)]
    pub(super) fn resume_resolve_decl_port(
        &self,
        mut build: ResolveDeclBuild,
        answer: PortAnswer,
    ) -> FamilyStep {
        verter_audit::attribute_scope!(ResolveDecl);
        match (
            std::mem::replace(&mut build.stage, ResolveDeclStage::Done),
            answer,
        ) {
            (ResolveDeclStage::Specifier { name, .. }, PortAnswer::Dependency(Some(file))) => {
                if !build.visited.insert((Arc::clone(&file), Arc::clone(&name))) {
                    return build.fall_back();
                }
                let next = Arc::clone(&file);
                build.stage = ResolveDeclStage::Hop { file, name };
                FamilyStep::Load(next, Box::new(FamilyBuild::ResolveDecl(Box::new(build))))
            }
            // An explicit route whose specifier does not resolve is the
            // route's answer, not a reason to try another: the in-place
            // resolver decides what the scope then names.
            (ResolveDeclStage::Specifier { .. }, PortAnswer::Dependency(None)) => build.fall_back(),
            (ResolveDeclStage::Fallback, PortAnswer::BareName(found)) => {
                if let Some(target) = found {
                    let target = ResolveDeclBuild::key_in_owner(
                        &Arc::from(target.canonical_id.as_ref()),
                        target.owner,
                        target.symbol_name.as_ref(),
                    );
                    build = match build.read_target(target) {
                        Ok(step) => return step,
                        Err(build) => *build,
                    };
                }
                if build.placeholder_allowed {
                    FamilyStep::Done(Box::new(self.resolve_decl_placeholder(build)))
                } else {
                    // The export graph could not resolve the defining site
                    // (e.g. an unresolvable specifier) and the scope's own
                    // inventory names the symbol only through a wildcard.
                    FamilyStep::Done(Box::new(
                        (
                            QueryResult::Error(QueryError::Miss),
                            super::empty_signature(),
                        )
                            .into(),
                    ))
                }
            }
            _ => unreachable!("a declaration lookup's port answers the stage that named it"),
        }
    }

    /// Finish a declaration lookup with the read of the defining declaration
    /// its route reached: the target's answer, rooted on every file the
    /// route read and on the declaration's own self-roots.
    #[inline(never)]
    pub(super) fn finish_resolve_decl(
        &self,
        build: ResolveDeclBuild,
        read: CacheRead<QueryResult<SemanticNodeId>>,
    ) -> FamilyStep {
        let completeness = read.fact_status.completeness();
        let mut roots = build.roots;
        if let QueryResult::Value(node) = &read.value {
            roots.extend(self.observed_self_roots_from_nodes([*node]));
        }
        let mut output: QueryBuildOutput = (read.value, read.dep_signature).into();
        output.cache_suppress = read.cache_suppress;
        output.merge_completeness(completeness);
        FamilyStep::Done(Box::new(output.with_observed_self_roots(roots)))
    }

    /// The scope-local placeholder a declaration lookup answers: a
    /// `DeclPlaceholder` carrying enough identity for callers to construct
    /// `Instantiate` keys, with the declaration's origin scope in the
    /// sidecar so dispatch builders reached from it route per-base-scope
    /// lookups through the scope's declaration-scope payload. It roots on
    /// the scope's observed content version — the one observation that also
    /// supplied the inventory this lookup read, so a concurrent edit cannot
    /// tear them — and on every route file read before it.
    fn resolve_decl_placeholder(&self, build: ResolveDeclBuild) -> QueryBuildOutput {
        let key = &build.key;
        let (_, observed_hash) = build
            .roots
            .first()
            .expect("the lookup read its scope before answering its placeholder");
        let observed_hash = *observed_hash;
        let scope = NodeScopeId::File {
            canonical_id: key.scope.canonical_id.clone(),
            owner: key.scope.owner,
            whole_hash: observed_hash,
            local_scope: key.scope.local_scope.map(|ordinal| {
                crate::semantic_query::NodeLocalScopeId::Lexical(
                    crate::semantic_query::LocalScopeId(ordinal),
                )
            }),
        };
        let signature = self.dep_signature_for(key.scope.canonical_id.clone(), observed_hash);
        let node_id = self.graph().intern_node_with_scope(
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id: key.scope.canonical_id.clone(),
                owner: key.scope.owner,
                name: key.name.clone(),
                whole_hash: observed_hash,
            }),
            scope,
        );
        QueryBuildOutput::from((QueryResult::Value(node_id), signature))
            .with_observed_self_roots(build.roots)
    }
}
