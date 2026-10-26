//! The differential lane the `differential_*` tests compare against the
//! checker: one host carrying the four `strictNullChecks` × `noImplicitAny`
//! projects, a probe module upserted verbatim into each, and every row read
//! back through the public audited flow-return boundary for diagnostics and
//! the producer's selected return-value fact, reduced and spelled as the
//! checker prints it. Runtime capture completion remains a separate fact.
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
use std::time::{Duration, Instant};

use super::ProjectSemanticDispatch;
use crate::types::HostConfig;
use crate::VerterHost;
use verter_type_engine::semantic_query::{
    LiteralValue, ProjectionMode, ProjectionReductionContext, ReturnProjectionDemand,
    SemanticNodeData, SemanticNodeId, SignatureKind,
};

/// One project of the matrix: its root and its `compilerOptions`.
#[derive(Clone, Copy)]
pub(super) struct Setting {
    pub(super) root: &'static str,
    pub(super) label: &'static str,
    pub(super) options: &'static str,
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

/// `strict` with `strictFunctionTypes` off.
pub(super) const STRICT_FUNCTION_TYPES_OFF: Setting = Setting {
    root: "/strict-loose-function-types",
    label: "strictFunctionTypes off",
    options: r#"{ "strict": true, "strictFunctionTypes": false }"#,
};

/// The four settings, in the order a four-answer row lists them.
pub(super) const ALL: [Setting; 4] = [STRICT, LOOSE, STRICT_IMPLICIT, LOOSE_IMPLICIT];

/// The four settings with `strictBindCallApply` off, in the same order.
pub(super) const BIND_CALL_APPLY_OFF: [Setting; 4] = [
    Setting {
        root: "/strict-loose-bind",
        label: "strict, strictBindCallApply off",
        options: r#"{ "strict": true, "strictBindCallApply": false }"#,
    },
    Setting {
        root: "/loose-loose-bind",
        label: "strictNullChecks off, strictBindCallApply off",
        options: r#"{ "strict": true, "strictNullChecks": false, "strictBindCallApply": false }"#,
    },
    Setting {
        root: "/strict-implicit-loose-bind",
        label: "noImplicitAny off, strictBindCallApply off",
        options: r#"{ "strict": true, "noImplicitAny": false, "strictBindCallApply": false }"#,
    },
    Setting {
        root: "/loose-implicit-loose-bind",
        label: "both off, strictBindCallApply off",
        options: r#"{ "strict": true, "strictNullChecks": false, "noImplicitAny": false, "strictBindCallApply": false }"#,
    },
];

/// The CPU time one row's evaluating thread may spend before the row is
/// reported overdue. A hang detector, not a speed budget: it catches a row
/// that loops or whose work blows up super-linearly, never a slow but
/// healthy machine. It is the thread's own CPU time, not the time on the
/// clock, so a row slowed by the tests running beside it in the same
/// process is not reported. On a platform with no reader of a thread's CPU
/// time it is the time on the clock.
const ROW_CPU_BUDGET: Duration = Duration::from_secs(60);

/// The time on the clock after which a row is reported overdue whatever
/// CPU time it spent: the backstop for a row that makes no progress (a
/// thread blocked on a lock or a channel spends no CPU time, so the budget
/// above never trips for it). It sits below the test runner's own
/// five-minute terminator, so the row reports its own diagnostic.
const ROW_NO_PROGRESS_BACKSTOP: Duration = Duration::from_secs(240);

/// How often the row's CPU time is read while it runs.
const ROW_POLL: Duration = Duration::from_millis(50);

/// What a row reads.
#[derive(Clone, Copy)]
pub(super) enum Read<'a> {
    /// A type in TYPE position.
    Type(&'a str),
    /// The body-derived return of a function of the module.
    Return(&'a str),
}

impl Read<'_> {
    pub(super) fn text(&self) -> &str {
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
    /// A file whose source differs per project, one source per setting in
    /// setting order.
    file_per_setting: Option<(&'a str, &'a [&'a str])>,
    /// The projects live on disk beside the workspace's installed
    /// `node_modules` — or the dependency directory the named chain of
    /// installed packages reaches — so the probe imports real packages.
    installed_packages: Option<&'a [&'a str]>,
    installed_typescript_libs: Option<&'a [&'a str]>,
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
            file_per_setting: None,
            installed_packages: None,
            installed_typescript_libs: None,
        }
    }

    /// With every project on disk under one workspace root holding the
    /// workspace's installed `node_modules`, so the probe module's imports
    /// resolve to the real packages through their own re-export chains and
    /// augmentations.
    pub(super) fn installed_packages(mut self) -> Self {
        self.installed_packages = Some(&[]);
        self
    }

