use super::*;

fn span(start: u32, end: u32) -> FrameSpan {
    FrameSpan::rebase(0, verter_span::Span::new(start, end))
}

fn sorted(entries: &[(u32, u32)]) -> Arc<[(FrameSpan, u32)]> {
    nesting_sorted(
        entries
            .iter()
            .enumerate()
            .map(|(id, &(start, end))| (span(start, end), id as u32)),
    )
}

fn found(entries: &Arc<[(FrameSpan, u32)]>, start: u32, end: u32) -> Vec<u32> {
    let mut ids = within(entries, span(start, end));
    ids.sort_unstable();
    ids
}

#[test]
fn containment_answers_exactly_what_the_span_contains() {
    // 0: the container itself; 1, 2: nested children; 3: zero-width at its
    // end edge; 4: overlaps the right edge; 5: overlaps the left edge;
    // 6: shares its start but is wider; 7: after it.
    let entries = sorted(&[
        (10, 20),
        (11, 14),
        (15, 20),
        (20, 20),
        (18, 25),
        (5, 12),
        (10, 30),
        (21, 22),
    ]);
    assert_eq!(found(&entries, 10, 20), vec![0, 1, 2, 3]);
    // An overlapping entry's own containment ignores the entry it overlaps.
    assert_eq!(found(&entries, 18, 25), vec![3, 4, 7]);
    assert_eq!(found(&entries, 10, 30), vec![0, 1, 2, 3, 4, 6, 7]);
    assert_eq!(found(&entries, 0, 4), Vec::<u32>::new());
}

#[test]
fn containment_inspects_only_entries_starting_inside_the_span() {
    let leaves: Vec<(u32, u32)> = (0..1024).map(|i| (i * 4, i * 4 + 3)).collect();
    let entries = sorted(&leaves);
    let before = span_index_visits();
    assert_eq!(found(&entries, 400, 415), vec![100, 101, 102, 103]);
    assert_eq!(span_index_visits() - before, 4);
}

#[test]
fn nesting_order_puts_a_container_before_what_it_contains() {
    let entries = sorted(&[(3, 4), (0, 9), (0, 2), (3, 9)]);
    let order: Vec<u32> = entries.iter().map(|&(_, id)| id).collect();
    assert_eq!(order, vec![1, 2, 3, 0]);
}

fn read_index(sites: &[(u32, u32)]) -> (SkeletonSpanIndex, FlowBindingRef) {
    let binding = FlowBindingRef::Local(SkeletonBindingId(0));
    let mut reads: Vec<IndexedRead> = sites
        .iter()
        .map(|&(start, end)| IndexedRead {
            site_span: span(start, end),
            path: Arc::from(Vec::new().into_boxed_slice()),
        })
        .collect();
    reads.sort_by_key(|read| read.site_span);
    let mut reads_by_end: Vec<u32> = (0..reads.len() as u32).collect();
    reads_by_end
        .sort_by(|&l, &r| end_cmp(reads[l as usize].site_span, reads[r as usize].site_span));
    let mut groups = FxHashMap::default();
    groups.insert(binding.clone(), (0, reads.len() as u32));
    let index = SkeletonSpanIndex {
        reads: Arc::from(reads.into_boxed_slice()),
        read_groups: Arc::new(groups),
        reads_by_end: Arc::from(reads_by_end.into_boxed_slice()),
        ..SkeletonSpanIndex::default()
    };
    (index, binding)
}

#[test]
fn reads_after_keeps_overlapping_reads_and_skips_interior_ones() {
    let sites = [
        (2, 6),
        (10, 20),
        (11, 14),
        (12, 25),
        (18, 30),
        (20, 20),
        (21, 22),
        (10, 40),
    ];
    let (index, binding) = read_index(&sites);
    for &(start, end) in &[(10, 20), (11, 14), (0, 50), (12, 12), (30, 31)] {
        let target = span(start, end);
        let mut expected: Vec<FrameSpan> = sites
            .iter()
            .map(|&(s, e)| span(s, e))
            .filter(|read| *read > target && !target.contains(*read))
            .collect();
        let mut actual: Vec<FrameSpan> = index
            .reads_after(&binding, target)
            .map(|read| read.site_span)
            .collect();
        expected.sort();
        actual.sort();
        assert_eq!(actual, expected, "reads after {start}..{end}");
    }
}
