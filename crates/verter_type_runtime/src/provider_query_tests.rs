use super::*;

fn bytes(text: &str) -> Arc<str> {
    Arc::from(text)
}

#[test]
fn completed_delivery_evidence_keeps_targets_and_rejects_a_retired_ledger() {
    let query = at_engine("/ws/a.ts");
    let ledger = DeliveryLedger::default();
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a")));
    ledger.record_wire(SurfaceEffect::deliver("/ws/b.ts", bytes("b")));
    let bound = bind(&ledger, &query).expect("bind");
    bound.target("/ws/b.ts", "/ws/b.ts").expect("decode target");
    ledger.settle(&bound, ["/ws/b.ts"]).expect("complete");
    assert_eq!(query.decoded_target("/ws/b.ts").as_deref(), Some("b"));
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a")));
    assert!(
        query.result_is_current(),
        "identical delivery within the same engine stays live"
    );
    drop(ledger);
    let replacement = DeliveryLedger::default();
    replacement.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a")));
    replacement.record_wire(SurfaceEffect::deliver("/ws/b.ts", bytes("b")));
    assert!(
        !query.result_is_current(),
        "identical bytes do not resurrect a retired binding"
    );
    assert!(replacement.settle(&bound, ["/ws/b.ts"]).is_err());
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
        .dispatch_with(prepared, |requested| Ok::<_, ()>(requested.to_string()))
        .ok()
        .expect("bind");
    assert_eq!(converted, "a1");

    ledger.deliver_with(
        [
            SurfaceEffect::deliver("/ws/a.ts", bytes("a2")),
            SurfaceEffect::withdraw("/ws/b.ts"),
        ],
        || (),
    );

    assert_eq!(&**bound.requested(), "a1");
    assert_eq!(
        &*bound.target("/ws/b.ts", "/ws/b.ts").expect("wire target"),
        "b1"
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
    assert_eq!(&**bound.requested(), "A");
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
fn a_file_the_engine_was_never_handed_is_a_conflict_whatever_its_disk_holds() {
    // The engine reads such a file itself, with no wire position: a disk edit
    // it has not re-read, or a replacement that keeps the file's timestamp,
    // leaves nothing that identifies the bytes it evaluated, however stable
    // the file looks.
    let dir = tempfile::tempdir().expect("tempdir");
    let closed = dir.path().join("closed.ts");
    std::fs::write(&closed, "closed").expect("write fixture");
    let closed = closed.to_string_lossy().to_string();
    let ledger = DeliveryLedger::default();
    assert_eq!(
        bind(&ledger, &at_engine(&closed)).unwrap_err(),
        ConflictKind::Undelivered,
        "a request on a file the engine reads itself never converts against its disk bytes"
    );

    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a")));
    let bound = bind(&ledger, &at_engine("/ws/a.ts")).expect("bind");
    assert_eq!(
        bound.target(&closed, &closed).unwrap_err().kind(),
        ConflictKind::Undelivered,
        "a location in a file the engine read from disk never decodes"
    );
    let bundled = dir.path().join("lib.d.ts").to_string_lossy().to_string();
    assert_eq!(
        bound.target(&bundled, &bundled).unwrap_err().kind(),
        ConflictKind::Undelivered,
        "nor one in a library the engine reads from its own bundle"
    );

    // Delivered only after the query's frame: the engine evaluated whatever it
    // read before, so the binding still has no bytes for it.
    ledger.record_wire(SurfaceEffect::deliver(closed.clone(), bytes("closed")));
    assert_eq!(
        bound.target(&closed, &closed).unwrap_err().kind(),
        ConflictKind::Undelivered
    );
    let delivered = bind(&ledger, &at_engine(&closed)).expect("a delivered file binds");
    assert_eq!(&**delivered.requested(), "closed");
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
    assert_eq!(&**bound.requested(), "buffer");

    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/loaded.ts", bytes("v1"))]);
    let bound = bind(&ledger, &at_engine("/ws/loaded.ts")).expect("bind");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/loaded.ts", bytes("v1"))]);
    ledger
        .settle(&bound, [])
        .expect("an identical re-record settles");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/loaded.ts", bytes("v2"))]);
    assert!(ledger.settle(&bound, []).is_err());
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
    assert_eq!(&*bound.target("/ws/a.ts", "/ws/a.ts").expect("own"), "a");
}

/// Published rows: path → (publication epoch, bytes).
type PublishedRows = HashMap<String, (u64, Arc<str>)>;

#[derive(Default)]
struct PublisherState {
    epoch: u64,
    rows: PublishedRows,
}

/// A publisher whose record a test moves explicitly — another writer's
/// publication included.
#[derive(Default)]
struct Publisher {
    state: parking_lot::Mutex<PublisherState>,
}

fn store_position(epoch: u64) -> PublicationPosition {
    PublicationPosition {
        instance: Arc::from("store"),
        epoch,
    }
}

impl Publisher {
    fn publish(&self, path: &str, content: &str) {
        let mut state = self.state.lock();
        state.epoch += 1;
        let epoch = state.epoch;
        state.rows.insert(path.to_string(), (epoch, bytes(content)));
    }

    fn withdraw(&self, path: &str) {
        let mut state = self.state.lock();
        state.epoch += 1;
        state.rows.remove(path);
    }
}

impl SurfacePublications for Publisher {
    fn position(&self) -> Option<PublicationPosition> {
        Some(store_position(self.state.lock().epoch))
    }

    fn attest(&self, path: &str, content: &str) -> Attestation {
        let state = self.state.lock();
        match state.rows.get(path) {
            None => Attestation::Unpublished,
            Some((_, published)) if **published != *content => Attestation::Contradicted,
            Some((epoch, _)) => Attestation::Attested(store_position(*epoch)),
        }
    }
}

/// A ledger over `publisher`, constructed before the engine reads anything.
fn published_ledger(publisher: &Arc<Publisher>) -> DeliveryLedger {
    DeliveryLedger::new(Some(Arc::clone(publisher) as Arc<dyn SurfacePublications>))
}

/// The engine re-reads its publisher: everything published so far is what it
/// holds from the next frame on.
fn adopt(ledger: &DeliveryLedger) {
    ledger.adopt_with(ledger.publication_position(), || ());
}

#[test]
fn a_publication_by_another_writer_after_dispatch_is_a_conflict() {
    let publisher = Arc::new(Publisher::default());
    let ledger = published_ledger(&publisher);
    publisher.publish("/ws/App.vue.tsx", "A");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/App.vue.tsx", bytes("A"))]);
    adopt(&ledger);

    let bound = bind(&ledger, &at_engine("/ws/App.vue.tsx")).expect("bind");
    ledger
        .settle(&bound, [])
        .expect("published and adopted before dispatch");

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
    let ledger = published_ledger(&publisher);
    publisher.publish("/ws/App.vue.tsx", "A");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/App.vue.tsx", bytes("A"))]);
    adopt(&ledger);
    // The store already serves B; this process has not registered it yet.
    publisher.publish("/ws/App.vue.tsx", "B");
    assert_eq!(
        bind(&ledger, &at_engine("/ws/App.vue.tsx")).unwrap_err(),
        ConflictKind::Publication,
        "the publisher contradicts the bytes the request would convert against"
    );
}

#[test]
fn a_registered_publication_binds_only_once_the_engine_has_adopted_it() {
    let publisher = Arc::new(Publisher::default());
    let ledger = published_ledger(&publisher);
    publisher.publish("/ws/App.vue.tsx", "A");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/App.vue.tsx", bytes("A"))]);
    adopt(&ledger);
    let on_a = bind(&ledger, &at_engine("/ws/App.vue.tsx")).expect("A is adopted");

    // B is published and registered here, but the engine has not re-read its
    // publisher: it still evaluates A.
    publisher.publish("/ws/App.vue.tsx", "B");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/App.vue.tsx", bytes("B"))]);
    assert_eq!(
        bind(&ledger, &at_engine("/ws/App.vue.tsx")).unwrap_err(),
        ConflictKind::Publication,
        "a query converted against B would be answered from A"
    );
    assert!(
        ledger.settle(&on_a, []).is_err(),
        "A no longer holds either"
    );

    adopt(&ledger);
    let on_b = bind(&ledger, &at_engine("/ws/App.vue.tsx")).expect("B is adopted");
    assert_eq!(&**on_b.requested(), "B");
    ledger.settle(&on_b, []).expect("adopted before dispatch");
}

#[test]
fn a_publication_adopted_only_after_dispatch_is_a_conflict() {
    let publisher = Arc::new(Publisher::default());
    let ledger = published_ledger(&publisher);
    publisher.publish("/ws/B.vue.tsx", "B1");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/B.vue.tsx", bytes("B1"))]);
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a")));
    // The engine read its publisher before B1 existed and has not re-read it.
    let bound = bind(&ledger, &at_engine("/ws/a.ts")).expect("bind");
    adopt(&ledger);
    assert_eq!(
        ledger.settle(&bound, ["/ws/B.vue.tsx"]).unwrap_err().kind(),
        ConflictKind::Publication,
        "the engine adopted B1 only after this query's frame"
    );
}

#[test]
fn an_out_of_band_foreign_target_settles_against_its_own_publication() {
    let publisher = Arc::new(Publisher::default());
    let ledger = published_ledger(&publisher);
    publisher.publish("/ws/B.vue.tsx", "B1");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/B.vue.tsx", bytes("B1"))]);
    adopt(&ledger);
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
fn a_row_withdrawn_under_an_answer_is_a_conflict() {
    let publisher = Arc::new(Publisher::default());
    let ledger = published_ledger(&publisher);
    publisher.publish("/ws/App.vue.tsx", "A");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/App.vue.tsx", bytes("A"))]);
    adopt(&ledger);

    let unchanged = bind(&ledger, &at_engine("/ws/App.vue.tsx")).expect("bind");
    ledger
        .settle(&unchanged, [])
        .expect("an unchanged publication settles");

    let bound = bind(&ledger, &at_engine("/ws/App.vue.tsx")).expect("bind");
    // Another writer publishes B while the engine evaluates, then retracts the
    // row; this process never re-registers.
    publisher.publish("/ws/App.vue.tsx", "B");
    publisher.withdraw("/ws/App.vue.tsx");
    assert_eq!(
        ledger.settle(&bound, []).unwrap_err().kind(),
        ConflictKind::Publication,
        "the engine may have evaluated B, or reads the withdrawn file itself"
    );
}

#[test]
fn a_foreign_target_this_process_never_registered_is_a_conflict() {
    let publisher = Arc::new(Publisher::default());
    let ledger = published_ledger(&publisher);
    // Another LSP published the target; this process never registered it, so
    // nothing ties the engine's bytes for it to this query.
    publisher.publish("/ws/B.vue.tsx", "published B");
    adopt(&ledger);
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a")));
    let bound = bind(&ledger, &at_engine("/ws/a.ts")).expect("bind");
    assert_eq!(
        bound
            .target("/ws/B.vue.tsx", "/ws/B.vue.tsx")
            .unwrap_err()
            .kind(),
        ConflictKind::Undelivered
    );
}

#[test]
fn an_out_of_band_file_no_publisher_names_is_never_bound() {
    let publisher = Arc::new(Publisher::default());
    let ledger = published_ledger(&publisher);
    // Recorded out of band, but the publisher names no row for it: the engine
    // reads the file itself.
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/loaded.ts", bytes("loaded"))]);
    assert_eq!(
        bind(&ledger, &at_engine("/ws/loaded.ts")).unwrap_err(),
        ConflictKind::Publication
    );
}

#[test]
fn a_delivery_in_flight_binds_and_decodes_nothing_until_its_outcome_is_recorded() {
    let ledger = DeliveryLedger::default();
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/App.vue.tsx", bytes("A"))]);
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a")));
    let before = bind(&ledger, &at_engine("/ws/App.vue.tsx")).expect("bind");

    // B is being injected through a channel the ledger cannot order: the
    // engine may hold A or B until the injection is confirmed.
    ledger.record_out_of_band([SurfaceEffect::unsettle("/ws/App.vue.tsx")]);
    assert_eq!(
        bind(&ledger, &at_engine("/ws/App.vue.tsx")).unwrap_err(),
        ConflictKind::InFlight
    );
    assert_eq!(
        ledger.settle(&before, []).unwrap_err().kind(),
        ConflictKind::Moved,
        "an answer bound to A may have been evaluated over B"
    );
    let origin = bind(&ledger, &at_engine("/ws/a.ts")).expect("bind");
    assert_eq!(
        origin
            .target("/ws/App.vue.tsx", "/ws/App.vue.tsx")
            .unwrap_err()
            .kind(),
        ConflictKind::InFlight
    );

    // The injection is confirmed: B binds, even when it repeats the old bytes.
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/App.vue.tsx", bytes("A"))]);
    let after = bind(&ledger, &at_engine("/ws/App.vue.tsx")).expect("bind");
    assert_eq!(&**after.requested(), "A");
    ledger.settle(&after, []).expect("settles");
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
    let other = DeliveryLedger::default();
    other.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a")));
    assert_ne!(
        bound.engine(),
        bind(&other, &at_engine("/ws/a.ts")).expect("bind").engine(),
        "each engine incarnation's ledger is a distinct binding target"
    );
    assert!(requester.admission().incarnation.is_none());
}

#[test]
fn a_navigation_answer_drops_only_its_undelivered_targets() {
    let ledger = DeliveryLedger::default();
    ledger.deliver_with(
        [
            SurfaceEffect::deliver("/ws/a.ts", bytes("a")),
            SurfaceEffect::deliver("/ws/b.ts", bytes("b")),
        ],
        || (),
    );
    let bound = bind(&ledger, &at_engine("/ws/a.ts")).expect("bind");
    let paths = |list: &[&str]| -> HashSet<String> {
        list.iter().map(|path| (*path).to_string()).collect()
    };

    let mixed = paths(&["/ws/a.ts", "/ws/b.ts", "/lib/lib.dom.d.ts"]);
    let decoded = bound
        .navigation_targets(&mixed, str::to_string)
        .expect("delivered targets still decode");
    assert_eq!(
        decoded.len(),
        2,
        "only the library target drops: {decoded:?}"
    );
    assert_eq!(&*decoded["/ws/a.ts"], "a");
    assert_eq!(&*decoded["/ws/b.ts"], "b");
    assert_eq!(
        bound.targets(&mixed, str::to_string).unwrap_err().kind(),
        ConflictKind::Undelivered,
        "an edit answer stays whole or refused"
    );

    let only_undelivered = paths(&["/lib/lib.dom.d.ts", "/ws/closed.ts"]);
    assert_eq!(
        bound
            .navigation_targets(&only_undelivered, str::to_string)
            .unwrap_err()
            .kind(),
        ConflictKind::Undelivered,
        "an answer with nothing delivered is the typed conflict, never an empty success"
    );
    assert!(bound
        .navigation_targets(&HashSet::new(), str::to_string)
        .expect("no targets")
        .is_empty());
}

#[test]
fn a_navigation_answer_still_refuses_every_conflict_other_than_undelivered() {
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
    let targets: HashSet<String> = ["/ws/a.ts", "/ws/B.vue.tsx", "/lib/lib.dom.d.ts"]
        .into_iter()
        .map(str::to_string)
        .collect();
    assert_eq!(
        bound
            .navigation_targets(&targets, str::to_string)
            .unwrap_err()
            .kind(),
        ConflictKind::IntendedSurface,
        "a target decoded through other bytes than the requester maps it through \
         refuses the whole answer"
    );
}
