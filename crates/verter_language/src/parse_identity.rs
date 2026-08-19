//! Canonical framework syntax-profile and parse identities.
//!
//! This module owns the parse-only option vocabulary shared by carrier
//! frontends and the host. It normalizes those options once, then composes
//! the typed identities from `verter_identity`; consumers compare the compact
//! IDs instead of independently rebuilding option hashes.

use std::collections::BTreeSet;

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{CompatibilityDomainId, CompatibilityEpoch, ContentId};
pub use verter_identity::identity::{ParseKey, SyntaxProfileId};

use crate::{
    FileLanguage, FrameworkAdapterId, JsModuleKind, LanguageId, ScriptFlavor, ScriptSourceType,
};

/// Vue's own spec-defined standard interpolation delimiters
/// (`compiler-core:ParserOptions.delimiters`' official default). A named
/// constant a caller opts into explicitly — never a value this crate
/// substitutes silently for an unspecified option. Deciding public/host
/// option defaults belongs to whichever layer owns option normalization;
/// this crate only names Vue's own well-known literal.
pub const VUE_STANDARD_DELIMITER_OPEN: &str = "{{";
/// See [`VUE_STANDARD_DELIMITER_OPEN`].
pub const VUE_STANDARD_DELIMITER_CLOSE: &str = "}}";

/// Compatibility namespace for Vue syntax construction.
pub const VUE_SYNTAX_COMPATIBILITY_DOMAIN: CompatibilityDomainId =
    CompatibilityDomainId("verter.vue.syntax");
/// Initial compatibility epoch for Vue syntax construction.
pub const VUE_SYNTAX_COMPATIBILITY_EPOCH: CompatibilityEpoch = CompatibilityEpoch(0);
/// Compatibility namespace for Svelte syntax construction.
pub const SVELTE_SYNTAX_COMPATIBILITY_DOMAIN: CompatibilityDomainId =
    CompatibilityDomainId("verter.svelte.syntax");
/// Initial compatibility epoch for Svelte syntax construction.
pub const SVELTE_SYNTAX_COMPATIBILITY_EPOCH: CompatibilityEpoch = CompatibilityEpoch(0);
/// Compatibility namespace for ordinary and adapter-owned script syntax.
pub const SCRIPT_SYNTAX_COMPATIBILITY_DOMAIN: CompatibilityDomainId =
    CompatibilityDomainId("verter.script.syntax");
/// Initial compatibility epoch for script syntax construction.
pub const SCRIPT_SYNTAX_COMPATIBILITY_EPOCH: CompatibilityEpoch = CompatibilityEpoch(0);

/// Parse-affecting options threaded into a carrier frontend.
///
/// `delimiters`/`custom_elements` are Vue-only; Svelte ignores them.
/// `svelte_loose` is Svelte-only; Vue ignores it. Every field defaults to
/// its safe, currently-supported value — this crate never widens a
/// caller's request, it only rejects one it cannot honor.
///
/// All fields are mandatory, not `Option` — this type is the boundary
/// where already-normalized, already-defaulted option values enter the
/// syntax-profile encoding. Whoever constructs a `ParseOptions` decides
/// what "the caller didn't ask for anything specific" means (e.g. Vue's
/// own standard delimiters, [`VUE_STANDARD_DELIMITER_OPEN`] /
/// [`VUE_STANDARD_DELIMITER_CLOSE`]) — this type and its consumers never
/// substitute a default for an absent value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParseOptions {
    /// Custom interpolation delimiters (Vue-only; ignored by Svelte).
    pub delimiters: (String, String),
    /// Tag-name prefixes treated as custom elements (Vue-only; ignored by
    /// Svelte). An empty list is the caller's explicit choice of "none",
    /// not an unspecified value.
    pub custom_elements: Vec<String>,
    /// Requests Svelte's official `loose` parse mode (Svelte-only; ignored
    /// by Vue). Not implemented by this frontend — a `true` request is
    /// rejected before parsing (`SyntaxReject::UnsupportedProfile`), never
    /// silently downgraded to strict parsing.
    pub svelte_loose: bool,
}

impl ParseOptions {
    /// A `ParseOptions` requesting Vue's own standard delimiters and no
    /// custom elements — the explicit, named "ordinary Vue parse" choice,
    /// not an implicit fallback. Callers that mean "plain Vue parsing"
    /// opt into this constructor by name; `ParseOptions::default()`
    /// carries no opinion about Vue's delimiters at all (empty strings,
    /// meaningless to Svelte/script profiles, wrong for a real Vue parse).
    #[must_use]
    pub fn vue_standard() -> Self {
        Self {
            delimiters: (
                VUE_STANDARD_DELIMITER_OPEN.to_string(),
                VUE_STANDARD_DELIMITER_CLOSE.to_string(),
            ),
            custom_elements: Vec::new(),
            svelte_loose: false,
        }
    }
}