    /// Register the selected installed SDK libraries and their reference closure.
    pub(super) fn installed_typescript_libs(mut self, libs: &'a [&'a str]) -> Self {
        assert!(self.installed_packages.is_some());
        self.installed_typescript_libs = Some(libs);
        self
    }

    /// [`Self::installed_packages`] over the dependencies the installed
    /// package chain `packages` reaches: `["vue", "@vue/runtime-dom"]` is
    /// the directory `@vue/runtime-dom`, as `vue` depends on it, resolves its
    /// own dependencies from, so the probe imports the packages a library
    /// re-exports and augments without that library's own re-exports.
    pub(super) fn installed_packages_of(mut self, packages: &'a [&'a str]) -> Self {
        self.installed_packages = Some(packages);
        self
    }

    /// With a file `name` beside the probe module whose source is
    /// `sources[i]` in the i-th setting's project.
    pub(super) fn file_per_setting(mut self, name: &'a str, sources: &'a [&'a str]) -> Self {
        self.file_per_setting = Some((name, sources));
        self
    }

    /// With `(file name, source)` modules beside the probe module in
    /// each project.
    pub(super) fn files(mut self, files: &'a [(&'a str, &'a str)]) -> Self {
        self.files = files;
        self
    }

    /// Checked in `settings` only, in place of all four.
    pub(super) fn settings(mut self, settings: &'a [Setting]) -> Self {
        self.settings = settings;
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

    /// One host carrying every setting's project with the probe module for
    /// `rows` (and the library and sibling files) upserted into it.
    pub(super) fn host(&self, rows: &[(Read<'_>, Vec<&str>)]) -> Arc<VerterHost> {
        assert!(
            self.installed_packages.is_none(),
            "an installed-packages matrix is read through `verdicts`"
        );
        let host = Arc::new(matrix_host(self.settings));
        let roots: Vec<String> = self
            .settings
            .iter()
            .map(|setting| setting.root.to_owned())
            .collect();
        self.upsert_projects(&host, rows, &roots);
        host
    }

    /// The probe module and its siblings upserted into each setting's
    /// project, the i-th setting's rooted at `roots[i]`.
    fn upsert_projects(&self, host: &VerterHost, rows: &[(Read<'_>, Vec<&str>)], roots: &[String]) {
        let module = self.module(rows);
        for (index, root) in roots.iter().enumerate() {
            let canonical = format!("{root}/probe.ts");
            if let Some((name, sources)) = self.file_per_setting {
                let path = format!("{root}/{name}");
                crate::u6_flow_shape_corpus_tests::upsert(
                    host,
                    &path,
                    sources[index],
                    crate::FileLanguage::script_ts(),
                );
            }
            if let Some(lib) = self.lib {
                crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::register_lib_environment(
                    host,
                    &canonical,
                    "lib.probe.d.ts",
                    lib,
                );
            }
            for (name, source) in self.files {
                let path = format!("{root}/{name}");
                // A sibling is classified by its name, as the host classifies
                // it: a `.d.ts` sibling is a declaration file.
                let language = crate::LanguageRegistry::global()
                    .classify_static(&path)
                    .static_resolution();
                crate::u6_flow_shape_corpus_tests::upsert(host, &path, source, language);
            }
            crate::u6_flow_shape_corpus_tests::upsert(
                host,
                &canonical,
                &module,
                crate::FileLanguage::script_ts(),
            );
        }
    }

    /// One host over every setting's project on disk, beside the
    /// workspace's installed `node_modules`, with the probe module for
    /// `rows` upserted into each; and each setting's project root.
    fn installed_packages_host(
        &self,
        rows: &[(Read<'_>, Vec<&str>)],
    ) -> (Arc<VerterHost>, Vec<String>, InstalledPackagesWorkspace) {
        let workspace_dir = InstalledPackagesWorkspace::new(self.installed_packages.unwrap_or(&[]));
        let workspace_root = workspace_dir.root();
        let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        ));
        let module = self.module(rows);
        let mut roots = Vec::with_capacity(self.settings.len());
        let mut configs = Vec::with_capacity(self.settings.len());
        for setting in self.settings {
            let dir = workspace_dir.project_dir(setting);
            let options = setting
                .options
                .trim_end()
                .strip_suffix('}')
                .expect("a setting's options are a JSON object");
            let libs = self
                .installed_typescript_libs
                .map_or_else(String::new, |libs| {
                    format!(
                        ", \"lib\": {}",
                        serde_json::to_string(libs).expect("SDK lib names")
                    )
                });
            std::fs::write(
                dir.join("tsconfig.json"),
                format!(
                    r#"{{ "compilerOptions": {options}, "module": "esnext", "moduleResolution": "bundler", "target": "es2020", "skipLibCheck": true{libs} }} }}"#
                ),
            )
            .expect("write the project's tsconfig");
            // On disk too, so the project's `include` lists it.
            std::fs::write(dir.join("probe.ts"), &module).expect("write the probe module");
            let root = slash_path(&dir);
            configs.push(verter_workspace::configured_ide_project(
                &*workspace,
                root.clone(),
                workspace_root.clone(),
                &format!("{root}/tsconfig.json"),
            ));
            roots.push(root);
        }
        let host = Arc::new(VerterHost::new(
            HostConfig {
                analysis_level: crate::types::AnalysisLevel::Full,
                audit_enabled: true,
                footprint_capture: false,
                ..HostConfig::default()
            },
            workspace,
        ));
        host.configure_projects(configs);
        if self.installed_typescript_libs.is_some() {
            use verter_workspace::IntrinsicLibraryAccess;
            let sdk = verter_workspace::NativeIntrinsicLibrary::discover(std::path::Path::new(
                &workspace_root,
            ));
            assert!(
                !sdk.list_intrinsic_libs().is_empty(),
                "an SDK matrix needs the installed TypeScript libraries (run pnpm install)"
            );
            for root in &roots {
                let project = host
                    .resolve_project_for_canonical(&format!("{root}/probe.ts"))
                    .expect("the probe belongs to its SDK project");
                let workspace = host.workspace();
                let key = workspace
                    .project_stable_key(project)
                    .expect("the SDK project has a stable key");
                let options = workspace
                    .semantic_compiler_options_for_project(project)
                    .expect("the SDK project has compiler options");
                let mut pending: Vec<String> = options
                    .effective_lib_file_names()
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                let mut seen = rustc_hash::FxHashSet::default();
                while let Some(name) = pending.pop() {
                    if !seen.insert(name.clone()) {
                        continue;
                    }
                    let source = sdk
                        .read_intrinsic_lib(&name)
                        .expect("read a selected SDK library");
                    // Registration supplies bytes and global names. The shared
                    // program loader indexes only the configured lib closure;
                    // eagerly upserting every SDK file also analyzes unselected
                    // libraries and repeats that work for each project.
                    workspace
                        .register_ambient_lib(verter_workspace::AmbientLibSpec {
                            project_id: Some(project),
                            canonical_id: Arc::from(name.as_str()),
                            source: Arc::from(source),
                        })
                        .expect("the SDK library registers against its project");
                    let canonical = verter_workspace::ambient_virtual_canonical_id(key, &name);
                    let serve = host
                        .ensure_indexed_ready_serve_unobserved(&canonical)
                        .expect("the selected SDK library indexes through the shared loader");
                    pending.extend(
                        serve
                            .indexed
                            .route_inventory
                            .program_references
                            .libs
                            .iter()
                            .map(|lib| {
                                verter_session_query::inputs::program_order::library_file_name(lib)
                                    .expect("the SDK reference names a known library")
                                    .to_owned()
                            }),
                    );
                }
                for symbol in ["Object", "StringConstructor", "Symbol"] {
                    assert!(
                        workspace.lookup_ambient_symbol(key, symbol).is_some(),
                        "the installed SDK provides {symbol} to {root}"
                    );
                    let space = if symbol == "Symbol" {
                        verter_type_engine::resolver_core::request_ports::AmbientSymbolSpace::Value
                    } else {
                        verter_type_engine::resolver_core::request_ports::AmbientSymbolSpace::Type
                    };
                    assert!(
                        host.program_ambient_symbol(key, symbol, space).is_some(),
                        "the program loads SDK symbol {symbol} for {root}"
                    );
                }
            }
        }
        self.upsert_projects(&host, rows, &roots);
        (host, roots, workspace_dir)
    }

    /// Every `(setting, function)` whose body-derived return closes WITHOUT
    /// its proof — the finalizer's verdict partial — with its partial
    /// reasons, as the failure list.
    pub(super) fn unproven_returns(&self, functions: &[&str]) -> Vec<String> {
        let host = self.host(&[]);
        let mut unproven = Vec::new();
        for setting in self.settings {
            let canonical = format!("{}/probe.ts", setting.root);
            for function in functions {
                let store_view = host.resolver_store_view_read().into_owned_view();
                let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
                let host_ctx = crate::resolver_core::HostResolverContext::<
                    '_,
                    verter_audit::observe::Uncaptured,
                >::new(&host, &store_view, overlay);
                let dispatch = ProjectSemanticDispatch::new(&host_ctx);
                let key = verter_type_engine::semantic_query::FlowReturnKey {
                    function: dispatch.flow_function_slot_for(
                        Arc::from(canonical.as_str()),
                        verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        Arc::from(*function),
                        verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
                        0,
                    ),
                    normalized_type_args: verter_type_expr::OwnedSlice::from(
                        Vec::new().into_boxed_slice(),
                    ),
                    context: dispatch.flow_return_context_for(&canonical),
                    demand: ReturnProjectionDemand::whole_return(),
                    input: verter_type_engine::semantic_query::FlowInputContext::empty(),
                    result_contract: super::flow_solve::flow_return_result_contract_id(),
                };
                let read = dispatch.execute_read(
                    verter_type_engine::semantic_query::SemanticQueryKey::FlowReturn(Box::new(key)),
                );
                if read.result_is_partial() {
                    unproven.push(format!(
                        "`{function}` [{}]: its proof does not close ({:?})",
                        setting.label,
                        read.partial_reasons()
                    ));
                }
            }
        }
        unproven
    }

    pub(super) fn return_relation_work(&self, function: &str) -> (u64, usize, usize) {
        let (relations, [arguments, bindings]) = self.return_work(
            function,
            [
                verter_type_engine::project_semantic_dispatch::call_resolve::argument_work_for_tests,
                verter_type_engine::project_semantic_dispatch::call_resolve::binding_work_for_tests,
            ],
        );
        (relations, arguments, bindings)
    }

    /// The relation checks of answering `function`'s return in the first
    /// setting, and the growth of each of `counters` over that answer.
    pub(super) fn return_work<const N: usize>(
        &self,
        function: &str,
        counters: [fn() -> usize; N],
    ) -> (u64, [usize; N]) {
        let host = self.host(&[]);
        let canonical = format!("{}/probe.ts", self.settings[0].root);
        let store_view = host.resolver_store_view_read().into_owned_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::<
            '_,
            verter_audit::observe::Uncaptured,
        >::new(&host, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);
        let key = verter_type_engine::semantic_query::FlowReturnKey {
            function: dispatch.flow_function_slot_for(
                Arc::from(canonical.as_str()),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                Arc::from(function),
                verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
                0,
            ),
            normalized_type_args: verter_type_expr::OwnedSlice::from([]),
            context: dispatch.flow_return_context_for(&canonical),
            demand: ReturnProjectionDemand::whole_return(),
            input: verter_type_engine::semantic_query::FlowInputContext::empty(),
            result_contract: super::flow_solve::flow_return_result_contract_id(),
        };
        let before = dispatch.graph().stats_snapshot().relation_check_count;
        let counted_before = counters.map(|counter| counter());
        let result = dispatch.execute_read(
            verter_type_engine::semantic_query::SemanticQueryKey::FlowReturn(Box::new(key)),
        );
        assert!(
            !result.result_is_partial(),
            "{function}: {:?}",
            result.partial_reasons()
        );
        let mut counted = counted_before;
        for (counted, counter) in counted.iter_mut().zip(counters) {
            *counted = counter() - *counted;
        }
        (
            dispatch.graph().stats_snapshot().relation_check_count - before,
            counted,
        )
    }

    /// The work the first setting's cold read of `function`'s return spends,
    /// by the deterministic counters a growth test compares: relation
    /// checks, cold builds, connected work, the operations charged against
    /// the request's fuse, and call resolution's argument and binding work.
    pub(super) fn return_work_counters(&self, function: &str) -> Vec<(&'static str, u64)> {
        let host = self.host(&[]);
        let canonical = format!("{}/probe.ts", self.settings[0].root);
        let store_view = host.resolver_store_view_read().into_owned_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::<
            '_,
            verter_audit::observe::Uncaptured,
        >::new(&host, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);
        let key = verter_type_engine::semantic_query::FlowReturnKey {
            function: dispatch.flow_function_slot_for(
                Arc::from(canonical.as_str()),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                Arc::from(function),
                verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
                0,
            ),
            normalized_type_args: verter_type_expr::OwnedSlice::from([]),
            context: dispatch.flow_return_context_for(&canonical),
            demand: ReturnProjectionDemand::whole_return(),
            input: verter_type_engine::semantic_query::FlowInputContext::empty(),
            result_contract: super::flow_solve::flow_return_result_contract_id(),
        };
        let stats = dispatch.graph().stats_snapshot();
        let argument_before =
            verter_type_engine::project_semantic_dispatch::call_resolve::argument_work_for_tests();
        let binding_before =
            verter_type_engine::project_semantic_dispatch::call_resolve::binding_work_for_tests();
        let connected_before = dispatch.connected_demand().work_used_for_tests();
        // The read runs in a request of its own, under the production
        // operation fuse, so the operations it charges are counted as the
        // public route counts them.
        let _request = verter_type_engine::request_context::RequestContextGuard::install(
            verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
                1,
                Arc::from(canonical.as_str()),
                verter_audit::RequestKind::TypeResolution,
                false,
                false,
                None,
                verter_type_engine::request_budget::DEFAULT_PROJECTION_OP_BUDGET,
            ),
        );
        let (result, charged, _) =
            verter_type_engine::request_budget::with_request_spend_capture_for_tests(|| {
                dispatch.execute_read(
                    verter_type_engine::semantic_query::SemanticQueryKey::FlowReturn(Box::new(key)),
                )
            });
        assert!(
            !result.result_is_partial(),
            "{function}: {:?}",
            result.partial_reasons()
        );
        assert!(
            charged > 0,
            "{function}: the read charges the request it runs in, so the count is live"
        );
        let after = dispatch.graph().stats_snapshot();
        vec![
            ("relation checks", after.relation_check_count - stats.relation_check_count),
            ("cold builds", after.misses - stats.misses),
            (
                "connected work",
                (dispatch.connected_demand().work_used_for_tests() - connected_before) as u64,
            ),
            ("charged operations", charged as u64),
            (
                "argument work",
                (verter_type_engine::project_semantic_dispatch::call_resolve::argument_work_for_tests()
                    - argument_before) as u64,
            ),
            (
                "binding work",
                (verter_type_engine::project_semantic_dispatch::call_resolve::binding_work_for_tests()
                    - binding_before) as u64,
            ),
        ]
    }

    /// Each row's verdict in each setting, in row then setting order.
    pub(super) fn verdicts(&self, rows: &[(Read<'_>, Vec<&str>)]) -> Vec<Vec<Verdict>> {
        if self.installed_packages.is_some() {
            let (host, roots, _workspace) = self.installed_packages_host(rows);
            return self.verdicts_at(&host, &roots, rows);
        }
        self.verdicts_on(&self.host(rows), rows)
    }

    /// [`Self::verdicts`] read from `host`, a host [`Self::host`] built for
    /// the same rows.
    pub(super) fn verdicts_on(
        &self,
        host: &Arc<VerterHost>,
        rows: &[(Read<'_>, Vec<&str>)],
    ) -> Vec<Vec<Verdict>> {
        let roots: Vec<String> = self
            .settings
            .iter()
            .map(|setting| setting.root.to_owned())
            .collect();
        self.verdicts_at(host, &roots, rows)
    }

    /// Each row's verdict read from `host` at each setting's project
    /// `roots`, in row then setting order.
    fn verdicts_at(
        &self,
        host: &Arc<VerterHost>,
        roots: &[String],
        rows: &[(Read<'_>, Vec<&str>)],
    ) -> Vec<Vec<Verdict>> {
        rows.iter()
            .enumerate()
            .map(|(index, (read, answers))| {
                let symbol = match read {
                    Read::Type(_) => format!("__probe_{index}"),
                    Read::Return(function) => (*function).to_owned(),
                };
                roots
                    .iter()
                    .zip(answers)
                    .map(|(root, expected)| {
                        let canonical = format!("{root}/probe.ts");
                        let (observed, diagnostics) =
                            observe_with_deadline(host, &canonical, &symbol, self.lib.is_some());
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
                            diagnostics,
                        }
                    })
                    .collect()
            })
            .collect()
    }

    pub(super) fn module(&self, rows: &[(Read<'_>, Vec<&str>)]) -> String {
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

/// A temporary workspace root holding a link to the workspace's installed
/// `node_modules` (or the directory `VERTER_TEST_NODE_MODULES` names, for a
/// worktree without its own install) and one project directory per
/// setting. The link is removed before the directory, so removal never
/// walks into the installed packages.
struct InstalledPackagesWorkspace {
    dir: tempfile::TempDir,
    link: std::path::PathBuf,
}

impl InstalledPackagesWorkspace {
    fn new(packages: &[&str]) -> Self {
        let node_modules = std::env::var_os("VERTER_TEST_NODE_MODULES").map_or_else(
            || {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .and_then(std::path::Path::parent)
                    .expect("crate is <workspace>/crates/verter_session")
                    .join("node_modules")
            },
            std::path::PathBuf::from,
        );
        assert!(
            node_modules.is_dir(),
            "an installed-packages matrix needs the workspace's installed packages under {} \
             (run pnpm install)",
            node_modules.display()
        );
        // Each package of the chain resolves its own dependencies from the
        // `node_modules` directory its installed location sits in.
        let node_modules = packages.iter().fold(node_modules, |node_modules, package| {
            let installed = node_modules
                .join(package)
                .canonicalize()
                .unwrap_or_else(|_| panic!("the installed package {package}"));
            installed
                .ancestors()
                .nth(package.split('/').count())
                .expect("an installed package sits in a node_modules directory")
                .to_path_buf()
        });
        let dir = tempfile::tempdir().expect("a temporary workspace");
        let link = dir.path().join("node_modules");
        link_dir(&node_modules, &link);
        Self { dir, link }
    }

    /// The workspace root, canonical and slash-separated.
    fn root(&self) -> String {
        slash_path(
            &self
                .dir
                .path()
                .canonicalize()
                .expect("canonical workspace root"),
        )
    }

    /// `setting`'s project directory, created.
    fn project_dir(&self, setting: &Setting) -> std::path::PathBuf {
        let dir = self
            .dir
            .path()
            .canonicalize()
            .expect("canonical workspace root")
            .join(setting.root.trim_start_matches('/'));
        std::fs::create_dir_all(&dir).expect("create the project directory");
        dir
    }
}

impl Drop for InstalledPackagesWorkspace {
    fn drop(&mut self) {
        #[cfg(windows)]
        let _ = std::fs::remove_dir(&self.link);
        #[cfg(not(windows))]
        let _ = std::fs::remove_file(&self.link);
    }
}

#[cfg(windows)]
fn link_dir(target: &std::path::Path, link: &std::path::Path) {
    let windows_path = |path: &std::path::Path| {
        let text = path.to_string_lossy().replace('/', "\\");
        text.strip_prefix(r"\\?\")
            .map(str::to_string)
            .unwrap_or(text)
    };
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(windows_path(link))
        .arg(windows_path(target))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("mklink runs");
    assert!(
        status.success(),
        "junction {} -> {}",
        link.display(),
        target.display()
    );
}

#[cfg(not(windows))]
fn link_dir(target: &std::path::Path, link: &std::path::Path) {
    std::os::unix::fs::symlink(target, link).expect("link the installed packages");
}

/// `path` slash-separated, without a verbatim prefix.
fn slash_path(path: &std::path::Path) -> String {
    verter_span::path::canonicalize_path(&path.to_string_lossy())
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
    /// The checker diagnostics the lane recorded for the row, by code.
    pub(super) diagnostics: Vec<u32>,
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
    /// The evaluation spent more than [`ROW_CPU_BUDGET`] of CPU time, or
    /// made no progress for [`ROW_NO_PROGRESS_BACKSTOP`].
    Overdue(String),
}

impl Observed {
    fn class(&self) -> &'static str {
        match self {
            Observed::Clean(text) if !text.contains('<') || is_generic_print(text) => "WRONG-CLEAN",
            Observed::Panicked(_) => "PANIC",
            Observed::Overdue(_) => "HANG",
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
            Observed::Overdue(reason) => format!("the lane {reason}"),
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
) -> (Observed, Vec<u32>) {
    let (sender, receiver) = mpsc::channel();
    let thread_host = Arc::clone(host);
    let canonical = canonical.to_owned();
    let symbol = symbol.to_owned();
    // Match the host CPU workers' stack: debug signature relations must
    // reach their logical depth guard before the platform's stack guard.
    let thread = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
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
                (Observed::Panicked(message), Vec::new())
            });
            let _ = sender.send(observed);
        })
        .expect("start semantic observation worker");
    match await_row(&thread, &receiver, ROW_CPU_BUDGET, ROW_NO_PROGRESS_BACKSTOP) {
        Ok(observed) => observed,
        Err(RowOverdue::Disconnected) => (
            Observed::Panicked("the evaluating thread ended with no answer".to_owned()),
            Vec::new(),
        ),
        Err(overdue) => (Observed::Overdue(overdue.to_string()), Vec::new()),
    }
}

/// Why a row has no answer.
#[derive(Debug, PartialEq, Eq)]
enum RowOverdue {
    /// Its thread spent more than the CPU budget.
    Cpu(Duration),
    /// It made no progress for the backstop, spending this much CPU time.
    NoProgress(Duration, Duration),
    /// Its thread ended without sending one.
    Disconnected,
}

impl std::fmt::Display for RowOverdue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RowOverdue::Cpu(budget) => write!(
                f,
                "spent more than {budget:?} of CPU time ({})",
                thread_cpu::SOURCE.unwrap_or("time on the clock")
            ),
            RowOverdue::NoProgress(backstop, spent) => write!(
                f,
                "made no progress for {backstop:?} (spent {spent:?} of CPU time)"
            ),
            RowOverdue::Disconnected => write!(f, "ended with no answer"),
        }
    }
}

/// The answer `thread` sends on `receiver`, unless the thread spends more
/// than `cpu_budget` of CPU time first or `backstop` passes on the clock.
fn await_row<T, R>(
    thread: &std::thread::JoinHandle<T>,
    receiver: &mpsc::Receiver<R>,
    cpu_budget: Duration,
    backstop: Duration,
) -> Result<R, RowOverdue> {
    let started = Instant::now();
    // bounded-loop: ends at the answer, the CPU budget or the backstop.
    loop {
        match receiver.recv_timeout(ROW_POLL) {
            Ok(answer) => return Ok(answer),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(RowOverdue::Disconnected),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let spent = thread_cpu::spent(thread).unwrap_or_else(|| started.elapsed());
        if spent > cpu_budget {
            return Err(RowOverdue::Cpu(cpu_budget));
        }
        if started.elapsed() > backstop {
            return Err(RowOverdue::NoProgress(backstop, spent));
        }
    }
}

/// The CPU time a running thread has spent, read from another thread.
#[cfg(target_os = "linux")]
mod thread_cpu {
    use std::os::raw::{c_int, c_long};
    use std::os::unix::thread::{JoinHandleExt, RawPthread};
    use std::time::Duration;

    /// `struct timespec` on Linux: `time_t` and the nanoseconds are `long`.
    #[repr(C)]
    struct Timespec {
        tv_sec: c_long,
        tv_nsec: c_long,
    }

    extern "C" {
        fn pthread_getcpuclockid(thread: RawPthread, clock: *mut c_int) -> c_int;
        fn clock_gettime(clock: c_int, time: *mut Timespec) -> c_int;
    }

    pub(super) const SOURCE: Option<&str> = Some("pthread_getcpuclockid");

    pub(super) fn spent<T>(thread: &std::thread::JoinHandle<T>) -> Option<Duration> {
        let mut clock: c_int = 0;
        let mut time = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: the handle is not joined or dropped while it is borrowed,
        // so its pthread id stays valid; both calls write only the values
        // they are given.
        let read = unsafe {
            pthread_getcpuclockid(thread.as_pthread_t(), &mut clock) == 0
                && clock_gettime(clock, &mut time) == 0
        };
        read.then(|| Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
    }
}

/// The CPU time a running thread has spent, read from another thread.
#[cfg(windows)]
mod thread_cpu {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::time::Duration;

    /// `FILETIME`: a count of 100-nanosecond intervals, split in halves.
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    impl FileTime {
        fn nanos(&self) -> u64 {
            ((u64::from(self.high) << 32) | u64::from(self.low)) * 100
        }
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetThreadTimes(
            thread: *mut c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }

    pub(super) const SOURCE: Option<&str> = Some("GetThreadTimes");

    pub(super) fn spent<T>(thread: &std::thread::JoinHandle<T>) -> Option<Duration> {
        let (mut creation, mut exit) = (FileTime::default(), FileTime::default());
        let (mut kernel, mut user) = (FileTime::default(), FileTime::default());
        // SAFETY: the handle stays open while the join handle is borrowed,
        // and the call writes the four `FILETIME`s it is given.
        let ok = unsafe {
            GetThreadTimes(
                thread.as_raw_handle(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        (ok != 0).then(|| Duration::from_nanos(kernel.nanos() + user.nanos()))
    }
}

/// No reader of a thread's CPU time here: the budget is the time on the
/// clock.
#[cfg(not(any(target_os = "linux", windows)))]
mod thread_cpu {
    use std::time::Duration;

    pub(super) const SOURCE: Option<&str> = None;

    pub(super) fn spent<T>(_thread: &std::thread::JoinHandle<T>) -> Option<Duration> {
        None
    }
}

fn observe(host: &VerterHost, canonical: &str, symbol: &str, scoped: bool) -> (Observed, Vec<u32>) {
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: verter_type_expr::OwnedStr::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: verter_type_expr::OwnedStr::from(symbol),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let carrier =
        host.get_flow_return_type_with_audit(&identity, ReturnProjectionDemand::whole_return());
    let result = match carrier.as_result() {
        Ok(result) => Arc::clone(result),
        Err(error) => return (Observed::NoValue(format!("{error:?}")), Vec::new()),
    };
    let diagnostics: Vec<u32> = result
        .checker_diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().code())
        .collect();
    // The probe's type is read as a consumer reads it: under a request for
    // the probe file, so every judgement takes that project's options.
    let _request = verter_type_engine::request_context::install_test_request_for(canonical);
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx =
        crate::resolver_core::HostResolverContext::<'_, verter_audit::observe::Uncaptured>::new(
            host,
            &store_view,
            overlay,
        );
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let _builder = dispatch.enter_construction_scope();
    let _demand_scope = scoped.then(|| {
        super::LexicalDemandScopeGuard::push(&dispatch.lexical_demand_scope, Arc::from(canonical))
    });
    let value_fact =
        match dispatch.execute_flow_return_value_fact(dispatch.flow_return_key_for(&identity)) {
            Ok(fact) => fact,
            Err(abort) => return (Observed::NoValue(format!("{abort:?}")), diagnostics),
        };
    let value = match &value_fact {
        verter_type_engine::semantic_query::FactResult::Complete(value)
        | verter_type_engine::semantic_query::FactResult::Approximate { value, .. } => *value,
        verter_type_engine::semantic_query::FactResult::Unavailable { .. } => {
            return (Observed::Partial, diagnostics);
        }
    };
    let Some(node) = dispatch
        .normalize_node_keeping_declaration_refs_for_tests(
            value,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .into_complete_node()
    else {
        return (Observed::Partial, diagnostics);
    };
    let text = print(&dispatch, node, 0);
    let observed = match result.degradation() {
        None if value_fact.status().is_complete() => Observed::Clean(text),
        None => Observed::Degraded(text, format!("{:?}", value_fact.status())),
        Some(degradation) => Observed::Degraded(text, format!("{degradation:?}")),
    };
    (observed, diagnostics)
}

/// `node` as the checker prints it.
fn print(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    node: SemanticNodeId,
    depth: usize,
) -> String {
    if depth > 64 {
        return "<unrendered deep>".to_owned();
    }
    let graph = dispatch.graph_arc();
    let Some(data) = graph.node_data(node) else {
        return "<evicted>".to_owned();
    };
    // A construction program with one closed alternative is the object it
    // builds, as every consumer of the carrier reduces it.
    if matches!(data.as_ref(), SemanticNodeData::ObjectSpreadProgram(_)) {
        if let Some((members, false)) = dispatch.spread_program_members_for_published_projection(
            node,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        ) {
            let object = graph.intern_node(SemanticNodeData::Object(
                verter_type_engine::semantic_query::SurfaceView::from_entries(
                    members
                        .into_iter()
                        .map(verter_type_engine::semantic_query::SurfaceEntry::Member)
                        .collect(),
                    None,
                    false,
                ),
            ));
            return print(dispatch, object, depth);
        }
    }
    let at = |node: SemanticNodeId| print(dispatch, node, depth + 1);
    match data.as_ref() {
        SemanticNodeData::Primitive(kind) => match kind {
            verter_type_engine::semantic_query::PrimitiveKind::BigInt => "bigint".to_owned(),
            other => format!("{other:?}").to_ascii_lowercase(),
        },
        SemanticNodeData::Literal(LiteralValue::String(value)) => checker_string_literal(value),
        SemanticNodeData::Literal(LiteralValue::Number(value)) => format!("{value}"),
        SemanticNodeData::Literal(LiteralValue::Boolean(value)) => format!("{value}"),
        SemanticNodeData::Literal(LiteralValue::BigInt(value)) => format!("{value}n"),
        SemanticNodeData::EnumLiteral(literal) => literal.printed_name(),
        SemanticNodeData::Alias(inner) => at(*inner),
        SemanticNodeData::Union(members) => {
            verter_type_engine::semantic_query::printed_union_arms(graph, members)
                .into_iter()
                .map(|arm| match arm {
                    verter_type_engine::semantic_query::PrintedUnionArm::Node(member) => {
                        operand(dispatch, member, depth, true)
                    }
                    verter_type_engine::semantic_query::PrintedUnionArm::Enum(decl) => {
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
        // An object type with one call signature and nothing else prints as
        // that signature's function type.
        SemanticNodeData::Object(surface)
            if surface.call_signatures().len() == 1
                && surface.construct_signatures().is_empty()
                && surface.index_signatures().is_empty()
                && surface.positive_members().is_empty() =>
        {
            signature_text(dispatch, surface.call_signatures()[0], depth, " => ")
        }
        SemanticNodeData::Object(surface) => {
            let mut members: Vec<String> = Vec::new();
            for signature in surface.call_signatures().iter() {
                members.push(format!(
                    "{};",
                    signature_text(dispatch, *signature, depth, ": ")
                ));
            }
            for signature in surface.construct_signatures().iter() {
                members.push(format!(
                    "{};",
                    signature_text(dispatch, *signature, depth, ": ")
                ));
            }
            for index in surface.index_signatures().iter() {
                members.push(format!(
                    "{}[x: {}]: {};",
                    if index.readonly { "readonly " } else { "" },
                    at(index.key_type),
                    at(index.value_type)
                ));
            }
            for member in surface.positive_members().iter() {
                let name = match member.key.cloned_known() {
                    Some(verter_type_expr::PropertyKey::String(name)) => name.to_string(),
                    Some(verter_type_expr::PropertyKey::Number(index)) => index.to_string(),
                    _ => "<key>".to_owned(),
                };
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
                verter_type_expr::push_template_quasi(&mut text, quasi);
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
            if args.is_empty() {
                return base.decl_name.to_string();
            }
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
        // A conditional the checker keeps (a generic operand) prints as
        // written, its branches as the pending substitution reads them.
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            pending,
            ..
        } => {
            let true_branch = dispatch.apply_conditional_branch_pending(
                *true_branch_ref,
                pending.as_deref(),
                true,
            );
            let false_branch = dispatch.apply_conditional_branch_pending(
                *false_branch_ref,
                pending.as_deref(),
                false,
            );
            format!(
                "{} extends {} ? {} : {}",
                operand(dispatch, *check, depth, false),
                operand(dispatch, *extends, depth, false),
                at(true_branch),
                at(false_branch)
            )
        }
        // An `infer` declaration prints with its constraint; a reference to
        // one is its name.
        SemanticNodeData::Infer {
            name, constraint, ..
        } => match constraint {
            Some(constraint) => format!("infer {name} extends {}", at(*constraint)),
            None => format!("infer {name}"),
        },
        SemanticNodeData::InferRef { name, .. } => name.to_string(),
        // An access of a type parameter is generic and prints as written; any
        // other access the lane kept is a gap.
        SemanticNodeData::IndexedAccess { object, index } => {
            let mut root = *object;
            while let Some(SemanticNodeData::IndexedAccess { object, .. }) =
                graph.node_data(root).as_deref()
            {
                root = *object;
            }
            if !matches!(
                graph.node_data(root).as_deref(),
                Some(SemanticNodeData::TypeParam { .. })
            ) {
                return "<unreduced indexed access>".to_owned();
            }
            let index = match index {
                verter_type_engine::semantic_query::IndexKey::String(name) => {
                    checker_string_literal(name)
                }
                verter_type_engine::semantic_query::IndexKey::Number(number) => number.to_string(),
                verter_type_engine::semantic_query::IndexKey::Computed(index) => at(*index),
                verter_type_engine::semantic_query::IndexKey::UniqueSymbol(_) => {
                    return "<unreduced indexed access>".to_owned();
                }
            };
            format!("{}[{index}]", operand(dispatch, *object, depth, false))
        }
        SemanticNodeData::Mapped { .. } => "<unreduced mapped>".to_owned(),
        other => {
            let text: String = format!("{other:?}").chars().take(60).collect();
            format!("<unrendered {text}>")
        }
    }
}

/// A string literal type as the checker prints it: double-quoted, with a
/// backslash, a double quote, every control character and U+2028 / U+2029 /
/// U+0085 escaped.
fn checker_string_literal(value: &str) -> String {
    let mut text = String::from("\"");
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' => text.push_str("\\\\"),
            '"' => text.push_str("\\\""),
            '\n' => text.push_str("\\n"),
            '\t' => text.push_str("\\t"),
            '\r' => text.push_str("\\r"),
            '\u{8}' => text.push_str("\\b"),
            '\u{c}' => text.push_str("\\f"),
            '\u{b}' => text.push_str("\\v"),
            '\0' if characters.peek().is_some_and(char::is_ascii_digit) => {
                text.push_str("\\x00");
            }
            '\0' => text.push_str("\\0"),
            control @ ('\u{1}'..='\u{1f}' | '\u{2028}' | '\u{2029}' | '\u{85}') => {
                text.push_str(&format!("\\u{:04X}", control as u32));
            }
            other => text.push(other),
        }
    }
    text.push('"');
    text
}

/// `node` printed as an operand of `|`, `&`, `[]` or `keyof`:
/// parenthesized as the checker parenthesizes it.
fn operand(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
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
                && verter_type_engine::semantic_query::printed_union_arms(
                    dispatch.graph().store(),
                    members,
                )
                .len()
                    > 1
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
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    node: SemanticNodeId,
    depth: usize,
    arrow: &str,
) -> String {
    let graph = dispatch.graph_arc();
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
    let mut rendered: Vec<String> = Vec::with_capacity(params.len());
    for (index, param) in params.iter().enumerate() {
        let name = param
            .name
            .as_deref()
            .map_or_else(|| format!("arg{index}"), str::to_owned);
        // A rest parameter of a tuple type prints as the parameters its
        // elements are (the checker's expanded parameters): an element's
        // label, else `<rest>_<index>`, names each.
        if param.rest {
            if let Some(SemanticNodeData::Tuple { elements, .. }) =
                graph.node_data(param.ty).as_deref()
            {
                for (position, element) in elements.iter().enumerate() {
                    let element_name = element
                        .label
                        .as_deref()
                        .map_or_else(|| format!("{name}_{position}"), str::to_owned);
                    rendered.push(format!(
                        "{}{element_name}{}: {}",
                        if element.rest { "..." } else { "" },
                        if element.optional { "?" } else { "" },
                        at(element.value)
                    ));
                }
                continue;
            }
        }
        rendered.push(format!(
            "{}{name}{}: {}",
            if param.rest { "..." } else { "" },
            if param.optional { "?" } else { "" },
            at(param.ty)
        ));
    }
    let result = match predicate {
        Some(predicate) => {
            let subject = match predicate.subject {
                verter_type_engine::semantic_query::PredicateSubject::This => "this".to_owned(),
                verter_type_engine::semantic_query::PredicateSubject::Parameter(_) => predicate
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
            // Without `exactOptionalPropertyTypes` (off in every setting)
            // `name?: T | undefined` and `name?: T` are one type; the
            // checker's declaration emit spells a declared `b?: string`
            // as authored and a synthesized optional (a spread's partial)
            // as `a?: number | undefined`, so both sides drop the
            // optional member's `undefined` arm.
            let arms: Vec<String> = split_top(&rest, " | ")
                .into_iter()
                .filter(|arm| arm.trim() != "undefined")
                .collect();
            let rest = if arms.is_empty() {
                rest
            } else {
                arms.join(" | ")
            };
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
        canonical_text("[a: string, b?: number | undefined]"),
        canonical_text("[a: string, b?: number]")
    );
    assert_eq!(
        canonical_text("{ a: 1; b?: 2 | undefined; }"),
        canonical_text("{ b?: 2; a: 1; }")
    );
    assert_ne!(
        canonical_text("{ b?: undefined; }"),
        canonical_text("{ b?: 2; }")
    );
    assert_ne!(
        canonical_text("{ b: 2 | undefined; }"),
        canonical_text("{ b: 2; }")
    );
    assert_ne!(
        canonical_text("[a: string, b?: number]"),
        canonical_text("[a: string, b: number]")
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

/// A row whose thread computes past the CPU budget is overdue by it, and a
/// row whose thread is blocked — spending no CPU time — is overdue by the
/// backstop on the clock; a row that answers within both is its answer.
#[test]
fn a_row_is_overdue_by_its_cpu_time_or_by_the_no_progress_backstop() {
    let run = |work: fn(), cpu_budget: Duration, backstop: Duration| {
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            work();
            let _ = sender.send(());
        });
        await_row(&thread, &receiver, cpu_budget, backstop)
    };
    let spin: fn() = || {
        let started = Instant::now();
        let mut n: u64 = 0;
        while started.elapsed() < Duration::from_secs(5) {
            n = std::hint::black_box(n.wrapping_add(1));
        }
    };
    let block: fn() = || std::thread::sleep(Duration::from_secs(5));
    let answer: fn() = || {};
    let cpu_budget = Duration::from_millis(200);
    assert_eq!(
        run(spin, cpu_budget, Duration::from_secs(4)),
        Err(RowOverdue::Cpu(cpu_budget))
    );
    let blocked = run(block, cpu_budget, Duration::from_millis(300));
    assert!(
        matches!(blocked, Err(RowOverdue::NoProgress(..))) || thread_cpu::SOURCE.is_none(),
        "{blocked:?}"
    );
    assert_eq!(run(answer, cpu_budget, Duration::from_secs(4)), Ok(()));
}
