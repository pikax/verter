//! The canonical Svelte portion of a
//! [`crate::compile_request::CompileRequest`] — [`SvelteCompileRequest`] —
//! plus the exhaustive, structural classification of every row of
//! `svelte-options.tsv` ([`SvelteOption`]/[`SvelteOptionClass`]).

/// One row of `svelte-options.tsv` (35 data rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SvelteOption {
    // svelte:parse (3)
    ParseFilename,
    ParseModern,
    ParseLoose,

    // svelte:ModuleCompileOptions (6)
    ModuleDev,
    ModuleGenerate,
    ModuleFilename,
    ModuleRootDir,
    ModuleWarningFilter,
    ModuleExperimentalAsync,

    // svelte:CompileOptions (19)
    CompileOptionsName,
    CompileOptionsCustomElement,
    CompileOptionsAccessors,
    CompileOptionsNamespace,
    CompileOptionsImmutable,
    CompileOptionsCss,
    CompileOptionsCssHash,
    CompileOptionsPreserveComments,
    CompileOptionsPreserveWhitespace,
    CompileOptionsFragments,
    CompileOptionsRunes,
    CompileOptionsDiscloseVersion,
    CompileOptionsCompatibility,
    CompileOptionsCompatibilityComponentApi,
    CompileOptionsSourcemap,
    CompileOptionsOutputFilename,
    CompileOptionsCssOutputFilename,
    CompileOptionsHmr,
    CompileOptionsModernAst,

    // svelte:SvelteOptions.customElement (3)
    CustomElementTag,
    CustomElementShadow,
    CustomElementExtend,

    // svelte:SvelteOptions.customElement.props (3)
    CustomElementPropsAttribute,
    CustomElementPropsReflect,
    CustomElementPropsType,

    // svelte:OptimizeOptions (1)
    OptimizeOptionsHydrate,
}

/// Same closed vocabulary as [`crate::compile_request::vue::VueOptionClass`]
/// (`option-inventories.md:37-41`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteOptionClass {
    SupportedCanonical,
    Derived,
    HostResolved,
    TestOnly,
    UnsupportedFailClosed,
    NotApplicable,
}

impl SvelteOption {
    /// Exhaustive: a new `svelte-options.tsv` row without an arm here is a
    /// compile error, not a silent skip.
    pub const fn class(self) -> SvelteOptionClass {
        use SvelteOption::*;
        use SvelteOptionClass::*;
        match self {
            ParseFilename => NotApplicable,
            ParseModern => NotApplicable,
            ParseLoose => UnsupportedFailClosed,

            ModuleDev => SupportedCanonical,
            ModuleGenerate => SupportedCanonical,
            ModuleFilename => HostResolved,
            ModuleRootDir => HostResolved,
            ModuleWarningFilter => HostResolved,
            ModuleExperimentalAsync => SupportedCanonical,

            CompileOptionsName => Derived,
            CompileOptionsCustomElement => SupportedCanonical,
            CompileOptionsAccessors => UnsupportedFailClosed,
            CompileOptionsNamespace => SupportedCanonical,
            CompileOptionsImmutable => UnsupportedFailClosed,
            CompileOptionsCss => SupportedCanonical,
            CompileOptionsCssHash => HostResolved,
            CompileOptionsPreserveComments => SupportedCanonical,
            CompileOptionsPreserveWhitespace => SupportedCanonical,
            CompileOptionsFragments => SupportedCanonical,
            CompileOptionsRunes => SupportedCanonical,
            CompileOptionsDiscloseVersion => SupportedCanonical,
            CompileOptionsCompatibility => SupportedCanonical,
            CompileOptionsCompatibilityComponentApi => UnsupportedFailClosed,
            CompileOptionsSourcemap => Derived,
            CompileOptionsOutputFilename => HostResolved,
            CompileOptionsCssOutputFilename => HostResolved,
            CompileOptionsHmr => UnsupportedFailClosed,
            CompileOptionsModernAst => NotApplicable,

            CustomElementTag => SupportedCanonical,
            CustomElementShadow => SupportedCanonical,
            CustomElementExtend => UnsupportedFailClosed,

            CustomElementPropsAttribute => SupportedCanonical,
            CustomElementPropsReflect => SupportedCanonical,
            CustomElementPropsType => SupportedCanonical,

            OptimizeOptionsHydrate => TestOnly,
        }
    }

