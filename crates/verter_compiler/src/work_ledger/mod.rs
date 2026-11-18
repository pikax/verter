//! Compiler work ledger: labelled counts of the work one compile performs.
//!
//! Recording is compiled for an execution mode `M`. With
//! [`verter_audit::observe::Uncaptured`] every `record*` call is an empty
//! function and the ledger holds no storage, so a default build carries no
//! counter state and no per-call test. Counters describe work; no compiler
//! decision may read them.

pub mod kind;

use std::collections::BTreeMap;
use std::marker::PhantomData;

use verter_audit::observe::ObserveExec;
use verter_debug_assert::verter_debug_assert;

use crate::compile_request::ProductKind;

pub use kind::{WorkFamily, WorkKind, WORK_KIND_COUNT, WORK_LEDGER_SCHEMA_VERSION};

/// The compiler stage a unit of work is charged to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OwnerPhase {
    Parse,
    Semantic,
    Structure,
    Style,
    Planning,
    Emission,
    Assembly,
    Reuse,
}

impl OwnerPhase {
    const fn label(self) -> &'static str {
        match self {
            OwnerPhase::Parse => "parse",
            OwnerPhase::Semantic => "semantic",
            OwnerPhase::Structure => "structure",
            OwnerPhase::Style => "style",
            OwnerPhase::Planning => "planning",
            OwnerPhase::Emission => "emission",
            OwnerPhase::Assembly => "assembly",
            OwnerPhase::Reuse => "reuse",
        }
    }
}

/// Storage lifetime class an allocation is charged to. Mirrors the lifetime
/// markers of `framework_common::structure`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AllocationClass {
    Frontend,
    Semantic,
    CompilerScratch,
    TargetScratch,
    Emission,
}

impl AllocationClass {
    const fn label(self) -> &'static str {
        match self {
            AllocationClass::Frontend => "frontend",
            AllocationClass::Semantic => "semantic",
            AllocationClass::CompilerScratch => "compiler_scratch",
            AllocationClass::TargetScratch => "target_scratch",
            AllocationClass::Emission => "emission",
        }
    }
}

/// Grammar a parse counter is keyed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorkGrammar {
    Carrier,
    Template,
    Script,
    Style,
    Expression,
}

impl WorkGrammar {
    const fn label(self) -> &'static str {
        match self {
            WorkGrammar::Carrier => "carrier",
            WorkGrammar::Template => "template",
            WorkGrammar::Script => "script",
            WorkGrammar::Style => "style",
            WorkGrammar::Expression => "expression",
        }
    }
}

/// Outcome a style match counter is keyed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MatchOutcome {
    Yes,
    Maybe,
    No,
}

impl MatchOutcome {
    const fn label(self) -> &'static str {
        match self {
            MatchOutcome::Yes => "yes",
            MatchOutcome::Maybe => "maybe",
            MatchOutcome::No => "no",
        }
    }
}

/// Sub-key of a keyed counter. Unkeyed counters use [`WorkKey::None`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorkKey {
    None,
    Grammar(WorkGrammar),
    Lifetime(AllocationClass),
    Product(ProductKindKey),
    Match(MatchOutcome),
}

impl WorkKey {
    /// Exported spelling, `None` for an unkeyed row.
    fn label(self) -> Option<String> {
        match self {
            WorkKey::None => None,
            WorkKey::Grammar(grammar) => Some(format!("grammar:{}", grammar.label())),
            WorkKey::Lifetime(class) => Some(format!("lifetime:{}", class.label())),
            WorkKey::Product(product) => Some(format!("product:{}", product.label())),
            WorkKey::Match(outcome) => Some(format!("match:{}", outcome.label())),
        }
    }

    /// Whether `key` is the kind of key `kind` is keyed by, with `None` only
    /// for unkeyed kinds.
    fn fits(kind: WorkKind, key: WorkKey) -> bool {
        if !kind.is_keyed() {
            return key == WorkKey::None;
        }
        match kind {
            WorkKind::ParseRegionScans | WorkKind::ParseBytes => {
                matches!(key, WorkKey::Grammar(_))
            }
            WorkKind::MemoryAllocatedByLifetime | WorkKind::MemoryPeakByLifetime => {
                matches!(key, WorkKey::Lifetime(_))
            }
            WorkKind::MemoryRetainedByProduct => matches!(key, WorkKey::Product(_)),
            WorkKind::StyleMatchYesMaybeNo => matches!(key, WorkKey::Match(_)),
            _ => false,
        }
    }
}

