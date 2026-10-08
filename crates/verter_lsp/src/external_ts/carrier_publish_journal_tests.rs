//! Incremental publication: per-publication work stays proportional to the records
//! written (writer and reader) with bounded compaction, and every commit boundary —
//! including a crash inside one — leaves membership whole across deletion and
//! restart.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_session::external_ts::{
    OpenState, PublishSnapshot, ScriptKind, SnapshotFile, SnapshotRole,
};

use super::*;

const HOST_VERSION: &str = "journal-test-host";
const PROJECT: &str = "d:/ws/tsconfig.json";

fn h16(s: &str) -> [u8; 16] {
    let d = blake3::hash(s.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(&d.as_bytes()[..16]);
    out
}

fn source(i: usize) -> String {
    format!("d:/ws/src/C{i}.vue")
}

fn provider(i: usize) -> String {
    format!("d:/ws/src/C{i}.vue.tsx")
}

/// One per-source live publish (the production `SourceDelta` shape) of carrier `i`.
fn publish(store: &CarrierPublishStore, ws: &str, i: usize, revision: u64) -> std::io::Result<u64> {
    let content = format!("export const C{i} = {revision};");
    let file = SnapshotFile {
        source_uri: Arc::from(source(i).as_str()),
        provider_uri: Arc::from(provider(i).as_str()),
        role: SnapshotRole::CarrierIde,
        script_kind: ScriptKind::Tsx,
        content_hash: h16(&content),
        content: Arc::from(content.as_str()),
        map_hash: [0u8; 16],
        map_json: None,
        structure: None,
        version: revision,
        open_state: OpenState::Closed,
    };
    let snapshot = PublishSnapshot {
        project: Arc::from(PROJECT),
        files: vec![file],
        resolution_map_version: 1,
        fs_generation: 1,
    };
    store.publish_batch(&PublishBatch::from_snapshot(
        ws,
        snapshot,
        None,
        OwnedSetScope::SourceDelta,
    ))
}

fn fresh() -> (CarrierPublishStore, String, tempfile::TempDir) {
    let user_tree = tempfile::tempdir().expect("user tree");
    let ws = user_tree.path().to_string_lossy().into_owned();
    (CarrierPublishStore::open(HOST_VERSION, &ws), ws, user_tree)
}

/// The carriers a manifest advertises: owned AND ready, with the owned row and the
/// ready entry agreeing on the provider (a torn publication would split them).
fn membership(manifest: &Manifest) -> BTreeSet<String> {
    let Some(project) = manifest.projects.get(PROJECT) else {
        return BTreeSet::new();
    };
    let owned: BTreeSet<String> = project
        .owned_sources
        .iter()
        .map(|o| o.provider_uri.clone())
        .collect();
    let ready: BTreeSet<String> = project.ready_files.keys().cloned().collect();
    assert_eq!(
        owned, ready,
        "every owned carrier of a whole publication is ready and vice versa"
    );
    for ready in project.ready_files.values() {
        assert!(!ready.blob_rel.is_empty());
    }
    owned
}

fn expect(indices: &[usize]) -> BTreeSet<String> {
    indices.iter().map(|&i| provider(i)).collect()
}

fn read_strict(store: &CarrierPublishStore) -> Manifest {
    store
        .read_published()
        .expect("the store reads back strictly")
        .expect("the store has published")
}

fn head_generation(store: &CarrierPublishStore) -> u64 {
    let bytes = std::fs::read(store.head_path()).expect("head");
    serde_json::from_slice::<serde_json::Value>(&bytes).expect("head json")["generation"]
        .as_u64()
        .expect("generation")
}

// ── AC: work proportional to records, bounded compaction ─────────────────────

/// Work one session of `n` publications costs: writer and an incremental reader
/// that refreshes after every publication.
struct SessionWork {
    writer: StoreWork,
    reader: StoreWork,
}

fn run_session(n: usize, distinct_sources: usize) -> SessionWork {
    let (store, ws, _ut) = fresh();
    let mut reader = PublishedStoreReader::open(store.workspace_dir());
    for k in 0..n {
        publish(&store, &ws, k % distinct_sources, k as u64).expect("publish");
        reader.refresh().expect("refresh");
        assert_eq!(
            reader.epoch(),
            Some(k as u64 + 1),
            "the reader follows every commit"
        );
    }
    let expected: Vec<usize> = (0..distinct_sources.min(n)).collect();
    assert_eq!(
        membership(&reader.manifest().expect("published")),
        expect(&expected)
    );
    assert_eq!(membership(&read_strict(&store)), expect(&expected));
    SessionWork {
        writer: store.work(),
        reader: reader.work(),
    }
}

#[test]
fn publication_work_is_proportional_to_records_with_bounded_compaction() {
    const SIZES: [usize; 4] = [128, 256, 512, 1024];
    // Churn over a fixed carrier set (compaction-heavy) and an ever-growing set
    // (every publication adds a carrier — the whole-manifest worst case).
    for distinct in [16usize, usize::MAX] {
        let mut previous: Option<(usize, u64)> = None;
        for n in SIZES {
            let work = run_session(n, distinct.min(n));
            let n64 = n as u64;
            // The writer never re-reads what it wrote: no base reload, no record
            // re-applied, exactly one append per publication.
            assert_eq!(work.writer.snapshot_loads, 0, "n={n}: writer reloaded");
            assert_eq!(
                work.writer.records_applied, 0,
                "n={n}: writer re-read records"
            );
            assert_eq!(work.writer.records_appended, n64);
            // Compaction is bounded: every compaction follows at least FLOOR records
            // and writes at most as many rows as the records it folded.
            assert!(
                work.writer.compactions <= n64 / COMPACTION_FLOOR,
                "n={n}: {} compactions",
                work.writer.compactions
            );
            assert!(
                work.writer.compaction_rows_written <= n64,
                "n={n}: compaction wrote {} rows for {n} records",
                work.writer.compaction_rows_written
            );
            // The reader decodes each record at most once (a record a compaction
            // folded before the reader looked arrives in the base instead) and
            // reloads a base only when a compaction changed the generation.
            assert_eq!(
                work.reader.records_applied + work.writer.compactions,
                n64,
                "n={n}: reader re-read records"
            );
            assert_eq!(
                work.reader.snapshot_loads,
                work.writer.compactions + 1,
                "n={n}: reader reloaded a base without a compaction"
            );
            assert!(
                work.reader.snapshot_rows_loaded <= n64,
                "n={n}: reader loaded {} base rows",
                work.reader.snapshot_rows_loaded
            );
            // Linear growth: doubling the publications at most ~doubles the bytes a
            // reader reads (a whole-manifest reader would roughly quadruple).
            let total = work.reader.journal_bytes_read;
            if let Some((prev_n, prev_total)) = previous {
                let ratio = total as f64 / prev_total as f64;
                let scale = n as f64 / prev_n as f64;
                assert!(
                    ratio <= scale * 1.25,
                    "distinct={distinct} {prev_n}->{n}: reader bytes grew {ratio:.2}x \
                     for {scale}x publications"
                );
            }
            previous = Some((n, total));
            if distinct == 16 {
                assert_eq!(
                    work.writer.compactions,
                    n64 / COMPACTION_FLOOR,
                    "n={n}: churn over a small live set compacts every FLOOR records"
                );
            }
        }
    }
}

#[test]
fn a_cold_load_reads_at_most_the_live_rows_plus_a_bounded_journal() {
    let (store, ws, _ut) = fresh();
    for k in 0..1024usize {
        publish(&store, &ws, k % 16, k as u64).expect("publish");
    }
    let mut cold = PublishedStoreReader::open(store.workspace_dir());
    cold.refresh().expect("cold load");
    let work = cold.work();
    assert_eq!(work.snapshot_loads, 1);
    assert!(work.snapshot_rows_loaded <= 32, "{work:?}");
    assert!(
        work.records_applied < COMPACTION_FLOOR,
        "the journal tail stays bounded: {work:?}"
    );
    assert_eq!(cold.epoch(), Some(1024));
    // Retired generations are removed; the current and the one before remain.
    let generation = head_generation(&store);
    let mut generations: BTreeSet<u64> = BTreeSet::new();
    for entry in std::fs::read_dir(store.workspace_dir())
        .expect("dir")
        .flatten()
    {
        if let Some(g) = entry
            .file_name()
            .to_str()
            .and_then(journal::parse_generation_file)
        {
            generations.insert(g);
        }
    }
    assert_eq!(
        generations,
        [generation - 1, generation].into_iter().collect(),
        "only the current and the previous generation stay on disk"
    );
}

// ── AC: commit-boundary faults, deletion and restart ─────────────────────────

/// Publish carriers 0 and 1, then churn carrier 0 until the next commit is the
/// one that compacts.
fn store_one_commit_before_compaction() -> (CarrierPublishStore, String, tempfile::TempDir) {
    let (store, ws, ut) = fresh();
    publish(&store, &ws, 0, 0).expect("publish 0");
    publish(&store, &ws, 1, 0).expect("publish 1");
    for revision in 1..COMPACTION_FLOOR - 2 {
        publish(&store, &ws, 0, revision).expect("churn 0");
    }
    assert_eq!(store.work().records_appended, COMPACTION_FLOOR - 1);
    assert_eq!(store.work().compactions, 0);
    (store, ws, ut)
}

#[test]
fn a_crash_at_any_commit_boundary_never_tears_membership() {
    use CommitFault::*;
    // (fault, publish result is an error, the faulted publication survives restart)
    let cases = [
        (BeforeAppend, true, false),
        (TornAppend { bytes: 1 }, true, false),
        (TornAppend { bytes: 40 }, true, false),
        (AfterAppend, true, true),
        (CompactionAfterSnapshot, false, true),
        (CompactionAfterJournal, false, true),
        (CompactionAfterHead, false, true),
    ];
    for (fault, errors, survives) in cases {
        let (store, ws, _ut) = store_one_commit_before_compaction();
        let mut follower = PublishedStoreReader::open(store.workspace_dir());
        follower.refresh().expect("follow");
        assert_eq!(membership(&follower.manifest().unwrap()), expect(&[0, 1]));
        let generation_before = head_generation(&store);

        store.arm_commit_fault(fault);
        let result = publish(&store, &ws, 2, 0);
        assert_eq!(result.is_err(), errors, "{fault:?}: {result:?}");

        // The fault fired at its boundary.
        let generation_after = head_generation(&store);
        match fault {
            CompactionAfterHead => assert_eq!(generation_after, generation_before + 1),
            _ => assert_eq!(generation_after, generation_before, "{fault:?}"),
        }

        // A reader that was following sees a whole state, never the torn record.
        follower.refresh().expect("refresh after the crash");
        let want = if survives {
            expect(&[0, 1, 2])
        } else {
            expect(&[0, 1])
        };
        assert_eq!(membership(&follower.manifest().unwrap()), want, "{fault:?}");
        // So does a cold reader.
        assert_eq!(membership(&read_strict(&store)), want, "{fault:?}");

        // Restart: a new writer process on the same store recovers and continues.
        drop(store);
        let restarted = CarrierPublishStore::open(HOST_VERSION, &ws);
        assert_eq!(membership(&restarted.current_manifest()), want, "{fault:?}");
        publish(&restarted, &ws, 3, 0).expect("publish after restart");
        let mut want = want;
        want.insert(provider(3));
        assert_eq!(membership(&read_strict(&restarted)), want, "{fault:?}");
        follower.refresh().expect("refresh after restart");
        assert_eq!(membership(&follower.manifest().unwrap()), want, "{fault:?}");
        assert_eq!(
            follower.epoch(),
            Some(read_strict(&restarted).epoch),
            "{fault:?}: the follower and a cold reader agree"
        );
    }
}

#[test]
fn deletion_survives_restart_and_compaction() {
    let (store, ws, _ut) = fresh();
    for i in 0..4 {
        publish(&store, &ws, i, 0).expect("publish");
    }
    store
        .retract_sources(PROJECT, &[&source(1)])
        .expect("retract 1");
    store
        .retract_source_from_all_projects(&source(2))
        .expect("retract 2 everywhere");
    assert_eq!(membership(&read_strict(&store)), expect(&[0, 3]));

    drop(store);
    let restarted = CarrierPublishStore::open(HOST_VERSION, &ws);
    assert_eq!(membership(&read_strict(&restarted)), expect(&[0, 3]));
    // Force compactions over the deletions: the retracted carriers stay gone.
    for revision in 0..2 * COMPACTION_FLOOR {
        publish(&restarted, &ws, 0, revision + 1).expect("churn");
    }
    assert!(restarted.work().compactions >= 1);
    assert_eq!(membership(&read_strict(&restarted)), expect(&[0, 3]));
    let base = std::fs::read_to_string(
        restarted
            .workspace_dir()
            .join(journal::snapshot_file(head_generation(&restarted))),
    )
    .expect("base");
    assert!(!base.contains(&provider(1)) && !base.contains(&provider(2)));
}

#[test]
fn two_writers_on_one_store_absorb_each_others_records() {
    let (first, ws, _ut) = fresh();
    let second = CarrierPublishStore::open(HOST_VERSION, &ws);
    assert_eq!(publish(&first, &ws, 0, 0).expect("first"), 1);
    assert_eq!(publish(&second, &ws, 1, 0).expect("second"), 2);
    assert_eq!(publish(&first, &ws, 2, 0).expect("first again"), 3);
    first
        .retract_sources(PROJECT, &[&source(1)])
        .expect("retract");
    assert_eq!(membership(&read_strict(&second)), expect(&[0, 2]));
    // The first writer absorbed one foreign record; it never reloaded a base.
    assert_eq!(first.work().records_applied, 1);
    assert_eq!(first.work().snapshot_loads, 0);
    // One writer compacting moves the other onto the new generation.
    for revision in 0..COMPACTION_FLOOR {
        publish(&first, &ws, 0, revision + 1).expect("churn");
    }
    assert!(first.work().compactions >= 1);
    publish(&second, &ws, 4, 0).expect("second after a foreign compaction");
    assert_eq!(membership(&read_strict(&first)), expect(&[0, 2, 4]));
}

#[test]
fn a_corrupt_journal_record_fails_closed_without_clobbering() {
    let (store, ws, _ut) = fresh();
    publish(&store, &ws, 0, 0).expect("publish 0");
    publish(&store, &ws, 1, 0).expect("publish 1");
    let journal_path = store
        .workspace_dir()
        .join(journal::journal_file(head_generation(&store)));
    let mut bytes = std::fs::read(&journal_path).expect("journal");
    // Flip one byte inside the FIRST record's JSON: the line stays complete, a
    // valid record follows it, so this is corruption, not a torn tail.
    let at = bytes.iter().position(|&b| b == b'{').expect("record json") + 2;
    bytes[at] ^= 0x01;
    std::fs::write(&journal_path, &bytes).expect("corrupt");

    let err = store
        .read_published()
        .expect_err("a corrupt record is an error");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    let err = publish(&CarrierPublishStore::open(HOST_VERSION, &ws), &ws, 2, 0)
        .expect_err("a restarted writer refuses to append over corruption");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(
        std::fs::read(&journal_path).expect("journal"),
        bytes,
        "the failed commit left the journal untouched"
    );
}

#[test]
fn a_re_created_store_is_reloaded_never_tailed_from_a_stale_offset() {
    let (store, ws, _ut) = fresh();
    for i in 0..3 {
        publish(&store, &ws, i, 0).expect("publish");
    }
    let mut follower = PublishedStoreReader::open(store.workspace_dir());
    follower.refresh().expect("follow");
    assert_eq!(
        membership(&follower.manifest().unwrap()),
        expect(&[0, 1, 2])
    );

    // The temp store vanishes (a temp-dir sweep) and a new process re-creates it:
    // generation 1 again, with a journal longer than the follower's old offset.
    std::fs::remove_dir_all(store.workspace_dir()).expect("wipe the store");
    drop(store);
    let recreated = CarrierPublishStore::open(HOST_VERSION, &ws);
    for i in 10..20 {
        publish(&recreated, &ws, i, 0).expect("publish into the re-created store");
    }
    assert_eq!(head_generation(&recreated), 1);
    follower
        .refresh()
        .expect("refresh over the re-created store");
    let want: Vec<usize> = (10..20).collect();
    assert_eq!(membership(&follower.manifest().unwrap()), expect(&want));
}

#[test]
fn fnv1a32_matches_the_published_vectors() {
    // Test vectors from the FNV reference (http://www.isthe.com/chongo/tech/comp/fnv/).
    assert_eq!(journal::fnv1a32(b""), 0x811c_9dc5);
    assert_eq!(journal::fnv1a32(b"a"), 0xe40c_292c);
    assert_eq!(journal::fnv1a32(b"foobar"), 0xbf9c_f968);
}

/// The journal lines both languages must agree on byte-for-byte; the Node reader
/// decodes and re-frames the same file.
const CORPUS: &str =
    include_str!("../../../../packages/typescript-plugin/src/helpers/carrierJournalCorpus.txt");

fn corpus_records() -> Vec<journal::JournalRecord> {
    use journal::JournalOp::*;
    let project = || PROJECT.to_string();
    let row = OwnedSource {
        source_uri: "d:/ws/src/A.vue".into(),
        provider_uri: "d:/ws/src/A.vue.tsx".into(),
        role: ManifestRole::CarrierIde,
        script_kind: ManifestScriptKind::Tsx,
    };
    let file = ReadyFile {
        content_hash: "aaaa".into(),
        version: 3,
        script_kind: ManifestScriptKind::Tsx,
        role: ManifestRole::CarrierIde,
        map_hash: "bbbb".into(),
        blob_rel: "blobs/blake3-aaaa.tsx".into(),
        map_rel: Some("maps/blake3-bbbb.json".into()),
        structure: None,
    };
    vec![
        journal::JournalRecord {
            epoch: 8,
            ops: vec![
                ProjectPut { project: project() },
                OwnedClear { project: project() },
                OwnedPut {
                    project: project(),
                    source_uri: row.source_uri.clone(),
                    rows: vec![row.clone()],
                },
                ReadyPut {
                    project: project(),
                    provider_uri: row.provider_uri.clone(),
                    file,
                },
            ],
        },
        journal::JournalRecord {
            epoch: 9,
            ops: vec![
                ReadyDel {
                    project: project(),
                    provider_uri: row.provider_uri.clone(),
                },
                OwnedDel {
                    project: project(),
                    source_uri: row.source_uri.clone(),
                },
            ],
        },
    ]
}

#[test]
fn the_shared_journal_corpus_is_what_the_rust_writer_frames_and_reads() {
    let framed: Vec<u8> = corpus_records()
        .iter()
        .flat_map(|r| journal::frame_record(r).expect("frame"))
        .collect();
    assert_eq!(
        framed,
        CORPUS.as_bytes(),
        "the Rust writer's bytes drifted from the corpus the Node reader decodes"
    );
    let decoded: Vec<_> = CORPUS
        .lines()
        .map(|l| journal::decode_line(l.as_bytes()).expect("corpus line verifies"))
        .collect();
    assert_eq!(decoded, corpus_records());
}

#[test]
fn a_complete_invalid_last_line_is_dropped_and_the_next_publish_rebuilds_on_the_last_good_prefix() {
    let (store, ws, _ut) = fresh();
    publish(&store, &ws, 0, 0).expect("publish 0");
    publish(&store, &ws, 1, 0).expect("publish 1");
    let journal_path = store
        .workspace_dir()
        .join(journal::journal_file(head_generation(&store)));
    let mut bytes = std::fs::read(&journal_path).expect("journal");
    // Corrupt the LAST record's JSON but keep its trailing newline as the journal's
    // final byte: a complete line that fails verification with nothing after it is
    // a torn tail (its ops are dropped), not fail-closed corruption.
    let last_start = bytes[..bytes.len() - 1]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    bytes[last_start + 12] ^= 0x01;
    std::fs::write(&journal_path, &bytes).expect("corrupt");

    assert_eq!(membership(&read_strict(&store)), expect(&[0]));
    // A restarted writer folds the last-good prefix, truncates the torn tail and
    // appends on top of it: publication 1's ops are gone, not deferred.
    let restarted = CarrierPublishStore::open(HOST_VERSION, &ws);
    publish(&restarted, &ws, 2, 0).expect("the next publish truncates the torn tail");
    assert_eq!(membership(&read_strict(&restarted)), expect(&[0, 2]));
}