/// Why a file-language row cannot produce a framework parse identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseIdentityError {
    /// The row is an external template rather than a parseable source file.
    UnsupportedFileLanguage,
    /// The row names a carrier frontend whose syntax-profile schema is not
    /// part of the currently supported Vue/Svelte set.
    UnsupportedFrameworkCarrier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FrameworkSyntaxProfile {
    Vue {
        delimiter_open: String,
        delimiter_close: String,
        custom_elements: Vec<String>,
    },
    Svelte {
        loose: bool,
    },
    Script {
        source_type: ScriptSourceType,
        flavor: ScriptFlavor,
    },
}

/// Owner-side descriptor for [`SyntaxProfileId`].
///
/// Fields are private so every value has passed through the same
/// framework-aware normalization before canonical encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxProfileDescriptor {
    adapter_id: FrameworkAdapterId,
    language_id: LanguageId,
    profile: FrameworkSyntaxProfile,
}

impl SyntaxProfileDescriptor {
    /// Normalizes the current carrier frontend's parse-affecting options.
    pub fn new(
        language: &FileLanguage,
        options: &ParseOptions,
    ) -> Result<Self, ParseIdentityError> {
        if let FileLanguage::Script {
            source_type,
            flavor,
        } = language
        {
            return Ok(Self {
                adapter_id: FrameworkAdapterId::new("script"),
                language_id: script_language_id(source_type, flavor),
                profile: FrameworkSyntaxProfile::Script {
                    source_type: *source_type,
                    flavor: flavor.clone(),
                },
            });
        }
        let FileLanguage::Framework {
            adapter_id,
            language_id,
        } = language
        else {
            return Err(ParseIdentityError::UnsupportedFileLanguage);
        };

        let profile = if language.is_vue() {
            // Consume the caller's already-decided values as-is — this
            // encoder normalizes for CANONICAL EQUIVALENCE only (two
            // requests differing solely in custom-element order/duplicates
            // must encode identically); it does not decide what value an
            // unspecified delimiter/element set would have meant.
            let (delimiter_open, delimiter_close) = options.delimiters.clone();
            let custom_elements = options
                .custom_elements
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            FrameworkSyntaxProfile::Vue {
                delimiter_open,
                delimiter_close,
                custom_elements,
            }
        } else if language.is_svelte() {
            FrameworkSyntaxProfile::Svelte {
                loose: options.svelte_loose,
            }
        } else {
            return Err(ParseIdentityError::UnsupportedFrameworkCarrier);
        };

        Ok(Self {
            adapter_id: adapter_id.clone(),
            language_id: language_id.clone(),
            profile,
        })
    }
}

impl CanonicalEncode for SyntaxProfileDescriptor {
    const DOMAIN_TAG: &'static str = "verter.language.syntax_profile.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_str(1, self.adapter_id.as_str())
            .field_str(2, self.language_id.as_str());
        match &self.profile {
            FrameworkSyntaxProfile::Vue {
                delimiter_open,
                delimiter_close,
                custom_elements,
            } => {
                encoder
                    .field_enum_discriminant(3, 1)
                    .field_str(4, delimiter_open)
                    .field_str(5, delimiter_close)
                    .field_sorted_set(6, custom_elements.iter().map(String::as_bytes));
            }
            FrameworkSyntaxProfile::Svelte { loose } => {
                encoder.field_enum_discriminant(3, 2).field_bool(8, *loose);
            }
            FrameworkSyntaxProfile::Script {
                source_type,
                flavor,
            } => {
                encoder.field_enum_discriminant(3, 3);
                encode_script_source_type(encoder, 7, source_type);
                match flavor {
                    ScriptFlavor::Plain => {
                        encoder.field_enum_discriminant(9, 1);
                    }
                    ScriptFlavor::AdapterModule {
                        adapter_id,
                        language_id,
                    } => {
                        encoder
                            .field_enum_discriminant(9, 2)
                            .field_str(10, adapter_id.as_str())
                            .field_str(11, language_id.as_str());
                    }
                }
            }
        }
    }
}

/// Owner-side descriptor for [`ParseKey`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseKeyDescriptor {
    content: ContentId,
    language: LanguageId,
    compatibility_domain: CompatibilityDomainId,
    compatibility_epoch: CompatibilityEpoch,
    syntax_profile: SyntaxProfileId,
}

impl ParseKeyDescriptor {
    /// Composes the exact content, carrier language, compatibility contract,
    /// and normalized syntax profile that determine one parse construction.
    pub fn new(
        content: ContentId,
        language: LanguageId,
        compatibility_domain: CompatibilityDomainId,
        compatibility_epoch: CompatibilityEpoch,
        syntax_profile: SyntaxProfileId,
    ) -> Self {
        Self {
            content,
            language,
            compatibility_domain,
            compatibility_epoch,
            syntax_profile,
        }
    }
}

impl CanonicalEncode for ParseKeyDescriptor {
    const DOMAIN_TAG: &'static str = "verter.language.parse_key.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_bytes(1, self.content.digest().as_bytes())
            .field_str(2, self.language.as_str())
            .field_str(3, self.compatibility_domain.0)
            .field_u32(4, self.compatibility_epoch.0)
            .field_bytes(5, self.syntax_profile.digest().as_bytes());
    }
}