    /// The exact (`surface`, `option`) column pair of this row in
    /// `packages/framework-conformance-harness/evidence/svelte-options.tsv` —
    /// the schema identity a refusal names, never a spelling derived from
    /// the Rust variant. Exhaustive for the same reason [`Self::class`] is: a
    /// new TSV row without an arm here is a compile error.
    pub const fn tsv_row(self) -> (&'static str, &'static str) {
        use SvelteOption::*;
        match self {
            ParseFilename => ("svelte:parse", "filename"),
            ParseModern => ("svelte:parse", "modern"),
            ParseLoose => ("svelte:parse", "loose"),
            ModuleDev => ("svelte:ModuleCompileOptions", "dev"),
            ModuleGenerate => ("svelte:ModuleCompileOptions", "generate"),
            ModuleFilename => ("svelte:ModuleCompileOptions", "filename"),
            ModuleRootDir => ("svelte:ModuleCompileOptions", "rootDir"),
            ModuleWarningFilter => ("svelte:ModuleCompileOptions", "warningFilter"),
            ModuleExperimentalAsync => ("svelte:ModuleCompileOptions", "experimental.async"),
            CompileOptionsName => ("svelte:CompileOptions", "name"),
            CompileOptionsCustomElement => ("svelte:CompileOptions", "customElement"),
            CompileOptionsAccessors => ("svelte:CompileOptions", "accessors"),
            CompileOptionsNamespace => ("svelte:CompileOptions", "namespace"),
            CompileOptionsImmutable => ("svelte:CompileOptions", "immutable"),
            CompileOptionsCss => ("svelte:CompileOptions", "css"),
            CompileOptionsCssHash => ("svelte:CompileOptions", "cssHash"),
            CompileOptionsPreserveComments => ("svelte:CompileOptions", "preserveComments"),
            CompileOptionsPreserveWhitespace => ("svelte:CompileOptions", "preserveWhitespace"),
            CompileOptionsFragments => ("svelte:CompileOptions", "fragments"),
            CompileOptionsRunes => ("svelte:CompileOptions", "runes"),
            CompileOptionsDiscloseVersion => ("svelte:CompileOptions", "discloseVersion"),
            CompileOptionsCompatibility => ("svelte:CompileOptions", "compatibility"),
            CompileOptionsCompatibilityComponentApi => {
                ("svelte:CompileOptions", "compatibility.componentApi")
            }
            CompileOptionsSourcemap => ("svelte:CompileOptions", "sourcemap"),
            CompileOptionsOutputFilename => ("svelte:CompileOptions", "outputFilename"),
            CompileOptionsCssOutputFilename => ("svelte:CompileOptions", "cssOutputFilename"),
            CompileOptionsHmr => ("svelte:CompileOptions", "hmr"),
            CompileOptionsModernAst => ("svelte:CompileOptions", "modernAst"),
            CustomElementTag => ("svelte:SvelteOptions.customElement", "tag"),
            CustomElementShadow => ("svelte:SvelteOptions.customElement", "shadow"),
            CustomElementExtend => ("svelte:SvelteOptions.customElement", "extend"),
            CustomElementPropsAttribute => {
                ("svelte:SvelteOptions.customElement.props", "*.attribute")
            }
            CustomElementPropsReflect => ("svelte:SvelteOptions.customElement.props", "*.reflect"),
            CustomElementPropsType => ("svelte:SvelteOptions.customElement.props", "*.type"),
            OptimizeOptionsHydrate => ("svelte:OptimizeOptions", "hydrate"),
        }
    }

