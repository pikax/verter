use super::*;
use std::time::Duration;

fn bytes(text: &str) -> Arc<str> {
    Arc::from(text)
}

const ID_A: DeliveredSurfaceId = DeliveredSurfaceId {
    generation: 1,
    content_epoch: 1,
    incarnation: 1,
};

/// Bind `query` on its own path, converting nothing; the frame is placed.
fn bind(ledger: &DeliveryLedger, query: &ProviderQuery) -> Result<BoundQuery, ConflictKind> {
    let prepared = ledger
        .prepare(query, query.path())
        .map_err(|conflict| conflict.kind())?;
    match ledger.dispatch_with(prepared, |_| Ok::<_, ()>(())) {
        Ok((bound, ())) => Ok(bound),
        Err(DispatchRefusal::Conflict(conflict)) => Err(conflict.kind()),
        Err(DispatchRefusal::Unplaced(())) => panic!("the frame builder never declines here"),
    }
}

fn at_engine(path: &str) -> ProviderQuery {
    ProviderQuery::at_engine_surface(path)
}

/// A file whose modification time is pinned `offset` away from now, so a
/// dispatch taken now is provably before or after its last write.
fn disk_file(dir: &tempfile::TempDir, name: &str, content: &str, offset: i64) -> String {
    let path = dir.path().join(name);
    std::fs::write(&path, content).expect("write fixture");
    let now = SystemTime::now();
    let modified = if offset < 0 {
        now - Duration::from_secs(offset.unsigned_abs())
    } else {
        now + Duration::from_secs(offset.unsigned_abs())
    };
    std::fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_modified(modified))
        .expect("pin modification time");
    path.to_string_lossy().to_string()
}

#[test]
fn a_binding_keeps_the_surface_its_frame_met_after_later_deliveries() {
    let ledger = DeliveryLedger::default();
    ledger.deliver_with(
        [
            SurfaceEffect::deliver("/ws/a.ts", bytes("a1")),
            SurfaceEffect::deliver("/ws/b.ts", bytes("b1")),
        ],
        || (),
    );
    let query = at_engine("/ws/a.ts");
    let prepared = ledger.prepare(&query, "/ws/a.ts").expect("prepare");
    let (bound, converted) = ledger
        .dispatch_with(prepared, |requested| {
            Ok::<_, ()>(requested.map(str::to_string))
        })
        .ok()
        .expect("bind");
    assert_eq!(converted.as_deref(), Some("a1"));

    ledger.deliver_with(
        [
            SurfaceEffect::deliver("/ws/a.ts", bytes("a2")),
            SurfaceEffect::withdraw("/ws/b.ts"),
        ],
        || (),
    );

    assert_eq!(bound.requested().map(|b| &**b), Some("a1"));
    assert_eq!(
        bound
            .target("/ws/b.ts", "/ws/b.ts")
            .expect("wire target")
            .as_deref(),
        Some("b1")
    );
}

#[test]
fn a_query_bound_to_other_bytes_than_it_intends_is_refused_before_dispatch() {
    let ledger = DeliveryLedger::default();
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("A")));
    // The requester captured A; B reaches the engine before the frame.
    let query = ProviderQuery::intending("/ws/a.ts", ID_A, bytes("A"));
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("B")));
    let prepared = ledger.prepare(&query, "/ws/a.ts").expect("prepare");
    let mut placed = false;
    let refused = ledger.dispatch_with(prepared, |_| {
        placed = true;
        Ok::<_, ()>(())
    });
    assert!(matches!(
        refused,
        Err(DispatchRefusal::Conflict(ref conflict)) if conflict.kind() == ConflictKind::IntendedSurface
    ));
    assert!(!placed, "A's position never reaches an engine holding B");

    // A delivered back before dispatch is exactly what the requester intends.
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("A")));
    let bound = bind(&ledger, &query).expect("A again binds");
    assert_eq!(bound.requested().map(|b| &**b), Some("A"));
    assert_eq!(
        bound.query().intended().map(IntendedSurface::id),
        Some(ID_A)
    );
}

