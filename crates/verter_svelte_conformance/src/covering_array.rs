//! Deterministic constrained mixed-strength covering-array engine.
//!
//! [`generate`] selects a compact set of candidate rows satisfying every
//! interaction obligation of a [`CoverageSpec`]; [`verify`] independently checks
//! a row set against the same obligation universe and produces a
//! [`CoverageProof`]. Both are pure functions of the spec and the classifier:
//! equal inputs yield byte-identical rows and proof renderings — no randomness,
//! no map-iteration-order leaks (all orderings are derived from ordinal or
//! `BTreeSet` enumeration).
//!
//! The obligation universe (identical for both entry points):
//! 1. **Global** — every `global_strength`-subset of factors, every
//!    Supported-satisfiable level tuple.
//! 2. **Group(i)** — every `strength`-subset of an [`InteractionGroup`]'s
//!    factors, every Supported-satisfiable level tuple.
//! 3. **Focus** — every Supported-satisfiable (factor, level) cell.
//! 4. **Refusal partitions** — every distinct `Refused`/`OracleRejected`
//!    partition any candidate classifies into.
//!
//! Credit rule: obligations 1–3 are credited only by selected **Supported**
//! rows (satisfiability is likewise defined over Supported candidates);
//! obligation 4 only by a selected row of the exact matching partition.
//! `Invalid` rows are dropped outright: never selected, never crediting.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::ops::Range;

/// Opaque refusal identifier; distinct, ordered ids are all the engine needs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RefusalKind(pub u16);

/// Opaque oracle-diagnostic identifier; distinct, ordered ids are all the engine needs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DiagnosticKind(pub u16);

/// Disposition of a candidate row. The real model.rs later maps CSS refusal/
/// diagnostic enums onto the opaque ids; the engine only needs distinct, ordered ids.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Partition {
    Supported,
    Refused(RefusalKind),
    OracleRejected(DiagnosticKind),
    Invalid,
}

/// A strengthened interaction obligation over a subset of factors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InteractionGroup {
    /// Factor indices: ascending, unique, each `< N`.
    pub factors: Vec<u8>,
    /// Interaction strength: `1 <= strength <= factors.len()`.
    pub strength: u8,
}

/// Coverage demands over an `N`-factor mixed-level design.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageSpec<const N: usize> {
    /// Levels per factor, each `>= 1`.
    pub cardinalities: [u16; N],
    /// Base interaction strength: `1 <= global_strength <= N`.
    pub global_strength: u8,
    /// Additional strengthened obligations.
    pub interaction_groups: Vec<InteractionGroup>,
}

/// One candidate assignment of a level to every factor. Ordinal (selection)
/// order is lexicographic over the level tuple, factor 0 most significant.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Row<const N: usize>(pub [u16; N]);

/// A row together with its classified disposition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClassifiedRow<const N: usize> {
    pub row: Row<N>,
    pub partition: Partition,
}

/// The generated selection plus its independently verified coverage proof.
pub struct CoveringArray<const N: usize> {
    pub rows: Vec<ClassifiedRow<N>>,
    pub proof: CoverageProof,
}

/// Deterministic, serializable proof. `render()` MUST be byte-identical for equal input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageProof {
    candidates: u64,
    selected_rows: u64,
    global_required: u64,
    global_covered: u64,
    /// `(required, covered)` per interaction group, in group index order.
    groups: Vec<(u64, u64)>,
    focus_cells: u64,
    refusal_partitions: u64,
}

impl CoverageProof {
    /// Stable rendering: fixed field order, groups by index — no map-order leak.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "covering-array proof");
        let _ = writeln!(out, "candidates: {}", self.candidates);
        let _ = writeln!(out, "selected-rows: {}", self.selected_rows);
        let _ = writeln!(
            out,
            "global: required={} covered={}",
            self.global_required, self.global_covered
        );
        for (index, &(required, covered)) in self.groups.iter().enumerate() {
            let _ = writeln!(out, "group[{index}]: required={required} covered={covered}");
        }
        let _ = writeln!(out, "focus-cells: {}", self.focus_cells);
        let _ = writeln!(out, "refusal-partitions: {}", self.refusal_partitions);
        out
    }
}