    /// The host compile request's own slot for this option, as
    /// `packages/native/host-compile-request.generated.ts` declares it.
    ///
    /// Same contract as
    /// [`crate::compile_request::VueOption::request_field`]: the field
    /// path a caller writes, not the official option surface
    /// [`Self::tsv_row`] quotes. `svelte:ModuleCompileOptions` +
    /// `experimental.async` is the request's flat `experimentalAsync`
    /// slot, `SvelteOptions.customElement.props` + `*.type` is
    /// `customElementDescriptor.props.*.propType`, and the request's
    /// `customElement` is a sibling boolean of the descriptor rather than
    /// its parent object.
    ///
    /// `None` means the request carries no slot for the row, so no caller
    /// can have written it. Exhaustive: a new `svelte-options.tsv` row
    /// without an arm here is a compile error.
    pub const fn request_field(self) -> Option<&'static str> {
        use SvelteOption::*;
        match self {
            ParseLoose => Some("loose"),
            ModuleDev => Some("dev"),
            ModuleGenerate => Some("generateModule"),
            ModuleExperimentalAsync => Some("experimentalAsync"),
            CompileOptionsCustomElement => Some("customElement"),
            CompileOptionsAccessors => Some("accessors"),
            CompileOptionsNamespace => Some("namespace"),
            CompileOptionsImmutable => Some("immutable"),
            CompileOptionsCss => Some("css"),
            CompileOptionsPreserveComments => Some("preserveComments"),
            CompileOptionsPreserveWhitespace => Some("preserveWhitespace"),
            CompileOptionsFragments => Some("fragments"),
            CompileOptionsRunes => Some("runes"),
            CompileOptionsDiscloseVersion => Some("discloseVersion"),
            CompileOptionsCompatibility => Some("compatibility"),
            CompileOptionsCompatibilityComponentApi => Some("compatibilityComponentApi"),
            CompileOptionsHmr => Some("hmr"),
            CustomElementTag => Some("customElementDescriptor.tag"),
            CustomElementShadow => Some("customElementDescriptor.shadow"),
            CustomElementExtend => Some("customElementExtend"),
            CustomElementPropsAttribute => Some("customElementDescriptor.props.*.attribute"),
            CustomElementPropsReflect => Some("customElementDescriptor.props.*.reflect"),
            CustomElementPropsType => Some("customElementDescriptor.props.*.propType"),

            // No slot: derived, host-resolved, oracle-only, or an output
            // shape this compiler does not publish.
            ParseFilename
            | ParseModern
            | ModuleFilename
            | ModuleRootDir
            | ModuleWarningFilter
            | CompileOptionsName
            | CompileOptionsCssHash
            | CompileOptionsSourcemap
            | CompileOptionsOutputFilename
            | CompileOptionsCssOutputFilename
            | CompileOptionsModernAst
            | OptimizeOptionsHydrate => None,
        }
    }
}

pub const ALL_SVELTE_OPTIONS: [SvelteOption; 35] = {
    use SvelteOption::*;
    [
        ParseFilename,
        ParseModern,
        ParseLoose,
        ModuleDev,
        ModuleGenerate,
        ModuleFilename,
        ModuleRootDir,
        ModuleWarningFilter,
        ModuleExperimentalAsync,
        CompileOptionsName,
        CompileOptionsCustomElement,
        CompileOptionsAccessors,
        CompileOptionsNamespace,
        CompileOptionsImmutable,
        CompileOptionsCss,
        CompileOptionsCssHash,
        CompileOptionsPreserveComments,
        CompileOptionsPreserveWhitespace,
        CompileOptionsFragments,
        CompileOptionsRunes,
        CompileOptionsDiscloseVersion,
        CompileOptionsCompatibility,
        CompileOptionsCompatibilityComponentApi,
        CompileOptionsSourcemap,
        CompileOptionsOutputFilename,
        CompileOptionsCssOutputFilename,
        CompileOptionsHmr,
        CompileOptionsModernAst,
        CustomElementTag,
        CustomElementShadow,
        CustomElementExtend,
        CustomElementPropsAttribute,
        CustomElementPropsReflect,
        CustomElementPropsType,
        OptimizeOptionsHydrate,
    ]
};