/// Orderable mirror of [`ProductKind`] (which is not `Ord`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProductKindKey(pub u8);

impl ProductKindKey {
    pub fn of(kind: ProductKind) -> Self {
        Self(kind.ordinal())
    }

    /// The product's request spelling, owned by [`ProductKind::wire_tag`].
    fn label(self) -> &'static str {
        ProductKind::ALL
            .get(usize::from(self.0))
            .map_or("unknown", |kind| kind.wire_tag())
    }
}

/// One exported counter row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LedgerEntry {
    pub kind: WorkKind,
    pub key: WorkKey,
    pub phase: OwnerPhase,
    pub count: u64,
}

/// Deterministic export: rows sorted by `(kind, key)`, zero rows omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerSnapshot {
    pub schema_version: u32,
    pub entries: Vec<LedgerEntry>,
}

impl LedgerSnapshot {
    /// Total of `kind` across every key.
    pub fn count(&self, kind: WorkKind) -> u64 {
        self.entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| entry.count)
            .sum()
    }

    pub fn count_keyed(&self, kind: WorkKind, key: WorkKey) -> u64 {
        self.entries
            .iter()
            .filter(|entry| entry.kind == kind && entry.key == key)
            .map(|entry| entry.count)
            .sum()
    }

    pub fn family_total(&self, family: WorkFamily) -> u64 {
        self.entries
            .iter()
            .filter(|entry| entry.kind.family() == family)
            .map(|entry| entry.count)
            .sum()
    }

    /// Versioned JSON export. Every name is a static label, so no escaping
    /// is needed.
    pub fn to_json(&self) -> String {
        let mut json = format!("{{\"schemaVersion\":{},\"entries\":[", self.schema_version);
        for (index, entry) in self.entries.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            json.push_str(&format!("{{\"kind\":\"{}\"", entry.kind.name()));
            if let Some(key) = entry.key.label() {
                json.push_str(&format!(",\"key\":\"{key}\""));
            }
            json.push_str(&format!(
                ",\"phase\":\"{}\",\"count\":{}}}",
                entry.phase.label(),
                entry.count
            ));
        }
        json.push_str("]}");
        json
    }
}

/// The ledger of one compile. Create one per request at the entry point and
/// pass it by `&mut` to every stage.
pub struct CompilerWorkLedger<M: ObserveExec> {
    counts: Option<Box<LedgerCounts>>,
    mode: PhantomData<M>,
}

struct LedgerCounts {
    unkeyed: [u64; WORK_KIND_COUNT],
    keyed: BTreeMap<(WorkKind, WorkKey), u64>,
    live: BTreeMap<AllocationClass, u64>,
    peak: BTreeMap<AllocationClass, u64>,
}

impl<M: ObserveExec> CompilerWorkLedger<M> {
    /// Allocates counter storage only when `M::CAPTURED`.
    pub fn new() -> Self {
        let counts = if M::CAPTURED {
            Some(Box::new(LedgerCounts {
                unkeyed: [0; WORK_KIND_COUNT],
                keyed: BTreeMap::new(),
                live: BTreeMap::new(),
                peak: BTreeMap::new(),
            }))
        } else {
            None
        };
        Self {
            counts,
            mode: PhantomData,
        }
    }

    /// Add `n` to an unkeyed counter.
    #[inline]
    pub fn record(&mut self, kind: WorkKind, n: u64) {
        if M::CAPTURED {
            verter_debug_assert!(
                WorkKey::fits(kind, WorkKey::None),
                "{} is keyed; use record_keyed",
                kind.name()
            );
            if let Some(counts) = self.counts.as_mut() {
                let slot = &mut counts.unkeyed[kind as usize];
                *slot = slot.saturating_add(n);
            }
        }
    }

    /// Add `n` to a keyed counter. The peak counter is derived by
    /// [`Self::record_allocation`] and is not recorded directly.
    #[inline]
    pub fn record_keyed(&mut self, kind: WorkKind, key: WorkKey, n: u64) {
        if M::CAPTURED {
            verter_debug_assert!(
                WorkKey::fits(kind, key) && kind != WorkKind::MemoryPeakByLifetime,
                "{} cannot be recorded with key {:?}",
                kind.name(),
                key
            );
            if let Some(counts) = self.counts.as_mut() {
                let slot = counts.keyed.entry((kind, key)).or_insert(0);
                *slot = slot.saturating_add(n);
            }
        }
    }

