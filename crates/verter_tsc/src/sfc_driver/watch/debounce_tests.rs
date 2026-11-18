use super::Debouncer;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn a_burst_releases_distinct_paths_once_after_the_last_event() {
    let start = Instant::now();
    let quiet = Duration::from_millis(100);
    let mut debounce = Debouncer::new(quiet);
    let paths: Vec<_> = (0..5)
        .map(|i| PathBuf::from(format!("component{i}.vue")))
        .collect();

    for i in 0..50 {
        let at = start + Duration::from_millis(i);
        debounce.push(paths[4 - (i as usize % paths.len())].clone(), at);
        assert_eq!(debounce.take_ready(at), None);
    }

    assert_eq!(debounce.take_ready(start + quiet), None);
    let ready_at = start + Duration::from_millis(49) + quiet;
    assert_eq!(
        debounce.take_ready(ready_at - Duration::from_nanos(1)),
        None
    );
    let mut batch = debounce.take_ready(ready_at).expect("the window is quiet");
    batch.sort();
    assert_eq!(batch, paths);
    assert_eq!(debounce.take_ready(ready_at), None);
    assert_eq!(debounce.take_ready(ready_at + quiet), None);
}

#[test]
fn an_event_after_release_starts_a_new_window() {
    let start = Instant::now();
    let quiet = Duration::from_millis(10);
    let mut debounce = Debouncer::new(quiet);
    let first = PathBuf::from("first.vue");
    let second = PathBuf::from("second.vue");
    assert_eq!(debounce.deadline(), None);
    debounce.push(first.clone(), start);
    assert_eq!(debounce.deadline(), Some(start + quiet));
    assert_eq!(
        debounce.take_ready(start + quiet),
        Some(vec![first.clone()])
    );
    assert_eq!(debounce.deadline(), None);

    let next = start + quiet + quiet;
    debounce.push(first.clone(), next);
    debounce.push(second.clone(), next);
    assert_eq!(debounce.deadline(), Some(next + quiet));
    assert_eq!(debounce.take_ready(next), None);
    let mut batch = debounce.take_ready(next + quiet).unwrap();
    batch.sort();
    assert_eq!(batch, vec![first, second]);
    assert_eq!(debounce.take_ready(next + quiet), None);
}

#[test]
fn no_events_release_nothing() {
    let start = Instant::now();
    let mut debounce = Debouncer::new(Duration::from_millis(10));
    assert_eq!(debounce.take_ready(start), None);
    assert_eq!(debounce.take_ready(start + Duration::from_secs(1)), None);
}

#[test]
fn a_zero_window_releases_at_the_event_instant_but_not_before() {
    let start = Instant::now();
    let mut debounce = Debouncer::new(Duration::ZERO);
    let path = PathBuf::from("component.vue");
    debounce.push(path.clone(), start + Duration::from_nanos(1));
    assert_eq!(debounce.take_ready(start), None);
    assert_eq!(
        debounce.take_ready(start + Duration::from_nanos(1)),
        Some(vec![path])
    );
    assert_eq!(debounce.take_ready(start + Duration::from_nanos(1)), None);
}

#[test]
fn an_older_event_does_not_shorten_the_quiet_window() {
    let start = Instant::now();
    let quiet = Duration::from_millis(10);
    let mut debounce = Debouncer::new(quiet);
    let path = PathBuf::from("component.vue");
    let latest = start + quiet;
    debounce.push(path.clone(), latest);
    debounce.push(path.clone(), start);
    assert_eq!(debounce.deadline(), Some(latest + quiet));
    assert_eq!(debounce.take_ready(start + quiet), None);
    assert_eq!(debounce.take_ready(latest + quiet), Some(vec![path]));
}