// ───────────────────────────── canonical request ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteNamespaceRequest {
    Html,
    Svg,
    MathMl,
    Foreign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteFragmentsRequest {
    Html,
    Tree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteRunesRequest {
    True,
    False,
    Infer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteCssRequest {
    Injected,
    External,
}

/// Declares the closed custom-element prop-type vocabulary from ONE list
/// of rows, each naming a variant and its two admitted spellings.
///
/// The single-list shape is the point. The vocabulary is generated from
/// these rows: the variant exists only because the row does, the
/// membership match is generated from the row's two spellings, and the
/// rendered backend spelling is generated from the row's capitalised
/// half. Adding a sixth prop type — both of its spellings, its
/// membership, and its rendering — is therefore the addition of one row,
/// and no second place can disagree about the vocabulary because no
/// second place states it.
macro_rules! svelte_custom_element_prop_types {
    ($($variant:ident => $lowercase:literal | $capitalised:literal),+ $(,)?) => {
        /// The closed Svelte custom-element prop-type vocabulary.
        ///
        /// This type IS the admission: an [`SvelteCompileRequest`] carries
        /// it rather than a string, so a request holding an unrecognised
        /// prop type is unrepresentable and no later stage — transport,
        /// normalisation, or emission — has anything left to refuse.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum SvelteCustomElementPropType {
            $($variant),+
        }

        impl SvelteCustomElementPropType {
            /// Every variant, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The sole membership decision over the vocabulary.
            ///
            /// Exactly two spellings admit each variant: the lowercase
            /// form the transport schema publishes and the capitalised
            /// form Svelte itself writes. Both were already accepted by
            /// one of the two public entry points, so admitting their
            /// union narrows neither and widens past neither. Any other
            /// casing — `STRING`, `nUmBeR` — is refused: this is a closed
            /// set of spellings, not a case-insensitive comparison.
            pub fn from_spelling(spelling: &str) -> Option<Self> {
                Some(match spelling {
                    $($lowercase | $capitalised => Self::$variant,)+
                    _ => return None,
                })
            }

            /// The capitalised spelling Svelte's backend emits. It does
            /// not depend on which admitted spelling the caller wrote, so
            /// rendered output is identical across a variant's spellings.
            pub fn as_svelte_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $capitalised),+
                }
            }
        }
    };
}

svelte_custom_element_prop_types! {
    String => "string" | "String",
    Boolean => "boolean" | "Boolean",
    Number => "number" | "Number",
    Array => "array" | "Array",
    Object => "object" | "Object",
}

/// A caller's unadmitted custom-element prop descriptor: `prop_type` is
/// still the caller's raw spelling, decided by
/// [`SvelteOptionAttempt::into_request`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SvelteCustomElementPropDescriptor {
    pub attribute: Option<String>,
    pub reflect: Option<bool>,
    pub prop_type: Option<String>,
}

/// The unadmitted custom-element descriptor a transport or host fills in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SvelteCustomElementDescriptor {
    pub tag: Option<String>,
    pub shadow: Option<bool>,
    pub props: std::collections::BTreeMap<String, SvelteCustomElementPropDescriptor>,
}

/// The admitted counterpart of [`SvelteCustomElementPropDescriptor`],
/// carrying the closed vocabulary instead of a raw spelling.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdmittedSvelteCustomElementPropDescriptor {
    pub attribute: Option<String>,
    pub reflect: Option<bool>,
    pub prop_type: Option<SvelteCustomElementPropType>,
}

/// The admitted counterpart of [`SvelteCustomElementDescriptor`], the only
/// descriptor an [`SvelteCompileRequest`] can carry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdmittedSvelteCustomElementDescriptor {
    pub tag: Option<String>,
    pub shadow: Option<bool>,
    pub props: std::collections::BTreeMap<String, AdmittedSvelteCustomElementPropDescriptor>,
}

/// The `compatibility` canonical object — only the inventoried field is
/// carried; `componentApi` (unsupported) has no slot here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SvelteCompatibilityRequest {
    _sealed: (),
}

/// The canonical, exhaustively-classified Svelte portion of a compile
/// request. Every field corresponds to exactly one `supported canonical`
/// row of `svelte-options.tsv`. There is no field for any `unsupported
/// fail-closed` row — see [`SvelteOptionAttempt`] for the typed refusal
/// surface.
///
/// `ModuleGenerate`/`ModuleExperimentalAsync` (`svelte-options.tsv`) are
/// themselves classified `SupportedCanonical` as OPTIONS — they are real,
/// well-formed Svelte options, not fabricated ones — but the CAPABILITY
/// they gate, the `SVELTE-MODULE` module-compilation product family, is
/// `unsupported fail-closed` per `capability-matrix.tsv`. There is
/// therefore no field for either on this struct either: setting them
/// refuses at [`SvelteOptionAttempt::into_request`] via
/// [`crate::compile_request::CompileRequestError::CapabilityUnsupported`],
/// the same "option admits fine but the capability it depends on does
/// not" pattern as `VUE-COMPAT-V2`/an SSR×Vapor backend combination.
#[derive(Debug, Clone, Default)]
pub struct SvelteCompileRequest {
    pub dev: Option<bool>,
    pub custom_element: Option<bool>,
    pub custom_element_descriptor: Option<AdmittedSvelteCustomElementDescriptor>,
    pub namespace: Option<SvelteNamespaceRequest>,
    pub css: Option<SvelteCssRequest>,
    pub preserve_comments: Option<bool>,
    pub preserve_whitespace: Option<bool>,
    pub fragments: Option<SvelteFragmentsRequest>,
    pub runes: Option<SvelteRunesRequest>,
    pub disclose_version: Option<bool>,
    pub compatibility: Option<SvelteCompatibilityRequest>,
}

