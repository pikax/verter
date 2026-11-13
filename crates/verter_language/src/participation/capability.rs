//! Profile-qualified capability identity (`docs/arch/kernel/identities.md`).

use std::fmt;

use verter_identity::encoding::{CanonicalDigest, CanonicalEncoder};
use verter_identity::successor::{CarrierProfileId, FrameworkProfileId, ProjectProfileId};

/// The catalog profile a capability is qualified by. Editor and browser
/// product surfaces are not profiles; a stylesheet region's CSS answers are
/// the CSS carrier's cells, never the enclosing framework's.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProfileRef {
    Carrier(CarrierProfileId),
    Framework(FrameworkProfileId),
    Project(ProjectProfileId),
}

impl ProfileRef {
    /// Assigned wire/encoding discriminant: never declaration order.
    pub const fn kind_discriminant(&self) -> u32 {
        match self {
            Self::Carrier(_) => 1,
            Self::Framework(_) => 2,
            Self::Project(_) => 3,
        }
    }

    pub fn digest(&self) -> CanonicalDigest {
        match self {
            Self::Carrier(id) => id.digest(),
            Self::Framework(id) => id.digest(),
            Self::Project(id) => id.digest(),
        }
    }

    /// Tagged, length-delimited bytes: kind discriminant then digest.
    pub(crate) fn encoded(&self) -> [u8; 36] {
        let mut bytes = [0u8; 36];
        bytes[..4].copy_from_slice(&self.kind_discriminant().to_le_bytes());
        bytes[4..].copy_from_slice(self.digest().as_bytes());
        bytes
    }
}

