//! The pure static classification registry.

use std::sync::OnceLock;

use crate::ids::CapabilityId;
use crate::language::{FileLanguage, ScriptSourceType};
use crate::parse_artifact::CarrierAccessToken;

/// A project-gated candidate classification.
///
/// The row's extension names a language only in projects with the
/// gating capability derived ON (e.g. `.html` is a framework template
/// only in projects with the owning framework's capability). Pure
/// static classification cannot read project state, so it surfaces the
/// candidate as [`StaticClassification::Gated`]; the host-level
/// classifier resolves it against the project capability snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatedCandidate {
    /// Capability bit that must be derived ON for the candidate to apply.
    pub capability: CapabilityId,
    /// Classification when the capability is ON.
    pub candidate: FileLanguage,
    /// Classification when the capability is OFF (and the pure static
    /// resolution for consumers below the host seam).
    pub fallback: FileLanguage,
}

/// How a registry row classifies its extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowClassification {
    /// The extension always classifies as this language.
    Static(FileLanguage),
    /// The extension is a project-gated candidate.
    Gated(GatedCandidate),
}

/// One extension row in the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageRow {
    /// Extension WITHOUT the leading dot. Multi-segment extensions
    /// (`"d.ts"`) are supported and match before their shorter
    /// suffixes (`"ts"`).
    pub extension: String,
    /// The row's classification.
    pub classification: RowClassification,
}

impl LanguageRow {
    /// A static extension row.
    pub fn fixed(extension: &str, language: FileLanguage) -> Self {
        Self {
            extension: extension.to_string(),
            classification: RowClassification::Static(language),
        }
    }

    /// A project-gated candidate row.
    pub fn gated(extension: &str, candidate: GatedCandidate) -> Self {
        Self {
            extension: extension.to_string(),
            classification: RowClassification::Gated(candidate),
        }
    }

    /// A framework CARRIER row. The SOLE [`CarrierAccessToken`] minting
    /// point (D-ba): the row's token is minted here, during carrier-row
    /// construction, and returned exactly ONCE to the
    /// registry-construction caller as the carrier row's registration
    /// proof. Consumers (adapter descriptors, the session's blessed
    /// `vue_parse()` accessor) RECEIVE that token; none constructs one.
    ///
    /// Crate-private: carrier rows are static built-in registration
    /// data owned by this crate. Keeping the minting row constructor
    /// out of the public API means downstream crates cannot mint a
    /// token for an ARBITRARY adapter id — the public receipt channel
    /// ([`LanguageRegistry::__built_in_with_carrier_tokens`]) hands out
    /// proofs only for the fixed built-in carrier rows.
    ///
    /// # Panics
    ///
    /// Panics when `language` is not a framework carrier — carrier rows
    /// are static built-in registration data, so a non-carrier language
    /// here is a programming error, not an input condition.
    pub(crate) fn carrier(extension: &str, language: FileLanguage) -> (Self, CarrierAccessToken) {
        let adapter_id = language
            .adapter_id()
            .cloned()
            .filter(|_| language.is_framework_carrier())
            .expect("LanguageRow::carrier requires a framework CARRIER language");
        let token = crate::parse_artifact::mint_carrier_access_token(adapter_id);
        (Self::fixed(extension, language), token)
    }
}

/// Result of pure static classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticClassification {
    /// A static extension row matched.
    Resolved(FileLanguage),
    /// A gated-candidate row matched; final resolution is host-gated.
    Gated(GatedCandidate),
    /// No registered row matched. Routing falls through to the
    /// plain-script catch-all ([`FileLanguage::script_ts`]).
    Unknown,
}

impl StaticClassification {
    /// The pure static resolution: resolved rows yield their language,
    /// gated candidates yield their ungated FALLBACK (consumers below
    /// the host seam never see project-gated candidates), unknown
    /// extensions fall through to the plain-script catch-all.
    pub fn static_resolution(self) -> FileLanguage {
        match self {
            Self::Resolved(language) => language,
            Self::Gated(candidate) => candidate.fallback,
            Self::Unknown => FileLanguage::script_ts(),
        }
    }
}