/// Every field a legacy/transport-facing caller might still try to set for
/// Svelte — the 17 `supported canonical` rows plus the 6 `unsupported
/// fail-closed` rows. See [`crate::compile_request::vue::VueOptionAttempt`]
/// for the identical design rationale.
#[derive(Debug, Clone, Default)]
pub struct SvelteOptionAttempt {
    pub dev: Option<bool>,
    pub generate_module: Option<bool>,
    pub experimental_async: Option<bool>,
    pub custom_element: Option<bool>,
    pub custom_element_descriptor: Option<SvelteCustomElementDescriptor>,
    pub namespace: Option<SvelteNamespaceRequest>,
    pub css: Option<SvelteCssRequest>,
    pub preserve_comments: Option<bool>,
    pub preserve_whitespace: Option<bool>,
    pub fragments: Option<SvelteFragmentsRequest>,
    pub runes: Option<SvelteRunesRequest>,
    pub disclose_version: Option<bool>,
    pub compatibility: Option<SvelteCompatibilityRequest>,

    // The 6 unsupported-fail-closed slots.
    pub loose: Option<bool>,
    pub accessors: Option<bool>,
    pub immutable: Option<bool>,
    pub compatibility_component_api: Option<bool>,
    pub hmr: Option<bool>,
    pub custom_element_extend: Option<bool>,
}

impl SvelteOptionAttempt {
    /// The 6 unconditionally-unsupported option rows, plus `generate_module`
    /// / `experimental_async` — `ModuleGenerate`/`ModuleExperimentalAsync`
    /// are themselves classified `SupportedCanonical` as OPTIONS (see
    /// [`SvelteOption::class`]), but the `SVELTE-MODULE` module-compilation
    /// CAPABILITY they gate is `unsupported fail-closed` per
    /// `capability-matrix.tsv`, so they carry `Some(SvelteModule)` here
    /// rather than `None` — the option admits fine in isolation, the
    /// capability it depends on does not.
    fn unsupported_slots(
        &self,
    ) -> [(
        bool,
        SvelteOption,
        Option<crate::compile_request::CapabilityCell>,
    ); 8] {
        use crate::compile_request::CapabilityCell;
        [
            (self.loose.is_some(), SvelteOption::ParseLoose, None),
            (
                self.accessors.is_some(),
                SvelteOption::CompileOptionsAccessors,
                None,
            ),
            (
                self.immutable.is_some(),
                SvelteOption::CompileOptionsImmutable,
                None,
            ),
            (
                self.compatibility_component_api.is_some(),
                SvelteOption::CompileOptionsCompatibilityComponentApi,
                None,
            ),
            (self.hmr.is_some(), SvelteOption::CompileOptionsHmr, None),
            (
                self.custom_element_extend.is_some(),
                SvelteOption::CustomElementExtend,
                None,
            ),
            (
                self.generate_module.is_some(),
                SvelteOption::ModuleGenerate,
                Some(CapabilityCell::SvelteModule),
            ),
            (
                self.experimental_async.is_some(),
                SvelteOption::ModuleExperimentalAsync,
                Some(CapabilityCell::SvelteModule),
            ),
        ]
    }

    pub fn into_request(
        self,
    ) -> Result<SvelteCompileRequest, crate::compile_request::CompileRequestError> {
        for (present, option, capability) in self.unsupported_slots() {
            if present {
                return Err(
                    crate::compile_request::CompileRequestError::UnsupportedOption {
                        option: crate::compile_request::FrameworkOption::Svelte(option),
                        capability,
                    },
                );
            }
        }
        let custom_element_descriptor = self
            .custom_element_descriptor
            .map(admit_custom_element_descriptor)
            .transpose()?;
        Ok(SvelteCompileRequest {
            dev: self.dev,
            custom_element: self.custom_element,
            custom_element_descriptor,
            namespace: self.namespace,
            css: self.css,
            preserve_comments: self.preserve_comments,
            preserve_whitespace: self.preserve_whitespace,
            fragments: self.fragments,
            runes: self.runes,
            disclose_version: self.disclose_version,
            compatibility: self.compatibility,
        })
    }
}