#[test]
fn an_identical_re_delivery_before_dispatch_still_binds() {
    let ledger = DeliveryLedger::default();
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("A")));
    let query = ProviderQuery::intending("/ws/a.ts", ID_A, bytes("A"));
    let prepared = ledger.prepare(&query, "/ws/a.ts").expect("prepare");
    // A replay of the same bytes reaches the writer between preparation and
    // placement.
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("A")));
    let (bound, ()) = ledger
        .dispatch_with(prepared, |_| Ok::<_, ()>(()))
        .ok()
        .expect("the bytes the engine holds are the ones intended");
    ledger.settle(&bound, []).expect("wire bytes settle");
}

#[test]
fn an_undelivered_request_retains_its_disk_bytes_and_settles_against_them() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = disk_file(&dir, "disk.ts", "disk", -60);
    let ledger = DeliveryLedger::default();
    let bound = bind(&ledger, &at_engine(&path)).expect("bind");
    assert_eq!(bound.requested().map(|b| &**b), Some("disk"));
    ledger
        .settle(&bound, [])
        .expect("unchanged disk bytes settle");

    std::fs::write(&path, "rewritten").expect("rewrite");
    assert_eq!(
        ledger.settle(&bound, []).unwrap_err().kind(),
        ConflictKind::Disk,
        "the engine may have read the rewritten bytes"
    );
}

#[test]
fn settling_rechecks_only_out_of_band_files_the_answer_decoded_through() {
    let ledger = DeliveryLedger::default();
    ledger.record_out_of_band([
        SurfaceEffect::deliver("/ws/App.vue.tsx", bytes("c1")),
        SurfaceEffect::deliver("/ws/Other.vue.tsx", bytes("o1")),
    ]);
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a1")));
    let bound = bind(&ledger, &at_engine("/ws/a.ts")).expect("bind");

    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a2")));
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/Other.vue.tsx", bytes("o2"))]);
    ledger
        .settle(&bound, ["/ws/a.ts", "/ws/App.vue.tsx"])
        .expect("wire-ordered and unmoved out-of-band files settle");
    let conflict = ledger.settle(&bound, ["/ws/Other.vue.tsx"]).unwrap_err();
    assert_eq!(
        (conflict.path(), conflict.kind()),
        ("/ws/Other.vue.tsx", ConflictKind::Moved)
    );

    let carrier = bind(&ledger, &at_engine("/ws/App.vue.tsx")).expect("bind");
    ledger.record_out_of_band([SurfaceEffect::withdraw("/ws/App.vue.tsx")]);
    assert!(
        ledger.settle(&carrier, []).is_err(),
        "an out-of-band requested file is settled even when no range names it"
    );
}

#[test]
fn out_of_band_records_never_displace_a_protocol_buffer_and_dedupe_identical_bytes() {
    let ledger = DeliveryLedger::default();
    ledger.record_wire(SurfaceEffect::deliver("/ws/open.ts", bytes("buffer")));
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/open.ts", bytes("disk"))]);
    ledger.record_out_of_band([SurfaceEffect::withdraw("/ws/open.ts")]);
    let bound = bind(&ledger, &at_engine("/ws/open.ts")).expect("bind");
    assert_eq!(bound.requested().map(|b| &**b), Some("buffer"));

    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/loaded.ts", bytes("v1"))]);
    let bound = bind(&ledger, &at_engine("/ws/loaded.ts")).expect("bind");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/loaded.ts", bytes("v1"))]);
    ledger
        .settle(&bound, [])
        .expect("an identical re-record settles");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/loaded.ts", bytes("v2"))]);
    assert!(ledger.settle(&bound, []).is_err());
}