/// Which obligation family an uncovered interaction belongs to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Obligation {
    Global,
    Group(usize),
    Focus,
    RefusalPartition,
}

/// The first uncovered obligation, in deterministic scan order.
///
/// For `RefusalPartition` there is no factor projection: `factors` is empty and
/// `levels` encodes the partition as `[0, refusal_kind]` for `Refused` or
/// `[1, diagnostic_kind]` for `OracleRejected`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UncoveredInteraction {
    pub obligation: Obligation,
    pub factors: Vec<u8>,
    pub levels: Vec<u16>,
}

/// Errors from [`generate`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoveringError {
    CandidateCeilingExceeded {
        candidates: u64,
        ceiling: u64,
    },
    /// The required-interaction obligation universe (the slot total of every
    /// global/group/focus projection) exceeds [`OBLIGATION_CEILING`] — the
    /// spec demands more interaction slots than the engine will allocate.
    ObligationCeilingExceeded {
        obligations: u64,
        ceiling: u64,
    },
    InvalidSpec(String),
}

/// Errors from [`verify`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoverageError {
    Uncovered(UncoveredInteraction),
    InvalidSpec(String),
}

/// Hard cap on the enumerated candidate space (`∏ cardinalities`).
pub const CANDIDATE_CEILING: u64 = 500_000;

/// Hard cap on the required-interaction OBLIGATION universe: the total
/// level-tuple slot count across every global-strength factor subset, every
/// strengthened group subset, and the per-factor focus cells. The candidate
/// ceiling alone does not bound this — a high-strength spec over many factors
/// explodes the subset universe while the row count stays small — so both
/// entry points enforce this cap BEFORE any slot allocation or
/// classification, failing typed instead of exhausting memory.
pub const OBLIGATION_CEILING: u64 = 5_000_000;

/// Generate a deterministic covering array satisfying `spec` under `classify`.
///
/// Enumerates the full cartesian candidate space in ascending ordinal order,
/// drops `Invalid` rows, greedily selects the candidate covering the most
/// currently-uncovered obligations (ties resolve to the smallest ordinal row),
/// then minimizes by reverse-delete in descending ordinal order. The result is
/// independently re-verified through [`verify`]; a self-verification failure is
/// an engine bug and panics rather than passing silently.
///
/// Two hard caps run BEFORE any classification or allocation: the candidate
/// space against [`CANDIDATE_CEILING`] and the obligation universe against
/// [`OBLIGATION_CEILING`] — an over-budget spec fails typed, never by
/// exhausting memory.
pub fn generate<const N: usize>(
    spec: &CoverageSpec<N>,
    classify: impl Fn(Row<N>) -> Partition,
) -> Result<CoveringArray<N>, CoveringError> {
    validate_spec(spec).map_err(CoveringError::InvalidSpec)?;
    let candidates = candidate_count(&spec.cardinalities);
    if candidates > CANDIDATE_CEILING {
        return Err(CoveringError::CandidateCeilingExceeded {
            candidates,
            ceiling: CANDIDATE_CEILING,
        });
    }
    let obligations = obligation_slot_count(spec);
    if obligations > OBLIGATION_CEILING {
        return Err(CoveringError::ObligationCeilingExceeded {
            obligations,
            ceiling: OBLIGATION_CEILING,
        });
    }
    let universe = Universe::build(spec, &classify, candidates);
    let selected = select_rows(&universe);
    let rows: Vec<ClassifiedRow<N>> = selected
        .iter()
        .map(|&ordinal| ClassifiedRow {
            row: universe.rows[ordinal],
            partition: universe.partitions[ordinal],
        })
        .collect();
    let proof = match verify(spec, &rows, &classify) {
        Ok(proof) => proof,
        Err(error) => {
            unreachable!("covering-array self-verification failed (engine bug): {error:?}")
        }
    };
    Ok(CoveringArray { rows, proof })
}