/// Computes the normalized syntax-profile identity for a supported carrier.
pub fn syntax_profile_id_for(
    language: &FileLanguage,
    options: &ParseOptions,
) -> Result<SyntaxProfileId, ParseIdentityError> {
    SyntaxProfileDescriptor::new(language, options)
        .map(|descriptor| SyntaxProfileId::from_canonical(&descriptor))
}

/// Computes the exact syntax-construction identity for carrier `content`.
pub fn parse_key_for(
    content: &str,
    language: &FileLanguage,
    compatibility_domain: CompatibilityDomainId,
    compatibility_epoch: CompatibilityEpoch,
    syntax_profile: &SyntaxProfileId,
) -> Result<ParseKey, ParseIdentityError> {
    let language_id = match language {
        FileLanguage::Framework { language_id, .. }
            if language.is_vue() || language.is_svelte() =>
        {
            language_id.clone()
        }
        FileLanguage::Framework { .. } => {
            return Err(ParseIdentityError::UnsupportedFrameworkCarrier)
        }
        FileLanguage::Script {
            source_type,
            flavor,
        } => script_language_id(source_type, flavor),
        FileLanguage::FrameworkTemplate { .. } => {
            return Err(ParseIdentityError::UnsupportedFileLanguage)
        }
    };
    let descriptor = ParseKeyDescriptor::new(
        ContentId::from_content_bytes(content.as_bytes()),
        language_id,
        compatibility_domain,
        compatibility_epoch,
        syntax_profile.clone(),
    );
    Ok(ParseKey::from_canonical(&descriptor))
}

/// Computes the current syntax-profile and parse identities for a file.
pub fn default_parse_identity_for(
    content: &str,
    language: &FileLanguage,
) -> Result<(SyntaxProfileId, ParseKey), ParseIdentityError> {
    let options = ParseOptions::default();
    let syntax_profile = syntax_profile_id_for(language, &options)?;
    let (domain, epoch) = if language.is_vue() {
        (
            VUE_SYNTAX_COMPATIBILITY_DOMAIN,
            VUE_SYNTAX_COMPATIBILITY_EPOCH,
        )
    } else if language.is_svelte() {
        (
            SVELTE_SYNTAX_COMPATIBILITY_DOMAIN,
            SVELTE_SYNTAX_COMPATIBILITY_EPOCH,
        )
    } else if matches!(language, FileLanguage::Script { .. }) {
        (
            SCRIPT_SYNTAX_COMPATIBILITY_DOMAIN,
            SCRIPT_SYNTAX_COMPATIBILITY_EPOCH,
        )
    } else {
        return Err(ParseIdentityError::UnsupportedFileLanguage);
    };
    let parse_key = parse_key_for(content, language, domain, epoch, &syntax_profile)?;
    Ok((syntax_profile, parse_key))
}

fn script_language_id(source_type: &ScriptSourceType, flavor: &ScriptFlavor) -> LanguageId {
    if let ScriptFlavor::AdapterModule { language_id, .. } = flavor {
        return language_id.clone();
    }
    let id = match source_type {
        ScriptSourceType::Ts => "script:ts",
        ScriptSourceType::Tsx => "script:tsx",
        ScriptSourceType::Js(JsModuleKind::Unambiguous) => "script:js:unambiguous",
        ScriptSourceType::Js(JsModuleKind::Module) => "script:js:module",
        ScriptSourceType::Js(JsModuleKind::CommonJs) => "script:js:commonjs",
        ScriptSourceType::Js(JsModuleKind::Script) => "script:js:script",
        ScriptSourceType::Jsx(JsModuleKind::Unambiguous) => "script:jsx:unambiguous",
        ScriptSourceType::Jsx(JsModuleKind::Module) => "script:jsx:module",
        ScriptSourceType::Jsx(JsModuleKind::CommonJs) => "script:jsx:commonjs",
        ScriptSourceType::Jsx(JsModuleKind::Script) => "script:jsx:script",
        ScriptSourceType::Dts => "script:dts",
    };
    LanguageId::new(id)
}

fn encode_script_source_type(
    encoder: &mut CanonicalEncoder,
    source_tag: u16,
    source_type: &ScriptSourceType,
) {
    let (source_discriminant, module_discriminant) = match source_type {
        ScriptSourceType::Ts => (1, None),
        ScriptSourceType::Tsx => (2, None),
        ScriptSourceType::Js(kind) => (3, Some(kind)),
        ScriptSourceType::Jsx(kind) => (4, Some(kind)),
        ScriptSourceType::Dts => (5, None),
    };
    encoder.field_enum_discriminant(source_tag, source_discriminant);
    if let Some(kind) = module_discriminant {
        let discriminant = match kind {
            JsModuleKind::Unambiguous => 1,
            JsModuleKind::Module => 2,
            JsModuleKind::CommonJs => 3,
            JsModuleKind::Script => 4,
        };
        encoder.field_enum_discriminant(source_tag + 1, discriminant);
    }
}
