//! Owned parse facts of a framework carrier and the exact source parse
//! identity derived from them.
//!
//! A carrier's framework parse already produced its parse identity and its
//! script-region geometry. [`FrameworkParseFacts`] is the owned projection of
//! exactly those facts, so the semantic engine and source lowering consume a
//! plain record instead of retaining the compiler's parse artifact.

use verter_language::{FileLanguage, FrameworkAdapterId, LanguageId, ParseKey};

/// The owned parse facts of one framework carrier parse: the adapter and
/// carrier language that produced it, the parse identity it recorded, and the
/// byte region of its module script (`<script module>` / legacy
/// `context="module"`), when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkParseFacts {
    adapter_id: FrameworkAdapterId,
    language_id: LanguageId,
    parse_key: ParseKey,
    module_script_region: Option<(u32, u32)>,
    /// The carrier's adapter-visible script regions, in carrier order. Their
    /// only readers are test-compiled proof surfaces, so a shipped build
    /// carries no copy.
    #[cfg(feature = "test-support")]
    script_regions: Vec<verter_language::ScriptRegion>,
}

impl FrameworkParseFacts {
    /// Record the facts a framework carrier parse produced.
    pub fn new(
        adapter_id: FrameworkAdapterId,
        language_id: LanguageId,
        parse_key: ParseKey,
        module_script_region: Option<(u32, u32)>,
    ) -> Self {
        Self {
            adapter_id,
            language_id,
            parse_key,
            module_script_region,
            #[cfg(feature = "test-support")]
            script_regions: Vec::new(),
        }
    }

    /// Record the carrier's adapter-visible script regions.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn with_script_regions(
        mut self,
        script_regions: Vec<verter_language::ScriptRegion>,
    ) -> Self {
        self.script_regions = script_regions;
        self
    }

    /// The carrier's adapter-visible script regions, in carrier order.
    #[cfg(feature = "test-support")]
    pub fn script_regions(&self) -> &[verter_language::ScriptRegion] {
        &self.script_regions
    }

    /// The framework adapter whose parse recorded these facts.
    pub fn adapter_id(&self) -> &FrameworkAdapterId {
        &self.adapter_id
    }

    /// The carrier language the parse recorded.
    pub fn language_id(&self) -> &LanguageId {
        &self.language_id
    }

    /// The parse identity the carrier parse recorded.
    pub fn parse_key(&self) -> &ParseKey {
        &self.parse_key
    }

    /// The module-script byte region of the carrier, when it records one.
    pub fn module_script_region(&self) -> Option<(u32, u32)> {
        self.module_script_region
    }

    /// The parse identity the carrier parse recorded, with the adapter and
    /// carrier language that recorded it.
    pub fn carrier_parse_key(&self) -> CarrierParseKey<'_> {
        CarrierParseKey {
            adapter_id: &self.adapter_id,
            language_id: &self.language_id,
            parse_key: &self.parse_key,
        }
    }
}

/// A borrowed carrier parse identity: the key a framework parse recorded and
/// the adapter / carrier language that recorded it.
#[derive(Debug, Clone, Copy)]
pub struct CarrierParseKey<'a> {
    pub adapter_id: &'a FrameworkAdapterId,
    pub language_id: &'a LanguageId,
    pub parse_key: &'a ParseKey,
}

/// The exact parse identity of `source` under its runtime language.
///
/// A carrier reads the key its framework parse already recorded, provided the
/// recording adapter and carrier language are the ones the language row names;
/// a mismatch is `None`. Only a source without a carrier parse derives its key,
/// by hashing the whole source under the language's default parse options.
pub fn exact_source_parse_key(
    source: &str,
    file_language: &FileLanguage,
    carrier: Option<CarrierParseKey<'_>>,
) -> Option<ParseKey> {
    match carrier {
        Some(carrier) => carrier_source_parse_key(file_language, carrier),
        None => {
            #[cfg(feature = "test-support")]
            SOURCE_PARSE_IDENTITY_DERIVATIONS.with(|count| count.set(count.get() + 1));
            #[cfg(feature = "test-support")]
            SOURCE_PARSE_IDENTITY_DERIVATIONS_IN_PROCESS
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            verter_language::default_parse_identity_for(source, file_language)
                .ok()
                .map(|(_, parse_key)| parse_key)
        }
    }
}

/// The parse identity a carrier's framework parse recorded, provided the
/// recording adapter and carrier language are the ones `file_language`
/// names; a mismatch is `None`. It reads the recorded key and never passes
/// over the source.
pub fn carrier_source_parse_key(
    file_language: &FileLanguage,
    carrier: CarrierParseKey<'_>,
) -> Option<ParseKey> {
    if carrier.adapter_id != file_language.adapter_id()?
        || Some(carrier.language_id) != file_language.carrier_language_id()
    {
        return None;
    }
    Some(carrier.parse_key.clone())
}

#[cfg(feature = "test-support")]
thread_local! {
    /// How many parse identities this thread derived by hashing a plain
    /// script's whole source through [`exact_source_parse_key`].
    static SOURCE_PARSE_IDENTITY_DERIVATIONS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// How many parse identities this thread derived from a plain script's whole
/// source text so far.
#[cfg(feature = "test-support")]
pub fn source_parse_identity_derivations_for_tests() -> usize {
    SOURCE_PARSE_IDENTITY_DERIVATIONS.with(std::cell::Cell::get)
}

/// How many parse identities every thread of this process derived by
/// hashing a whole source through [`exact_source_parse_key`].
#[cfg(feature = "test-support")]
static SOURCE_PARSE_IDENTITY_DERIVATIONS_IN_PROCESS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// How many parse identities every thread of this process derived from a
/// whole source text so far: the scheduler's workers included, so a test
/// that reads it owns its process.
#[cfg(feature = "test-support")]
pub fn source_parse_identity_derivations_in_process_for_tests() -> usize {
    SOURCE_PARSE_IDENTITY_DERIVATIONS_IN_PROCESS.load(std::sync::atomic::Ordering::Relaxed)
}
