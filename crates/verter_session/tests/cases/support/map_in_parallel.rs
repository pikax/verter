//! `map` over independent items on every core, for the guards and gates
//! whose items (a production source to parse, an engine run to await) are
//! each checked on their own: one at a time they take most of a test's
//! time budget. `#[path]`-included from each entry that uses it, so every
//! entry keeps its own copy (the entries share no state).

/// `map` over every item, in item order, spread over the machine's cores.
/// A panic in `map` is resumed on the calling thread with its own payload.
pub fn map_in_parallel<T: Sync, R: Send>(items: &[T], map: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, items.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut mapped: Vec<(usize, R)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    // bounded-loop: one pass over `items`, shared by the workers.
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            break out;
                        };
                        out.push((index, map(item)));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    mapped.sort_by_key(|(index, _)| *index);
    mapped.into_iter().map(|(_, value)| value).collect()
}
