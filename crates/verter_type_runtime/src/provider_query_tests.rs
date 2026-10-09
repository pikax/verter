use super::*;

fn bytes(text: &str) -> Arc<str> {
    Arc::from(text)
}

#[test]
fn a_query_keeps_the_surface_its_frame_met_after_later_deliveries() {
    let ledger = DeliveryLedger::default();
    ledger.deliver_with(
        [
            SurfaceEffect::deliver("/ws/a.ts", bytes("a1")),
            SurfaceEffect::deliver("/ws/b.ts", bytes("b1")),
        ],
        || (),
    );
    let (query, converted) = ledger
        .dispatch_with("/ws/a.ts", None, |requested| {
            Ok::<_, ()>(requested.map(str::to_string))
        })
        .unwrap();
    assert_eq!(converted.as_deref(), Some("a1"));

    ledger.deliver_with(
        [
            SurfaceEffect::deliver("/ws/a.ts", bytes("a2")),
            SurfaceEffect::withdraw("/ws/b.ts"),
        ],
        || (),
    );

    assert_eq!(query.requested().map(|b| &**b), Some("a1"));
    assert_eq!(query.delivered("/ws/b.ts").map(|b| &**b), Some("b1"));
    let targeted = query.targeted(&HashSet::from([
        "/ws/a.ts".to_string(),
        "/ws/b.ts".to_string(),
        "/ws/disk.ts".to_string(),
    ]));
    assert_eq!(
        targeted.len(),
        2,
        "a disk-read target is left to the decoder"
    );
    assert_eq!(&*targeted["/ws/a.ts"], "a1");
}

#[test]
fn an_undelivered_request_retains_its_fallback_bytes() {
    let ledger = DeliveryLedger::default();
    let (query, ()) = ledger
        .dispatch_with("/ws/disk.ts", Some(bytes("disk")), |requested| {
            assert_eq!(requested, Some("disk"));
            Ok::<_, ()>(())
        })
        .unwrap();
    assert_eq!(query.delivered("/ws/disk.ts").map(|b| &**b), Some("disk"));
}

#[test]
fn binding_after_the_requested_file_was_replaced_is_a_conflict() {
    let ledger = DeliveryLedger::default();
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a1")));
    let converted = ledger.requested("/ws/a.ts", None);
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a1")));
    assert_eq!(
        ledger.bind("/ws/a.ts", converted).unwrap_err().path(),
        "/ws/a.ts",
        "an identical re-delivery is still a different delivery"
    );

    let converted = ledger.requested("/ws/b.ts", Some(bytes("disk")));
    ledger.record_wire(SurfaceEffect::deliver("/ws/b.ts", bytes("b1")));
    assert!(
        ledger.bind("/ws/b.ts", converted).is_err(),
        "a first delivery after a disk conversion moves the requested bytes"
    );

    let converted = ledger.requested("/ws/a.ts", None);
    ledger.record_wire(SurfaceEffect::deliver("/ws/other.ts", bytes("x")));
    let query = ledger
        .bind("/ws/a.ts", converted)
        .expect("an unrelated delivery never conflicts");
    assert_eq!(query.delivered("/ws/other.ts").map(|b| &**b), Some("x"));
}

#[test]
fn settling_rechecks_only_out_of_band_files_the_answer_decoded_through() {
    let ledger = DeliveryLedger::default();
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/App.vue.tsx", bytes("c1"))]);
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/Other.vue.tsx", bytes("o1"))]);
    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a1")));
    let (query, ()) = ledger
        .dispatch_with("/ws/a.ts", None, |_| Ok::<_, ()>(()))
        .unwrap();

    ledger.record_wire(SurfaceEffect::deliver("/ws/a.ts", bytes("a2")));
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/Other.vue.tsx", bytes("o2"))]);
    ledger
        .settle(&query, ["/ws/a.ts", "/ws/App.vue.tsx"])
        .expect("wire-ordered and unmoved out-of-band files settle");
    assert_eq!(
        ledger
            .settle(&query, ["/ws/Other.vue.tsx"])
            .unwrap_err()
            .path(),
        "/ws/Other.vue.tsx"
    );

    let (carrier_query, ()) = ledger
        .dispatch_with("/ws/App.vue.tsx", None, |_| Ok::<_, ()>(()))
        .unwrap();
    ledger.record_out_of_band([SurfaceEffect::withdraw("/ws/App.vue.tsx")]);
    assert!(
        ledger.settle(&carrier_query, []).is_err(),
        "an out-of-band requested file is settled even when no range names it"
    );
}

#[test]
fn out_of_band_records_never_displace_a_protocol_buffer_and_dedupe_identical_bytes() {
    let ledger = DeliveryLedger::default();
    ledger.record_wire(SurfaceEffect::deliver("/ws/open.ts", bytes("buffer")));
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/open.ts", bytes("disk"))]);
    ledger.record_out_of_band([SurfaceEffect::withdraw("/ws/open.ts")]);
    let (query, ()) = ledger
        .dispatch_with("/ws/open.ts", None, |_| Ok::<_, ()>(()))
        .unwrap();
    assert_eq!(query.requested().map(|b| &**b), Some("buffer"));

    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/loaded.ts", bytes("v1"))]);
    let converted = ledger.requested("/ws/loaded.ts", None);
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/loaded.ts", bytes("v1"))]);
    let query = ledger
        .bind("/ws/loaded.ts", converted)
        .expect("identical out-of-band bytes are the same delivery");
    ledger
        .settle(&query, [])
        .expect("an identical re-record settles");
    ledger.record_out_of_band([SurfaceEffect::deliver("/ws/loaded.ts", bytes("v2"))]);
    assert!(ledger.settle(&query, []).is_err());
}