#[test]
fn a_foreign_disk_target_decodes_only_when_unmodified_since_dispatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stable = disk_file(&dir, "stable.ts", "stable", -60);
    let rewritten = disk_file(&dir, "rewritten.ts", "after", 60);
    let ledger = DeliveryLedger::default();
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a")));
    let bound = bind(&ledger, &at_engine("/ws/a.ts")).expect("bind");

    assert_eq!(
        bound.target(&stable, &stable).expect("stable").as_deref(),
        Some("stable")
    );
    assert_eq!(
        bound.target(&rewritten, &rewritten).unwrap_err().kind(),
        ConflictKind::Disk,
        "bytes written after dispatch are not the ones the engine evaluated"
    );
    let gone = dir.path().join("gone.ts").to_string_lossy().to_string();
    assert_eq!(
        bound.target(&gone, &gone).expect("missing"),
        None,
        "a target with no bytes has no decodable location"
    );
}

struct Captured(HashMap<String, Arc<str>>);

impl IntendedTargets for Captured {
    fn intended(&self, path: &str) -> Option<Arc<str>> {
        self.0.get(path).cloned()
    }
}

#[test]
fn a_foreign_target_decodes_only_through_the_surface_the_requester_maps_it_through() {
    let ledger = DeliveryLedger::default();
    ledger.deliver_with(
        [
            SurfaceEffect::deliver("/ws/a.ts", bytes("a")),
            SurfaceEffect::deliver("/ws/B.vue.tsx", bytes("B2")),
        ],
        || (),
    );
    let query = at_engine("/ws/a.ts").with_targets(Arc::new(Captured(HashMap::from([(
        "/ws/B.vue.tsx".to_string(),
        bytes("B1"),
    )]))));
    let bound = bind(&ledger, &query).expect("bind");
    assert_eq!(
        bound
            .target("/ws/B.vue.tsx", "/ws/B.vue.tsx")
            .unwrap_err()
            .kind(),
        ConflictKind::IntendedSurface,
        "the requester would map the engine's B2 offsets through its captured B1"
    );
    assert_eq!(
        bound
            .target("/ws/a.ts", "/ws/a.ts")
            .expect("own")
            .as_deref(),
        Some("a")
    );
}

/// Published rows: path → (publication epoch, bytes).
type PublishedRows = HashMap<String, (u64, Arc<str>)>;

/// A publisher whose record a test moves explicitly — another writer's
/// publication included.
#[derive(Default)]
struct Publisher {
    rows: parking_lot::Mutex<(u64, PublishedRows)>,
}

impl Publisher {
    fn publish(&self, path: &str, content: &str) {
        let mut rows = self.rows.lock();
        rows.0 += 1;
        let epoch = rows.0;
        rows.1.insert(path.to_string(), (epoch, bytes(content)));
    }
}

impl SurfacePublications for Publisher {
    fn position(&self) -> Option<PublicationPosition> {
        Some(PublicationPosition {
            instance: Arc::from("store"),
            epoch: self.rows.lock().0,
        })
    }

    fn attest(&self, path: &str, content: &str) -> Attestation {
        match self.rows.lock().1.get(path) {
            None => Attestation::Unpublished,
            Some((_, published)) if **published != *content => Attestation::Contradicted,
            Some((epoch, _)) => Attestation::Attested(PublicationPosition {
                instance: Arc::from("store"),
                epoch: *epoch,
            }),
        }
    }
}

#[test]
fn a_publication_by_another_writer_after_dispatch_is_a_conflict() {
    let publisher = Arc::new(Publisher::default());
    let ledger = DeliveryLedger::new(Some(publisher.clone() as Arc<dyn SurfacePublications>));
    publisher.publish("/ws/App.vue.tsx", "A");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/App.vue.tsx", bytes("A"))]);

    let bound = bind(&ledger, &at_engine("/ws/App.vue.tsx")).expect("bind");
    ledger
        .settle(&bound, [])
        .expect("published before dispatch");

    // Another process publishes B; this process never re-registers.
    publisher.publish("/ws/App.vue.tsx", "B");
    assert_eq!(
        ledger.settle(&bound, []).unwrap_err().kind(),
        ConflictKind::Publication
    );
    // ...and A again: the row is A, but published after dispatch.
    publisher.publish("/ws/App.vue.tsx", "A");
    assert_eq!(
        ledger.settle(&bound, []).unwrap_err().kind(),
        ConflictKind::Publication,
        "an A→B→A publication is not the publication the engine was dispatched under"
    );
}

