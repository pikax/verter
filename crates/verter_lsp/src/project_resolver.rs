use verter_host::FileKind;

/// Resolution kind for an import or external source request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveRequestKind {
    EsmImport,
    TypeImport,
    RequireCall,
    SfcSrcAttr,
}

/// Which dependency graph is asking for resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvePhase {
    CodegenBlocker,
    ProviderGraph,
}

/// Where the resolved file should be exposed to the provider layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTarget {
    SourceFile,
    VuePublicApi,
    ShadowSourceFile,
}

/// High-level category describing how the specifier resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionKind {
    Relative,
    TsConfigPath,
    NodeModules,
    PackageExports,
    PackageImports,
    WorkspaceAlias,
    Bundler,
    PlaygroundMap,
}

/// Input for native project resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveRequest {
    pub importer_id: String,
    pub specifier: String,
    pub kind: ResolveRequestKind,
    pub phase: ResolvePhase,
}

/// Output from native project resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveResult {
    pub resolved_id: String,
    pub file_kind: FileKind,
    pub provider_target: ProviderTarget,
    pub resolution_kind: ResolutionKind,
}

/// Placeholder seam for the native resolver.
///
/// TODO(native-resolver): replace the legacy path alias resolver and wire this
/// into IDE project sync, tsserver/tsgo feeding, and compile-blocker hydration.
pub trait ProjectResolver: Send + Sync {
    fn resolve(&self, request: &ResolveRequest) -> Option<ResolveResult>;
}

/// Default no-op implementation until the native resolver lands.
#[derive(Debug, Default)]
pub struct UnconfiguredProjectResolver;

impl ProjectResolver for UnconfiguredProjectResolver {
    fn resolve(&self, _request: &ResolveRequest) -> Option<ResolveResult> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_project_resolver_returns_none() {
        let resolver = UnconfiguredProjectResolver;
        let request = ResolveRequest {
            importer_id: "/src/App.vue".to_string(),
            specifier: "./child".to_string(),
            kind: ResolveRequestKind::TypeImport,
            phase: ResolvePhase::CodegenBlocker,
        };

        assert_eq!(resolver.resolve(&request), None);
    }
}