/// The extension table mapping file paths to [`FileLanguage`] rows.
///
/// PURE by construction: rows are static data; classification reads
/// only the path. Project-gated rows resolve at the host level.
#[derive(Debug, Clone)]
pub struct LanguageRegistry {
    /// Rows with precomputed `.ext` suffixes, ordered longest-suffix
    /// first so `"d.ts"` wins over `"ts"`.
    rows: Vec<(String, LanguageRow)>,
}

impl LanguageRegistry {
    /// Build a registry from rows. Row order is irrelevant; matching is
    /// longest-suffix-first.
    pub fn new(rows: Vec<LanguageRow>) -> Self {
        let mut rows: Vec<(String, LanguageRow)> = rows
            .into_iter()
            .map(|row| (format!(".{}", row.extension), row))
            .collect();
        rows.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        Self { rows }
    }

    /// The built-in rows: the TS/JS script family plus the `.vue` and
    /// `.svelte` framework carriers. Carrier registration proofs are
    /// dropped; the blessed adapter receipt sites use the hidden
    /// [`Self::__built_in_with_carrier_tokens`] channel.
    pub fn built_in() -> Self {
        Self::__built_in_with_carrier_tokens().0
    }

    /// The built-in rows plus the carrier rows' registration proofs
    /// ([`CarrierAccessToken`]s, one per carrier row, in row order).
    ///
    /// The tokens are minted during carrier-row construction
    /// ([`LanguageRow::carrier`]) and returned exactly once, here, to
    /// the registry-construction caller. The host receives its adapter
    /// tokens through this channel at host construction; there is no
    /// by-id token lookup and no arbitrary-id mint channel.
    ///
    /// Hidden, not `pub(crate)`: the sanctioned receipt site lives in
    /// `verter_session` (the Vue adapter's `vue_parse()` accessor), so
    /// the channel must cross the crate seam — a literal `pub(crate)`
    /// cannot compile there. Carrier privacy is public-hidden +
    /// token-gated + statically guarded: the
    /// `carrier_access_token_minted_only_in_verter_language` guard
    /// confines every call site of this channel to the blessed receipt
    /// allowlist, exactly like the `__carrier_downcast_*` helpers.
    #[doc(hidden)]
    pub fn __built_in_with_carrier_tokens() -> (Self, Vec<CarrierAccessToken>) {
        let (vue_row, vue_token) = LanguageRow::carrier("vue", FileLanguage::vue());
        let (svelte_row, svelte_token) = LanguageRow::carrier("svelte", FileLanguage::svelte());
        let registry = Self::new(vec![
            vue_row,
            svelte_row,
            LanguageRow::fixed("d.ts", FileLanguage::script(ScriptSourceType::Dts)),
            LanguageRow::fixed("d.mts", FileLanguage::script(ScriptSourceType::Dts)),
            LanguageRow::fixed("d.cts", FileLanguage::script(ScriptSourceType::Dts)),
            LanguageRow::fixed("ts", FileLanguage::script(ScriptSourceType::Ts)),
            LanguageRow::fixed("mts", FileLanguage::script(ScriptSourceType::Ts)),
            LanguageRow::fixed("cts", FileLanguage::script(ScriptSourceType::Ts)),
            LanguageRow::fixed("tsx", FileLanguage::script(ScriptSourceType::Tsx)),
            LanguageRow::fixed("js", FileLanguage::script(ScriptSourceType::js())),
            LanguageRow::fixed("mjs", FileLanguage::script(ScriptSourceType::mjs())),
            LanguageRow::fixed("cjs", FileLanguage::script(ScriptSourceType::cjs())),
            LanguageRow::fixed("jsx", FileLanguage::script(ScriptSourceType::jsx())),
        ]);
        (registry, vec![vue_token, svelte_token])
    }