/// Verify `rows` against the obligation universe of `spec` under `classify`.
///
/// The universe is recomputed from scratch; the classifier is authoritative for
/// crediting (a caller-supplied [`ClassifiedRow::partition`] label cannot smuggle
/// credit), and rows outside the candidate universe credit nothing. On failure
/// the FIRST uncovered obligation in deterministic scan order is returned:
/// Global → each Group in index order → Focus → RefusalPartition; within each,
/// ascending factors then ascending levels.
pub fn verify<const N: usize>(
    spec: &CoverageSpec<N>,
    rows: &[ClassifiedRow<N>],
    classify: impl Fn(Row<N>) -> Partition,
) -> Result<CoverageProof, CoverageError> {
    validate_spec(spec).map_err(CoverageError::InvalidSpec)?;
    let candidates = candidate_count(&spec.cardinalities);
    if candidates > CANDIDATE_CEILING {
        return Err(CoverageError::InvalidSpec(format!(
            "candidate space {candidates} exceeds CANDIDATE_CEILING {CANDIDATE_CEILING}"
        )));
    }
    let obligations = obligation_slot_count(spec);
    if obligations > OBLIGATION_CEILING {
        return Err(CoverageError::InvalidSpec(format!(
            "obligation universe {obligations} exceeds OBLIGATION_CEILING {OBLIGATION_CEILING}"
        )));
    }
    let universe = Universe::build(spec, &classify, candidates);
    let mut covered = vec![false; universe.total_slots()];
    for classified in rows {
        // The recomputed universe classification is authoritative; rows with
        // out-of-range levels have no ordinal and credit nothing.
        if let Some(ordinal) = ordinal_of_row(&classified.row, &universe.cards) {
            universe.for_each_required_slot(ordinal, |slot| covered[slot] = true);
        }
    }
    if let Some(uncovered) = first_uncovered(&universe, &covered) {
        return Err(CoverageError::Uncovered(uncovered));
    }
    Ok(build_proof(&universe, &covered, rows.len() as u64))
}

// ---------------------------------------------------------------------------
// Spec validation and candidate enumeration
// ---------------------------------------------------------------------------

fn validate_spec<const N: usize>(spec: &CoverageSpec<N>) -> Result<(), String> {
    if N > usize::from(u8::MAX) + 1 {
        return Err(format!(
            "{N} factors exceed the u8 factor-index domain (at most 256 supported)"
        ));
    }
    for (factor, &cardinality) in spec.cardinalities.iter().enumerate() {
        if cardinality == 0 {
            return Err(format!("cardinality of factor {factor} must be >= 1"));
        }
    }
    if spec.global_strength == 0 || usize::from(spec.global_strength) > N {
        return Err(format!(
            "global_strength {} out of range 1..={N}",
            spec.global_strength
        ));
    }
    for (index, group) in spec.interaction_groups.iter().enumerate() {
        if group.factors.is_empty() {
            return Err(format!("interaction group {index} has no factors"));
        }
        for pair in group.factors.windows(2) {
            if pair[0] >= pair[1] {
                return Err(format!(
                    "interaction group {index} factors must be strictly ascending"
                ));
            }
        }
        if let Some(&last) = group.factors.last() {
            if usize::from(last) >= N {
                return Err(format!(
                    "interaction group {index} factor {last} out of range for {N} factors"
                ));
            }
        }
        if group.strength == 0 || usize::from(group.strength) > group.factors.len() {
            return Err(format!(
                "interaction group {index} strength {} out of range 1..={}",
                group.strength,
                group.factors.len()
            ));
        }
    }
    Ok(())
}

/// `∏ cardinalities` saturating at `u64::MAX` (any saturated value is far above
/// [`CANDIDATE_CEILING`], so the exceed path still triggers).
fn candidate_count<const N: usize>(cards: &[u16; N]) -> u64 {
    cards.iter().fold(1u64, |product, &card| {
        product.saturating_mul(u64::from(card))
    })
}

