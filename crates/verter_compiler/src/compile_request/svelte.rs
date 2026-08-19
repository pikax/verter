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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SvelteCustomElementPropDescriptor {
    pub attribute: Option<String>,
    pub reflect: Option<bool>,
    pub prop_type: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SvelteCustomElementDescriptor {
    pub tag: Option<String>,
    pub shadow: Option<bool>,
    pub props: std::collections::BTreeMap<String, SvelteCustomElementPropDescriptor>,
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
    pub custom_element_descriptor: Option<SvelteCustomElementDescriptor>,
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
        Ok(SvelteCompileRequest {
            dev: self.dev,
            custom_element: self.custom_element,
            custom_element_descriptor: self.custom_element_descriptor,
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
}