    /// The process-wide built-in registry.
    pub fn global() -> &'static LanguageRegistry {
        static GLOBAL: OnceLock<LanguageRegistry> = OnceLock::new();
        GLOBAL.get_or_init(LanguageRegistry::built_in)
    }

    /// Pure static classification of a path against the extension
    /// table. The ONE static classification entry point: consumers
    /// below the host seam call this directly (or through
    /// [`StaticClassification::static_resolution`]); the host-level
    /// classifier composes it with the project capability snapshot.
    pub fn classify_static(&self, path: &str) -> StaticClassification {
        for (suffix, row) in &self.rows {
            if path.ends_with(suffix.as_str()) {
                return match &row.classification {
                    RowClassification::Static(language) => {
                        StaticClassification::Resolved(language.clone())
                    }
                    RowClassification::Gated(candidate) => {
                        StaticClassification::Gated(candidate.clone())
                    }
                };
            }
        }
        StaticClassification::Unknown
    }

    /// Extensions (without the leading dot) whose static row is a
    /// framework CARRIER language, in longest-suffix-first row order.
    /// Watcher globs and carrier-file scans build from this list.
    pub fn carrier_extensions(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter_map(|(_, row)| match &row.classification {
                RowClassification::Static(language) if language.is_framework_carrier() => {
                    Some(row.extension.as_str())
                }
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::FrameworkAdapterId;

    #[test]
    fn classify_table_matches_built_in_rows() {
        let registry = LanguageRegistry::built_in();
        let resolved = |path: &str| match registry.classify_static(path) {
            StaticClassification::Resolved(language) => language,
            other => panic!("expected a resolved row for {path}, got {other:?}"),
        };

        assert_eq!(resolved("/src/App.vue"), FileLanguage::vue());
        assert_eq!(resolved("/src/Box.svelte"), FileLanguage::svelte());
        assert_eq!(
            resolved("/src/a.ts"),
            FileLanguage::script(ScriptSourceType::Ts)
        );
        assert_eq!(
            resolved("/src/a.mts"),
            FileLanguage::script(ScriptSourceType::Ts)
        );
        assert_eq!(
            resolved("/src/a.cts"),
            FileLanguage::script(ScriptSourceType::Ts)
        );
        assert_eq!(
            resolved("/src/a.tsx"),
            FileLanguage::script(ScriptSourceType::Tsx)
        );
        assert_eq!(
            resolved("/src/a.js"),
            FileLanguage::script(ScriptSourceType::js())
        );
        assert_eq!(
            resolved("/src/a.mjs"),
            FileLanguage::script(ScriptSourceType::mjs())
        );
        assert_eq!(
            resolved("/src/a.cjs"),
            FileLanguage::script(ScriptSourceType::cjs())
        );
        assert_eq!(
            resolved("/src/a.jsx"),
            FileLanguage::script(ScriptSourceType::jsx())
        );
        assert_eq!(
            resolved("/src/a.d.ts"),
            FileLanguage::script(ScriptSourceType::Dts)
        );
        assert_eq!(
            resolved("/src/a.d.mts"),
            FileLanguage::script(ScriptSourceType::Dts)
        );
        assert_eq!(
            resolved("/src/a.d.cts"),
            FileLanguage::script(ScriptSourceType::Dts)
        );
    }

    #[test]
    fn svelte_is_a_known_carrier_row_not_an_unknown_extension() {
        // D-ao: `.svelte` classifies through a landed registry row, and (since
        // B8a) the carrier implementation registers behind it. Classification is
        // KNOWN, never unknown-extension fallthrough.
        let registry = LanguageRegistry::built_in();
        match registry.classify_static("/src/Box.svelte") {
            StaticClassification::Resolved(FileLanguage::Framework {
                adapter_id,
                language_id,
            }) => {
                assert_eq!(adapter_id, FrameworkAdapterId::svelte());
                assert_eq!(language_id.as_str(), "svelte");
            }
            other => panic!("expected the svelte framework row, got {other:?}"),
        }
    }

    #[test]
    fn svelte_ts_and_svelte_js_are_plain_scripts_not_carriers() {
        // D-bg: `.svelte.ts` / `.svelte.js` rune modules are NOT carriers — they
        // classify STRUCTURALLY as a plain script (the path ends with `.ts` /
        // `.js`, so it matches the plain-script row, NEVER the `.svelte` carrier
        // row). DISCRIMINATING: a naive `contains("svelte")` classifier would
        // (mis)route them to the carrier. They serve the REAL file; no carrier
        // participates.
        let registry = LanguageRegistry::built_in();

        // `.svelte.ts` → Script(Ts), NOT a framework carrier.
        let ts = registry
            .classify_static("/src/store.svelte.ts")
            .static_resolution();
        assert!(
            !ts.is_framework_carrier(),
            "a `.svelte.ts` rune module must NOT classify as a framework carrier, got {ts:?}"
        );
        assert_eq!(
            ts,
            FileLanguage::script(ScriptSourceType::Ts),
            "`.svelte.ts` resolves to the plain TS script row"
        );

        // `.svelte.js` → Script(Js), NOT a framework carrier.
        let js = registry
            .classify_static("/src/store.svelte.js")
            .static_resolution();
        assert!(
            !js.is_framework_carrier(),
            "a `.svelte.js` rune module must NOT classify as a framework carrier, got {js:?}"
        );

        // The bare `.svelte` component DOES classify as the carrier (proves the
        // discrimination is real — not a blanket non-carrier verdict).
        assert!(
            registry
                .classify_static("/src/Box.svelte")
                .static_resolution()
                .is_framework_carrier(),
            "a bare `.svelte` component IS a carrier"
        );
    }

    #[test]
    fn unknown_extension_falls_through_to_plain_script_routing() {
        let registry = LanguageRegistry::built_in();
        assert_eq!(
            registry.classify_static("/src/style.css"),
            StaticClassification::Unknown
        );
        assert_eq!(
            registry.classify_static("/src/README"),
            StaticClassification::Unknown
        );
        assert_eq!(
            registry
                .classify_static("/src/style.css")
                .static_resolution(),
            FileLanguage::script_ts()
        );
    }

    #[test]
    fn carrier_extensions_lists_vue_and_svelte() {
        let registry = LanguageRegistry::built_in();
        let mut extensions = registry.carrier_extensions();
        extensions.sort_unstable();
        assert_eq!(extensions, vec!["svelte", "vue"]);
    }

    #[test]
    fn multi_segment_extensions_win_over_suffixes() {
        // `.d.ts` must not classify as a plain `.ts` script.
        let registry = LanguageRegistry::built_in();
        assert_eq!(
            registry.classify_static("/types.d.ts").static_resolution(),
            FileLanguage::script(ScriptSourceType::Dts)
        );
    }

    #[test]
    fn gated_rows_surface_candidates_and_resolve_to_fallback_statically() {
        let candidate = GatedCandidate {
            capability: CapabilityId::new("fixture-capability"),
            candidate: FileLanguage::FrameworkTemplate {
                adapter_id: FrameworkAdapterId::new("fixture-framework"),
                owner_hint: None,
            },
            fallback: FileLanguage::script_ts(),
        };
        let registry = LanguageRegistry::new(vec![
            LanguageRow::fixed("vue", FileLanguage::vue()),
            LanguageRow::gated("html", candidate.clone()),
        ]);

        match registry.classify_static("/src/page.html") {
            StaticClassification::Gated(found) => assert_eq!(found, candidate),
            other => panic!("expected the gated candidate, got {other:?}"),
        }
        // Pure consumers (below the host seam) resolve gated rows to the
        // ungated fallback — never to the candidate.
        assert_eq!(
            registry
                .classify_static("/src/page.html")
                .static_resolution(),
            FileLanguage::script_ts()
        );
        // A gated row is NOT a carrier row.
        assert_eq!(registry.carrier_extensions(), vec!["vue"]);
    }
}