macro_rules! capability_operations {
    ($( $(#[$doc:meta])* $variant:ident => $key:literal, optional = $optional:literal; )*) => {
        /// One separately withdrawable operation facet of the capability inventory.
        ///
        /// Several facets can share one transport method (diagnostic producers,
        /// code-action kinds); an adapter maps its methods onto facets. The
        /// facet alone is not a capability: [`CapabilityId`] qualifies it by its
        /// profile. The stable [`Self::key`] is the only spelling that crosses a
        /// wire or enters an identity.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum CapabilityOperation {
            $( $(#[$doc])* $variant, )*
        }

        impl CapabilityOperation {
            /// Every facet, in declaration order.
            pub const ALL: &'static [Self] = &[$( Self::$variant, )*];

            /// The stable key.
            pub const fn key(self) -> &'static str {
                match self {
                    $( Self::$variant => $key, )*
                }
            }

            /// Whether the facet is an optional visual layer a user can turn
            /// off without turning off any semantic fact.
            pub const fn is_optional_presentation(self) -> bool {
                match self {
                    $( Self::$variant => $optional, )*
                }
            }

            /// Decode a stable key; unknown keys are `None`, never a guess.
            pub fn from_key(key: &str) -> Option<Self> {
                match key {
                    $( $key => Some(Self::$variant), )*
                    _ => None,
                }
            }
        }
    };
}

capability_operations! {
    Hover => "hover", optional = false;
    /// The separately labelled native inspection supplement of a hover.
    HoverInspection => "hover.inspection", optional = false;
    Completion => "completion", optional = false;
    /// Resolve and its auto-import edits; never survives withdrawal of the
    /// item's completion basis.
    CompletionResolve => "completion.resolve", optional = false;
    Definition => "definition", optional = false;
    TypeDefinition => "typeDefinition", optional = false;
    References => "references", optional = false;
    PrepareRename => "rename.prepare", optional = false;
    Rename => "rename", optional = false;
    DocumentHighlight => "documentHighlight", optional = false;
    SignatureHelp => "signatureHelp", optional = false;
    DocumentSymbol => "documentSymbol", optional = false;
    FoldingRange => "foldingRange", optional = false;
    SelectionRange => "selectionRange", optional = false;
    LinkedEditing => "linkedEditing", optional = false;
    DocumentLink => "documentLink", optional = false;
    CallHierarchyPrepare => "callHierarchy.prepare", optional = false;
    CallHierarchyIncoming => "callHierarchy.incoming", optional = false;
    CallHierarchyOutgoing => "callHierarchy.outgoing", optional = false;
    /// Workspace symbol presentation; workspace facts are
    /// [`Self::WorkspaceContribution`].
    WorkspaceSymbol => "workspaceSymbol", optional = false;
    TypeScriptDiagnostics => "diagnostics.typescript", optional = false;
    CompilerDiagnostics => "diagnostics.compiler", optional = false;
    ComponentUsageDiagnostics => "diagnostics.componentUsage", optional = false;
    LintDiagnostics => "diagnostics.lint", optional = false;
    QuickFix => "codeAction.quickFix", optional = false;
    OrganizeImports => "codeAction.organizeImports", optional = false;
    Refactor => "codeAction.refactor", optional = false;
    DocumentDropEdit => "documentDropEdit", optional = false;
    Formatting => "formatting", optional = false;
    OnTypeTagClosing => "onTypeFormatting.tagClosing", optional = false;
    DocumentColor => "documentColor", optional = false;
    ColorPresentation => "colorPresentation", optional = false;
    SemanticTokens => "semanticTokens", optional = false;
    InlayHint => "inlayHint", optional = true;
    CodeLens => "codeLens", optional = false;
    ApiCallDecoration => "decoration.apiCalls", optional = true;
    BindingColorDecoration => "decoration.bindingColors", optional = true;
    PropConstnessDecoration => "decoration.propConstness", optional = true;
    AnalysisInspection => "inspection.analysis", optional = false;
    BindingTypeInspection => "inspection.bindingTypes", optional = false;
    DocumentStructure => "inspection.documentStructure", optional = false;
    ComponentMetadata => "componentMeta", optional = false;
    ComponentMetaSurface => "componentMeta.surface", optional = false;
    TypeHandleExpansion => "componentMeta.typeExpansion", optional = false;
    ProjectOverview => "workspace.projectOverview", optional = false;
    ComponentParents => "workspace.componentParents", optional = false;
    RouteTree => "workspace.routeTree", optional = false;
    CompiledCodeInspection => "inspection.compiledCode", optional = false;
    VirtualFileInspection => "inspection.virtualFiles", optional = false;
    SourceMapInspection => "inspection.sourceMap", optional = false;
    /// Atomic workspace contributions and bounded index reads.
    WorkspaceContribution => "workspace.contribution", optional = false;
    ProjectCheck => "check.project", optional = false;
    Compile => "compile", optional = false;
}

impl fmt::Display for CapabilityOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// A capability qualified by its profile: one operation facet of one
/// catalog profile.
///
/// Minted only from its two inputs, so a new release (a new
/// [`FrameworkProfileId`]) yields a new capability identity while the carrier
/// capability stays equal. The project-capability bit that gates a
/// registry candidate is the separate [`crate::ProjectCapabilityId`].
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CapabilityId {
    profile: ProfileRef,
    operation: CapabilityOperation,
    digest: CanonicalDigest,
}

impl CapabilityId {
    const DOMAIN_TAG: &'static str = "verter.language.participation.capability.v1";

    pub fn new(profile: ProfileRef, operation: CapabilityOperation) -> Self {
        let mut encoder = CanonicalEncoder::new(Self::DOMAIN_TAG);
        encoder.field_bytes(1, &profile.encoded());
        encoder.field_str(2, operation.key());
        let digest = encoder.digest();
        Self {
            profile,
            operation,
            digest,
        }
    }

    pub fn profile(&self) -> &ProfileRef {
        &self.profile
    }

    pub fn operation(&self) -> CapabilityOperation {
        self.operation
    }

    pub fn digest(&self) -> CanonicalDigest {
        self.digest
    }
}

impl fmt::Debug for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CapabilityId({}:{} {})",
            self.profile.kind_discriminant(),
            self.operation.key(),
            self.digest.to_hex()
        )
    }
}