/// `Σ` over all `k`-subsets `S` of `cards` of `∏_{c ∈ S} c` — the exact total
/// level-tuple slot count of every `k`-factor projection — computed by the
/// elementary-symmetric-polynomial recurrence in `O(len · k)`, saturating at
/// `u64::MAX`. Never enumerates the subsets themselves, so a combinatorially
/// explosive spec is counted (and rejected) without allocation.
fn subset_product_sum(cards: &[u64], k: usize) -> u64 {
    let mut partial = vec![0u64; k + 1];
    partial[0] = 1;
    for &card in cards {
        // In-place update `e_j += e_{j-1} · card`, walked from the highest
        // degree down so each card contributes to every subset exactly once.
        for j in (1..=k).rev() {
            partial[j] = partial[j].saturating_add(partial[j - 1].saturating_mul(card));
        }
    }
    partial[k]
}

/// The EXACT obligation slot total [`Universe::build`] would allocate for
/// `spec`: every global-strength factor subset's level tuples, plus each
/// interaction group's strengthened subsets, plus the per-factor focus cells
/// — saturating (a saturated value is far above [`OBLIGATION_CEILING`], so
/// the exceed path still triggers). Refusal-partition obligations are
/// excluded: they are bounded by the classifier's kind vocabulary, not the
/// spec's combinatorics.
fn obligation_slot_count<const N: usize>(spec: &CoverageSpec<N>) -> u64 {
    let cards: Vec<u64> = spec.cardinalities.iter().map(|&c| u64::from(c)).collect();
    let mut total = subset_product_sum(&cards, usize::from(spec.global_strength));
    for group in &spec.interaction_groups {
        let group_cards: Vec<u64> = group
            .factors
            .iter()
            .map(|&factor| u64::from(spec.cardinalities[usize::from(factor)]))
            .collect();
        total = total.saturating_add(subset_product_sum(
            &group_cards,
            usize::from(group.strength),
        ));
    }
    total.saturating_add(
        cards
            .iter()
            .fold(0u64, |sum, &card| sum.saturating_add(card)),
    )
}

/// Decode an ordinal into its row: mixed radix, factor 0 most significant, so
/// ascending ordinals are exactly ascending lexicographic level tuples.
fn row_of_ordinal<const N: usize>(ordinal: usize, cards: &[u16; N]) -> Row<N> {
    let mut remainder = ordinal;
    let mut levels = [0u16; N];
    for (level, &card) in levels.iter_mut().zip(cards.iter()).rev() {
        let card = usize::from(card);
        *level = (remainder % card) as u16;
        remainder /= card;
    }
    Row(levels)
}

/// Inverse of [`row_of_ordinal`]; `None` when any level is out of range (the
/// row is outside the candidate universe).
fn ordinal_of_row<const N: usize>(row: &Row<N>, cards: &[u16; N]) -> Option<usize> {
    let mut ordinal = 0usize;
    for (&level, &card) in row.0.iter().zip(cards.iter()) {
        if level >= card {
            return None;
        }
        ordinal = ordinal * usize::from(card) + usize::from(level);
    }
    Some(ordinal)
}

/// All `k`-combinations of `items` in lexicographic order (`items` ascending).
fn combinations(items: &[u8], k: usize) -> Vec<Vec<u8>> {
    fn recurse(
        items: &[u8],
        k: usize,
        start: usize,
        current: &mut Vec<u8>,
        out: &mut Vec<Vec<u8>>,
    ) {
        if current.len() == k {
            out.push(current.clone());
            return;
        }
        let need = k - current.len();
        for index in start..=(items.len() - need) {
            current.push(items[index]);
            recurse(items, k, index + 1, current, out);
            current.pop();
        }
    }
    debug_assert!(k >= 1 && k <= items.len());
    let mut out = Vec::new();
    recurse(items, k, 0, &mut Vec::with_capacity(k), &mut out);
    out
}

// ---------------------------------------------------------------------------
// Obligation universe
// ---------------------------------------------------------------------------