    /// Charge `bytes` allocated in `class`, and raise that class's peak.
    #[inline]
    pub fn record_allocation(&mut self, class: AllocationClass, bytes: u64) {
        if M::CAPTURED {
            if let Some(counts) = self.counts.as_mut() {
                let allocated = counts
                    .keyed
                    .entry((
                        WorkKind::MemoryAllocatedByLifetime,
                        WorkKey::Lifetime(class),
                    ))
                    .or_insert(0);
                *allocated = allocated.saturating_add(bytes);
                let live = counts.live.entry(class).or_insert(0);
                *live = live.saturating_add(bytes);
                let live = *live;
                let peak = counts.peak.entry(class).or_insert(0);
                *peak = (*peak).max(live);
            }
        }
    }

    /// Release `bytes` from `class` (peak is unchanged).
    #[inline]
    pub fn record_release(&mut self, class: AllocationClass, bytes: u64) {
        if M::CAPTURED {
            if let Some(counts) = self.counts.as_mut() {
                let live = counts.live.entry(class).or_insert(0);
                *live = live.saturating_sub(bytes);
            }
        }
    }

    /// `None` when capture is compiled out or not selected.
    pub fn snapshot(&self) -> Option<LedgerSnapshot> {
        let counts = self.counts.as_ref()?;
        let mut entries = Vec::new();
        for kind in WorkKind::ALL {
            let count = counts.unkeyed[kind as usize];
            if count > 0 {
                entries.push(LedgerEntry {
                    kind,
                    key: WorkKey::None,
                    phase: kind.phase(),
                    count,
                });
            }
        }
        for (&(kind, key), &count) in &counts.keyed {
            if count > 0 {
                entries.push(LedgerEntry {
                    kind,
                    key,
                    phase: kind.phase(),
                    count,
                });
            }
        }
        for (&class, &count) in &counts.peak {
            if count > 0 {
                entries.push(LedgerEntry {
                    kind: WorkKind::MemoryPeakByLifetime,
                    key: WorkKey::Lifetime(class),
                    phase: WorkKind::MemoryPeakByLifetime.phase(),
                    count,
                });
            }
        }
        entries.sort_by_key(|entry| (entry.kind, entry.key));
        Some(LedgerSnapshot {
            schema_version: WORK_LEDGER_SCHEMA_VERSION,
            entries,
        })
    }
}