#[test]
fn a_publication_racing_ahead_of_registration_refuses_the_query() {
    let publisher = Arc::new(Publisher::default());
    let ledger = DeliveryLedger::new(Some(publisher.clone() as Arc<dyn SurfacePublications>));
    publisher.publish("/ws/App.vue.tsx", "A");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/App.vue.tsx", bytes("A"))]);
    // The store already serves B; this process has not registered it yet.
    publisher.publish("/ws/App.vue.tsx", "B");
    assert_eq!(
        bind(&ledger, &at_engine("/ws/App.vue.tsx")).unwrap_err(),
        ConflictKind::Publication,
        "the publisher contradicts the bytes the request would convert against"
    );
}

#[test]
fn an_out_of_band_foreign_target_settles_against_its_own_publication() {
    let publisher = Arc::new(Publisher::default());
    let ledger = DeliveryLedger::new(Some(publisher.clone() as Arc<dyn SurfacePublications>));
    publisher.publish("/ws/B.vue.tsx", "B1");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/B.vue.tsx", bytes("B1"))]);
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a")));
    let bound = bind(&ledger, &at_engine("/ws/a.ts")).expect("bind");
    ledger
        .settle(&bound, ["/ws/B.vue.tsx"])
        .expect("the target was published before dispatch");
    publisher.publish("/ws/B.vue.tsx", "B2");
    assert_eq!(
        ledger.settle(&bound, ["/ws/B.vue.tsx"]).unwrap_err().path(),
        "/ws/B.vue.tsx"
    );
    ledger
        .settle(&bound, [])
        .expect("a target the answer did not decode through is not settled");
}

#[test]
fn an_unpublished_out_of_band_file_is_evidenced_by_its_disk_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = disk_file(&dir, "loaded.ts", "loaded", -60);
    let publisher = Arc::new(Publisher::default());
    let ledger = DeliveryLedger::new(Some(publisher as Arc<dyn SurfacePublications>));
    ledger.record_out_of_band([SurfaceEffect::deliver(path.clone(), bytes("loaded"))]);
    let bound = bind(&ledger, &at_engine(&path)).expect("bind");
    ledger
        .settle(&bound, [])
        .expect("disk holds the loaded bytes");

    let virtual_path = dir
        .path()
        .join("Virtual.vue.tsx")
        .to_string_lossy()
        .to_string();
    ledger.record_out_of_band([SurfaceEffect::deliver(virtual_path.clone(), bytes("v"))]);
    let bound = bind(&ledger, &at_engine(&virtual_path)).expect("bind");
    assert_eq!(
        ledger.settle(&bound, []).unwrap_err().kind(),
        ConflictKind::Disk,
        "bytes no publisher names and no disk holds were never evidenced"
    );
}

#[test]
fn the_binding_carries_the_admission_the_query_was_dispatched_under() {
    let ledger = DeliveryLedger::default();
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a")));
    let requester = at_engine("/ws/a.ts");
    let admitted = requester
        .admitted_into(Arc::from("/ws/tsconfig.json"))
        .admitted_to(verter_identity::identity::ProviderEpoch(7));
    let bound = bind(&ledger, &admitted).expect("bind");
    assert_eq!(
        bound.query().admission(),
        &QueryAdmission {
            incarnation: Some(verter_identity::identity::ProviderEpoch(7)),
            project: Some(Arc::from("/ws/tsconfig.json")),
        }
    );
    assert_ne!(
        bound.engine(),
        bind(&DeliveryLedger::default(), &at_engine("/ws/a.ts"))
            .map(|bound| bound.engine())
            .unwrap_or_default(),
        "each engine incarnation's ledger is a distinct binding target"
    );
    assert!(requester.admission().incarnation.is_none());
}