/// One factor projection (a strength-subset of some family's factors). Its
/// level tuples occupy `offset..offset + size` in the flat slot space, indexed
/// factor-major so ascending slot order is ascending lexicographic levels.
struct Combo {
    factors: Vec<u8>,
    offset: usize,
    size: usize,
}

impl Combo {
    fn slot_of<const N: usize>(&self, row: &Row<N>, cards: &[u16; N]) -> usize {
        let mut index = 0usize;
        for &factor in &self.factors {
            let factor = usize::from(factor);
            index = index * usize::from(cards[factor]) + usize::from(row.0[factor]);
        }
        self.offset + index
    }

    fn levels_of<const N: usize>(&self, tuple_index: usize, cards: &[u16; N]) -> Vec<u16> {
        let mut remainder = tuple_index;
        let mut levels = vec![0u16; self.factors.len()];
        for (level, &factor) in levels.iter_mut().zip(self.factors.iter()).rev() {
            let card = usize::from(cards[usize::from(factor)]);
            *level = (remainder % card) as u16;
            remainder /= card;
        }
        levels
    }
}

/// The shared obligation universe both `generate` and `verify` enforce.
///
/// Slot space layout: `[0, supported_slot_count)` holds the level-tuple slots
/// of every combo (globals first, then each group's subsets in group index
/// order, then the focus cells); `satisfiable` marks which of those a Supported
/// candidate carries (only those are required). Refusal-partition obligations
/// occupy `supported_slot_count + i` for `required_partitions[i]` (ascending).
struct Universe<const N: usize> {
    product: usize,
    candidates: u64,
    cards: [u16; N],
    rows: Vec<Row<N>>,
    partitions: Vec<Partition>,
    combos: Vec<Combo>,
    global_combos: usize,
    group_ranges: Vec<(usize, usize)>,
    focus_start: usize,
    supported_slot_count: usize,
    satisfiable: Vec<bool>,
    required_partitions: Vec<Partition>,
}

impl<const N: usize> Universe<N> {
    fn build(
        spec: &CoverageSpec<N>,
        classify: &impl Fn(Row<N>) -> Partition,
        candidates: u64,
    ) -> Self {
        let cards = spec.cardinalities;
        let product = candidates as usize;
        let mut rows = Vec::with_capacity(product);
        let mut partitions = Vec::with_capacity(product);
        for ordinal in 0..product {
            let row = row_of_ordinal(ordinal, &cards);
            partitions.push(classify(row));
            rows.push(row);
        }

        let all_factors: Vec<u8> = (0..N).map(|factor| factor as u8).collect();
        let mut factor_sets = combinations(&all_factors, usize::from(spec.global_strength));
        let global_combos = factor_sets.len();
        let mut group_ranges = Vec::with_capacity(spec.interaction_groups.len());
        for group in &spec.interaction_groups {
            let start = factor_sets.len();
            factor_sets.extend(combinations(&group.factors, usize::from(group.strength)));
            group_ranges.push((start, factor_sets.len()));
        }
        let focus_start = factor_sets.len();
        factor_sets.extend(all_factors.iter().map(|&factor| vec![factor]));

        let mut combos = Vec::with_capacity(factor_sets.len());
        let mut offset = 0usize;
        for factors in factor_sets {
            let size = factors
                .iter()
                .map(|&factor| usize::from(cards[usize::from(factor)]))
                .product::<usize>();
            combos.push(Combo {
                factors,
                offset,
                size,
            });
            offset += size;
        }
        let supported_slot_count = offset;

        let mut satisfiable = vec![false; supported_slot_count];
        let mut partition_set: BTreeSet<Partition> = BTreeSet::new();
        for (ordinal, &partition) in partitions.iter().enumerate() {
            match partition {
                Partition::Supported => {
                    for combo in &combos {
                        satisfiable[combo.slot_of(&rows[ordinal], &cards)] = true;
                    }
                }
                refusal @ (Partition::Refused(_) | Partition::OracleRejected(_)) => {
                    partition_set.insert(refusal);
                }
                Partition::Invalid => {}
            }
        }

        Universe {
            product,
            candidates,
            cards,
            rows,
            partitions,
            combos,
            global_combos,
            group_ranges,
            focus_start,
            supported_slot_count,
            satisfiable,
            required_partitions: partition_set.into_iter().collect(),
        }
    }