impl<M: ObserveExec> Default for CompilerWorkLedger<M> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_audit::observe::Uncaptured;

    #[test]
    fn uncaptured_ledger_holds_no_storage() {
        assert_eq!(
            std::mem::size_of::<CompilerWorkLedger<Uncaptured>>(),
            std::mem::size_of::<usize>()
        );
        let mut ledger = CompilerWorkLedger::<Uncaptured>::new();
        ledger.record(WorkKind::StyleBlocks, 3);
        ledger.record_keyed(
            WorkKind::ParseBytes,
            WorkKey::Grammar(WorkGrammar::Template),
            9,
        );
        ledger.record_allocation(AllocationClass::Emission, 5);
        ledger.record_release(AllocationClass::Emission, 5);
        assert!(ledger.snapshot().is_none());
    }

    #[test]
    fn keyed_kinds_accept_exactly_one_key_class() {
        let samples = [
            WorkKey::Grammar(WorkGrammar::Script),
            WorkKey::Lifetime(AllocationClass::Emission),
            WorkKey::Product(ProductKindKey(0)),
            WorkKey::Match(MatchOutcome::Maybe),
        ];
        for kind in WorkKind::ALL {
            let accepted = samples
                .iter()
                .filter(|key| WorkKey::fits(kind, **key))
                .count();
            if kind.is_keyed() {
                assert_eq!(accepted, 1, "{}", kind.name());
                assert!(!WorkKey::fits(kind, WorkKey::None), "{}", kind.name());
            } else {
                assert_eq!(accepted, 0, "{}", kind.name());
                assert!(WorkKey::fits(kind, WorkKey::None), "{}", kind.name());
            }
        }
    }

    #[test]
    fn product_keys_name_their_product() {
        for kind in ProductKind::ALL {
            assert_eq!(ProductKindKey::of(kind).label(), kind.wire_tag());
        }
    }

    #[cfg(feature = "semantic-observe")]
    mod captured {
        use super::*;
        use verter_audit::observe::Captured;

        type Ledger = CompilerWorkLedger<Captured>;

        #[test]
        fn equal_work_gives_equal_snapshots() {
            let mut first = Ledger::new();
            first.record(WorkKind::StyleBlocks, 2);
            first.record(WorkKind::ParseFullSourceScans, 1);
            first.record_keyed(
                WorkKind::ParseBytes,
                WorkKey::Grammar(WorkGrammar::Script),
                7,
            );
            let mut second = Ledger::new();
            second.record_keyed(
                WorkKind::ParseBytes,
                WorkKey::Grammar(WorkGrammar::Script),
                7,
            );
            second.record(WorkKind::ParseFullSourceScans, 1);
            second.record(WorkKind::StyleBlocks, 2);
            assert_eq!(first.snapshot(), second.snapshot());
        }

        #[cfg(debug_assertions)]
        #[test]
        #[should_panic]
        fn keyed_kind_without_a_key_is_rejected() {
            Ledger::new().record_keyed(WorkKind::ParseBytes, WorkKey::None, 1);
        }

        #[cfg(debug_assertions)]
        #[test]
        #[should_panic]
        fn unkeyed_kind_with_a_key_is_rejected() {
            Ledger::new().record_keyed(
                WorkKind::StyleBlocks,
                WorkKey::Grammar(WorkGrammar::Style),
                1,
            );
        }

        #[cfg(debug_assertions)]
        #[test]
        #[should_panic]
        fn keyed_kind_recorded_unkeyed_is_rejected() {
            Ledger::new().record(WorkKind::ParseBytes, 1);
        }

        #[test]
        fn json_is_sorted_and_omits_zero_rows() {
            let mut ledger = Ledger::new();
            ledger.record(WorkKind::StyleBlocks, 2);
            ledger.record(WorkKind::StylePrunedRules, 0);
            ledger.record_keyed(
                WorkKind::ParseBytes,
                WorkKey::Grammar(WorkGrammar::Template),
                12,
            );
            ledger.record_keyed(
                WorkKind::ParseBytes,
                WorkKey::Grammar(WorkGrammar::Carrier),
                4,
            );
            let snapshot = ledger.snapshot().expect("captured");
            assert_eq!(
                snapshot.to_json(),
                concat!(
                    "{\"schemaVersion\":1,\"entries\":[",
                    "{\"kind\":\"parse.bytes\",\"key\":\"grammar:carrier\",\"phase\":\"parse\",\"count\":4},",
                    "{\"kind\":\"parse.bytes\",\"key\":\"grammar:template\",\"phase\":\"parse\",\"count\":12},",
                    "{\"kind\":\"style.blocks\",\"phase\":\"style\",\"count\":2}",
                    "]}"
                )
            );
            assert_eq!(snapshot.count(WorkKind::ParseBytes), 16);
            assert_eq!(
                snapshot.count_keyed(
                    WorkKind::ParseBytes,
                    WorkKey::Grammar(WorkGrammar::Template)
                ),
                12
            );
            assert_eq!(snapshot.family_total(WorkFamily::Parse), 16);
            assert_eq!(snapshot.family_total(WorkFamily::Style), 2);
        }

        #[test]
        fn peak_survives_release() {
            let mut ledger = Ledger::new();
            ledger.record_allocation(AllocationClass::Semantic, 10);
            ledger.record_release(AllocationClass::Semantic, 10);
            ledger.record_allocation(AllocationClass::Semantic, 4);
            let snapshot = ledger.snapshot().expect("captured");
            let class = WorkKey::Lifetime(AllocationClass::Semantic);
            assert_eq!(
                snapshot.count_keyed(WorkKind::MemoryAllocatedByLifetime, class),
                14
            );
            assert_eq!(
                snapshot.count_keyed(WorkKind::MemoryPeakByLifetime, class),
                10
            );
        }

        #[test]
        fn classes_are_independent() {
            let mut ledger = Ledger::new();
            ledger.record_allocation(AllocationClass::Frontend, 8);
            ledger.record_allocation(AllocationClass::Emission, 3);
            ledger.record_release(AllocationClass::Frontend, 8);
            let snapshot = ledger.snapshot().expect("captured");
            let emission = WorkKey::Lifetime(AllocationClass::Emission);
            let frontend = WorkKey::Lifetime(AllocationClass::Frontend);
            let semantic = WorkKey::Lifetime(AllocationClass::Semantic);
            assert_eq!(
                snapshot.count_keyed(WorkKind::MemoryPeakByLifetime, emission),
                3
            );
            assert_eq!(
                snapshot.count_keyed(WorkKind::MemoryPeakByLifetime, frontend),
                8
            );
            assert_eq!(
                snapshot.count_keyed(WorkKind::MemoryAllocatedByLifetime, semantic),
                0
            );
        }
    }
}