/// Admits a caller's custom-element descriptor, converting every prop-type
/// spelling into the closed vocabulary.
///
/// This is the only membership decision over that vocabulary in the
/// workspace. Transports forward the caller's spelling verbatim and the
/// execution path renders an already-admitted value, so an unrecognised
/// spelling is refused here — at request construction — naming the
/// `customElement.props.type` row and carrying the offending value.
fn admit_custom_element_descriptor(
    descriptor: SvelteCustomElementDescriptor,
) -> Result<AdmittedSvelteCustomElementDescriptor, crate::compile_request::CompileRequestError> {
    let SvelteCustomElementDescriptor { tag, shadow, props } = descriptor;
    let mut admitted = std::collections::BTreeMap::new();
    for (name, prop) in props {
        let SvelteCustomElementPropDescriptor {
            attribute,
            reflect,
            prop_type,
        } = prop;
        let prop_type = match prop_type {
            None => None,
            Some(spelling) => Some(SvelteCustomElementPropType::from_spelling(&spelling).ok_or(
                crate::compile_request::CompileRequestError::MalformedOptionValue {
                    option: crate::compile_request::FrameworkOption::Svelte(
                        SvelteOption::CustomElementPropsType,
                    ),
                    value: spelling,
                },
            )?),
        };
        admitted.insert(
            name,
            AdmittedSvelteCustomElementPropDescriptor {
                attribute,
                reflect,
                prop_type,
            },
        );
    }
    Ok(AdmittedSvelteCustomElementDescriptor {
        tag,
        shadow,
        props: admitted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_svelte_options_list_matches_the_35_row_count() {
        assert_eq!(
            ALL_SVELTE_OPTIONS.len(),
            35,
            "svelte-options.tsv has 35 data rows"
        );
    }

    #[test]
    fn svelte_option_classification_counts_match_the_committed_tsv() {
        use SvelteOptionClass::*;
        let count = |c: SvelteOptionClass| {
            ALL_SVELTE_OPTIONS
                .iter()
                .filter(|o| std::mem::discriminant(&o.class()) == std::mem::discriminant(&c))
                .count()
        };
        assert_eq!(count(SupportedCanonical), 17);
        assert_eq!(count(UnsupportedFailClosed), 6);
        assert_eq!(count(HostResolved), 6);
        assert_eq!(count(NotApplicable), 3);
        assert_eq!(count(Derived), 2);
        assert_eq!(count(TestOnly), 1);
    }

    #[test]
    fn attempt_refuses_unsupported_option_even_when_explicitly_false() {
        let attempt = SvelteOptionAttempt {
            hmr: Some(false),
            ..Default::default()
        };
        let err = attempt.into_request().unwrap_err();
        match err {
            crate::compile_request::CompileRequestError::UnsupportedOption { option, .. } => {
                assert_eq!(
                    option,
                    crate::compile_request::FrameworkOption::Svelte(
                        SvelteOption::CompileOptionsHmr
                    )
                );
            }
            other => panic!("expected UnsupportedOption, got {other:?}"),
        }
    }

    /// The direct characterization test for the previously-hardcoded-None
    /// Svelte fields: the canonical request threads every one of the
    /// 8 previously-dead options through as a genuinely settable field.
    #[test]
    fn previously_hardcoded_svelte_fields_are_now_settable_on_the_canonical_request() {
        let attempt = SvelteOptionAttempt {
            runes: Some(SvelteRunesRequest::True),
            namespace: Some(SvelteNamespaceRequest::Svg),
            fragments: Some(SvelteFragmentsRequest::Tree),
            preserve_whitespace: Some(true),
            preserve_comments: Some(true),
            disclose_version: Some(false),
            compatibility: Some(SvelteCompatibilityRequest::default()),
            dev: Some(true),
            ..Default::default()
        };
        let request = attempt
            .into_request()
            .expect("all fields are supported canonical");
        assert_eq!(request.runes, Some(SvelteRunesRequest::True));
        assert_eq!(request.namespace, Some(SvelteNamespaceRequest::Svg));
        assert_eq!(request.fragments, Some(SvelteFragmentsRequest::Tree));
        assert_eq!(request.preserve_whitespace, Some(true));
        assert_eq!(request.preserve_comments, Some(true));
        assert_eq!(request.disclose_version, Some(false));
        assert!(request.compatibility.is_some());
        assert_eq!(request.dev, Some(true));
    }

    #[test]
    fn each_of_the_six_unsupported_slots_refuses_independently() {
        let base = SvelteOptionAttempt::default();
        for i in 0..6u8 {
            let mut a = base.clone();
            match i {
                0 => a.loose = Some(true),
                1 => a.accessors = Some(true),
                2 => a.immutable = Some(true),
                3 => a.compatibility_component_api = Some(true),
                4 => a.hmr = Some(true),
                5 => a.custom_element_extend = Some(true),
                _ => unreachable!(),
            }
            assert!(
                a.into_request().is_err(),
                "slot {i} must refuse construction"
            );
        }
    }
    fn attempt_with_prop_type(spelling: &str) -> SvelteOptionAttempt {
        let mut props = std::collections::BTreeMap::new();
        props.insert(
            "count".to_string(),
            SvelteCustomElementPropDescriptor {
                prop_type: Some(spelling.to_string()),
                ..Default::default()
            },
        );
        SvelteOptionAttempt {
            custom_element_descriptor: Some(SvelteCustomElementDescriptor {
                props,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn admitted_prop_type(spelling: &str) -> Option<SvelteCustomElementPropType> {
        attempt_with_prop_type(spelling)
            .into_request()
            .unwrap_or_else(|e| panic!("`{spelling}` must be admitted, got {e:?}"))
            .custom_element_descriptor
            .expect("the admitted request keeps the descriptor")
            .props
            .remove("count")
            .expect("the admitted descriptor keeps the prop")
            .prop_type
    }

    /// The admitted vocabulary is exactly each variant's capitalised
    /// spelling and its lowercase form — ten spellings over five variants.
    ///
    /// The per-variant expectations are derived from the vocabulary itself
    /// rather than restated, so a sixth prop type is covered by the loop
    /// without editing it. The roster below is deliberately NOT derived: it
    /// pins the backend spellings as literal bytes, because every other
    /// assertion here reads them back out of `as_svelte_name`, and an oracle
    /// that asks the function under test what the right answer is cannot
    /// notice the function changing its answer. These five strings are
    /// Svelte's, not this table's.
    #[test]
    fn exactly_two_spellings_admit_each_prop_type_variant() {
        assert_eq!(
            SvelteCustomElementPropType::ALL
                .iter()
                .map(|variant| variant.as_svelte_name())
                .collect::<Vec<_>>(),
            ["String", "Boolean", "Number", "Array", "Object"],
            "the rendered backend spellings are Svelte's own and are byte-pinned here"
        );
        for variant in SvelteCustomElementPropType::ALL {
            let capitalised = variant.as_svelte_name();
            let lowercase = capitalised.to_ascii_lowercase();
            assert_ne!(
                capitalised, lowercase,
                "the two spellings of {variant:?} must differ"
            );
            assert_eq!(
                SvelteCustomElementPropType::from_spelling(capitalised),
                Some(*variant)
            );
            assert_eq!(
                SvelteCustomElementPropType::from_spelling(&lowercase),
                Some(*variant)
            );
            assert_eq!(admitted_prop_type(capitalised), Some(*variant));
            assert_eq!(admitted_prop_type(&lowercase), Some(*variant));

            // Every OTHER casing of the same word is refused: the set of
            // spellings is closed, not case-normalised.
            let uppercase = capitalised.to_ascii_uppercase();
            let alternating: String = capitalised
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    if i % 2 == 0 {
                        c.to_ascii_lowercase()
                    } else {
                        c.to_ascii_uppercase()
                    }
                })
                .collect();
            for refused in [uppercase, alternating] {
                if refused == capitalised || refused == lowercase {
                    continue;
                }
                assert_eq!(
                    SvelteCustomElementPropType::from_spelling(&refused),
                    None,
                    "`{refused}` is outside the admitted set"
                );
            }
        }
    }

    /// Spellings that resemble the vocabulary but are not in it.
    #[test]
    fn a_spelling_outside_the_admitted_set_is_not_admitted() {
        for refused in [
            "", " string", "string ", "Str", "strings", "symbol", "Symbol", "bigint", "Function",
        ] {
            assert_eq!(
                SvelteCustomElementPropType::from_spelling(refused),
                None,
                "`{refused}` must not be admitted"
            );
        }
    }

    /// An absent prop type stays absent — admission decides membership, it
    /// does not invent a default.
    #[test]
    fn an_absent_prop_type_admits_as_absent() {
        let attempt = SvelteOptionAttempt {
            custom_element_descriptor: Some(SvelteCustomElementDescriptor {
                props: [(
                    "count".to_string(),
                    SvelteCustomElementPropDescriptor::default(),
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let request = attempt.into_request().expect("an absent prop type admits");
        assert_eq!(
            request
                .custom_element_descriptor
                .expect("descriptor")
                .props
                .remove("count")
                .expect("prop")
                .prop_type,
            None
        );
    }

    /// Admission carries the other descriptor fields through untouched.
    #[test]
    fn admission_preserves_the_rest_of_the_descriptor() {
        let attempt = SvelteOptionAttempt {
            custom_element_descriptor: Some(SvelteCustomElementDescriptor {
                tag: Some("x-el".to_string()),
                shadow: Some(false),
                props: [(
                    "count".to_string(),
                    SvelteCustomElementPropDescriptor {
                        attribute: Some("data-count".to_string()),
                        reflect: Some(true),
                        prop_type: Some("number".to_string()),
                    },
                )]
                .into_iter()
                .collect(),
            }),
            ..Default::default()
        };
        let descriptor = attempt
            .into_request()
            .expect("admits")
            .custom_element_descriptor
            .expect("descriptor");
        assert_eq!(descriptor.tag.as_deref(), Some("x-el"));
        assert_eq!(descriptor.shadow, Some(false));
        let prop = descriptor.props.get("count").expect("prop");
        assert_eq!(prop.attribute.as_deref(), Some("data-count"));
        assert_eq!(prop.reflect, Some(true));
        assert_eq!(prop.prop_type, Some(SvelteCustomElementPropType::Number));
    }

    /// The refusal names the offending prop, not the first prop in the map.
    #[test]
    fn the_refusal_carries_the_offending_spelling_from_any_prop() {
        let mut props = std::collections::BTreeMap::new();
        props.insert(
            "aaa".to_string(),
            SvelteCustomElementPropDescriptor {
                prop_type: Some("string".to_string()),
                ..Default::default()
            },
        );
        props.insert(
            "zzz".to_string(),
            SvelteCustomElementPropDescriptor {
                prop_type: Some("nonsense".to_string()),
                ..Default::default()
            },
        );
        let attempt = SvelteOptionAttempt {
            custom_element_descriptor: Some(SvelteCustomElementDescriptor {
                props,
                ..Default::default()
            }),
            ..Default::default()
        };
        match attempt.into_request().unwrap_err() {
            crate::compile_request::CompileRequestError::MalformedOptionValue { value, .. } => {
                assert_eq!(value, "nonsense")
            }
            other => panic!("expected MalformedOptionValue, got {other:?}"),
        }
    }

    #[test]
    fn a_prop_type_outside_the_admitted_vocabulary_is_refused_at_request_construction() {
        let mut props = std::collections::BTreeMap::new();
        props.insert(
            "count".to_string(),
            SvelteCustomElementPropDescriptor {
                prop_type: Some("nonsense".to_string()),
                ..Default::default()
            },
        );
        let attempt = SvelteOptionAttempt {
            custom_element_descriptor: Some(SvelteCustomElementDescriptor {
                props,
                ..Default::default()
            }),
            ..Default::default()
        };
        match attempt.into_request().unwrap_err() {
            crate::compile_request::CompileRequestError::MalformedOptionValue { option, value } => {
                assert_eq!(
                    option,
                    crate::compile_request::FrameworkOption::Svelte(
                        SvelteOption::CustomElementPropsType
                    )
                );
                assert_eq!(value, "nonsense");
            }
            other => panic!("expected MalformedOptionValue, got {other:?}"),
        }
    }
}