    fn total_slots(&self) -> usize {
        self.supported_slot_count + self.required_partitions.len()
    }

    fn partition_slot(&self, partition: Partition) -> usize {
        let index = self
            .required_partitions
            .binary_search(&partition)
            .expect("partition of an in-universe candidate is always required");
        self.supported_slot_count + index
    }

    /// Visit every REQUIRED slot the candidate at `ordinal` credits under the
    /// credit rule: Supported rows credit their satisfiable combo tuples;
    /// Refused/OracleRejected rows credit exactly their partition obligation;
    /// Invalid rows credit nothing.
    fn for_each_required_slot(&self, ordinal: usize, mut visit: impl FnMut(usize)) {
        match self.partitions[ordinal] {
            Partition::Supported => {
                let row = &self.rows[ordinal];
                for combo in &self.combos {
                    let slot = combo.slot_of(row, &self.cards);
                    if self.satisfiable[slot] {
                        visit(slot);
                    }
                }
            }
            refusal @ (Partition::Refused(_) | Partition::OracleRejected(_)) => {
                visit(self.partition_slot(refusal));
            }
            Partition::Invalid => {}
        }
    }

    fn uncovered_at(
        &self,
        obligation: Obligation,
        combo_index: usize,
        tuple_index: usize,
    ) -> UncoveredInteraction {
        let combo = &self.combos[combo_index];
        UncoveredInteraction {
            obligation,
            factors: combo.factors.clone(),
            levels: combo.levels_of(tuple_index, &self.cards),
        }
    }
}

// ---------------------------------------------------------------------------
// Selection: greedy set cover + reverse-delete minimization
// ---------------------------------------------------------------------------

/// Returns the ascending ordinals of the selected rows.
fn select_rows<const N: usize>(universe: &Universe<N>) -> Vec<usize> {
    let total = universe.total_slots();
    let mut covered = vec![false; total];
    let mut uncovered =
        universe.satisfiable.iter().filter(|&&s| s).count() + universe.required_partitions.len();

    let mut selected: Vec<usize> = Vec::new();
    let mut is_selected = vec![false; universe.product];
    // Stale upper bounds for the accelerated greedy: a candidate's gain only
    // decreases as coverage grows, so a bound at or below the current best can
    // be skipped without changing the argmax or the smallest-ordinal tie rule
    // (the scan is ascending, so the first maximal candidate wins ties).
    let mut gain_bound = vec![u32::MAX; universe.product];

    while uncovered > 0 {
        let mut best: Option<(u32, usize)> = None;
        for ordinal in 0..universe.product {
            if is_selected[ordinal] || gain_bound[ordinal] == 0 {
                continue;
            }
            if let Some((best_gain, _)) = best {
                if gain_bound[ordinal] <= best_gain {
                    continue;
                }
            }
            let mut gain = 0u32;
            universe.for_each_required_slot(ordinal, |slot| gain += u32::from(!covered[slot]));
            gain_bound[ordinal] = gain;
            if gain == 0 {
                continue;
            }
            if best.is_none_or(|(best_gain, _)| gain > best_gain) {
                best = Some((gain, ordinal));
            }
        }
        let (_, pick) =
            best.expect("required obligations remain but no candidate covers them (engine bug)");
        is_selected[pick] = true;
        selected.push(pick);
        universe.for_each_required_slot(pick, |slot| {
            if !covered[slot] {
                covered[slot] = true;
                uncovered -= 1;
            }
        });
    }

    // Reverse-delete minimization: walk selected rows in descending ordinal
    // order and drop a row iff every obligation it credits stays covered.
    selected.sort_unstable();
    let mut cover_count = vec![0u32; total];
    for &ordinal in &selected {
        universe.for_each_required_slot(ordinal, |slot| cover_count[slot] += 1);
    }
    let mut kept = Vec::with_capacity(selected.len());
    for &ordinal in selected.iter().rev() {
        let mut is_sole_cover = false;
        universe.for_each_required_slot(ordinal, |slot| is_sole_cover |= cover_count[slot] < 2);
        if is_sole_cover {
            kept.push(ordinal);
        } else {
            universe.for_each_required_slot(ordinal, |slot| cover_count[slot] -= 1);
        }
    }
    kept.sort_unstable();
    kept
}

// ---------------------------------------------------------------------------
// Verification scan and proof
// ---------------------------------------------------------------------------

/// First satisfiable-but-uncovered tuple within a combo range, in combo order
/// then ascending tuple order.
fn first_hole_in_range<const N: usize>(
    universe: &Universe<N>,
    covered: &[bool],
    range: Range<usize>,
) -> Option<(usize, usize)> {
    for (offset_in_range, combo) in universe.combos[range.clone()].iter().enumerate() {
        for tuple_index in 0..combo.size {
            let slot = combo.offset + tuple_index;
            if universe.satisfiable[slot] && !covered[slot] {
                return Some((range.start + offset_in_range, tuple_index));
            }
        }
    }
    None
}

fn first_uncovered<const N: usize>(
    universe: &Universe<N>,
    covered: &[bool],
) -> Option<UncoveredInteraction> {
    if let Some((combo, tuple)) = first_hole_in_range(universe, covered, 0..universe.global_combos)
    {
        return Some(universe.uncovered_at(Obligation::Global, combo, tuple));
    }
    for (group_index, &(start, end)) in universe.group_ranges.iter().enumerate() {
        if let Some((combo, tuple)) = first_hole_in_range(universe, covered, start..end) {
            return Some(universe.uncovered_at(Obligation::Group(group_index), combo, tuple));
        }
    }
    if let Some((combo, tuple)) = first_hole_in_range(
        universe,
        covered,
        universe.focus_start..universe.combos.len(),
    ) {
        return Some(universe.uncovered_at(Obligation::Focus, combo, tuple));
    }
    for (index, &partition) in universe.required_partitions.iter().enumerate() {
        if !covered[universe.supported_slot_count + index] {
            return Some(UncoveredInteraction {
                obligation: Obligation::RefusalPartition,
                factors: Vec::new(),
                levels: encode_partition(partition),
            });
        }
    }
    None
}

/// Refusal-partition obligations carry no factor projection; see
/// [`UncoveredInteraction`] for the `[tag, id]` encoding.
fn encode_partition(partition: Partition) -> Vec<u16> {
    match partition {
        Partition::Refused(RefusalKind(kind)) => vec![0, kind],
        Partition::OracleRejected(DiagnosticKind(kind)) => vec![1, kind],
        Partition::Supported | Partition::Invalid => {
            unreachable!("only refusal partitions are partition obligations")
        }
    }
}

fn build_proof<const N: usize>(
    universe: &Universe<N>,
    covered: &[bool],
    selected_rows: u64,
) -> CoverageProof {
    let counts = |range: Range<usize>| {
        let mut required = 0u64;
        let mut done = 0u64;
        for combo in &universe.combos[range] {
            for tuple_index in 0..combo.size {
                let slot = combo.offset + tuple_index;
                if universe.satisfiable[slot] {
                    required += 1;
                    done += u64::from(covered[slot]);
                }
            }
        }
        (required, done)
    };
    let (global_required, global_covered) = counts(0..universe.global_combos);
    let groups = universe
        .group_ranges
        .iter()
        .map(|&(start, end)| counts(start..end))
        .collect();
    let (focus_cells, _) = counts(universe.focus_start..universe.combos.len());
    CoverageProof {
        candidates: universe.candidates,
        selected_rows,
        global_required,
        global_covered,
        groups,
        focus_cells,
        refusal_partitions: universe.required_partitions.len() as u64,
    }
}

#[cfg(test)]
#[path = "covering_array_tests.rs"]
mod covering_array_tests;
