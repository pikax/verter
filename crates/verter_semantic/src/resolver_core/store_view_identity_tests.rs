use super::{StoreViewOverlayIdentity, StoreViewProjectIdentity, StoreViewValidationToken};

fn hash(byte: u8) -> [u8; 16] {
    [byte; 16]
}

fn project_identity(byte: u8) -> StoreViewProjectIdentity {
    StoreViewProjectIdentity(hash(byte))
}

fn token(store_view_epoch: u64) -> StoreViewValidationToken {
    StoreViewValidationToken::new(
        store_view_epoch,
        1,
        1,
        1,
        1,
        Some(1),
        1,
        hash(9),
        project_identity(9),
        None,
    )
}

#[test]
fn externally_superseded_by_detects_epoch_change() {
    let earlier = token(1);
    let later = token(2);
    // Discriminates: a comparison that ignored `store_view_epoch` would
    // report these two otherwise-identical tokens as not superseded.
    assert!(earlier.externally_superseded_by(&later));
    assert!(later.externally_superseded_by(&earlier));
    assert!(!earlier.externally_superseded_by(&earlier));
}

#[test]
fn externally_superseded_by_ignores_artifact_and_load_generation() {
    let base = token(5);
    let mut bumped = base;
    bumped.artifact_generation += 1;
    bumped.load_generation += 1;
    // Discriminates: a naive field-by-field != comparison across every
    // field would flag this as superseded; the whole point of this method
    // is that a cold compute's own artifact publications/loads must not
    // self-fence promotion.
    assert!(!base.externally_superseded_by(&bumped));
}

#[test]
fn externally_superseded_by_detects_project_identity_change() {
    let base = token(5);
    let mut different_identity = base;
    different_identity.project_identity = project_identity(200);
    assert!(base.externally_superseded_by(&different_identity));
}

#[test]
fn externally_superseded_by_detects_overlay_identity_change() {
    let base = token(5);
    let mut overlaid = base;
    overlaid.overlay_identity = Some(StoreViewOverlayIdentity {
        session_id: Some(42),
        overlay_fingerprint: hash(3),
    });
    assert!(base.externally_superseded_by(&overlaid));
}

#[test]
fn external_supersession_fingerprint_matches_iff_neither_supersedes() {
    let a = token(7);
    let b = token(7);
    assert_eq!(
        a.external_supersession_fingerprint(),
        b.external_supersession_fingerprint()
    );

    let mut c = a;
    c.store_view_epoch += 1;
    // Discriminates: a fingerprint that dropped `store_view_epoch` from the
    // fold would collide here despite `a` being superseded by `c`.
    assert_ne!(
        a.external_supersession_fingerprint(),
        c.external_supersession_fingerprint()
    );
}

#[test]
fn external_supersession_fingerprint_ignores_artifact_and_load_generation() {
    let a = token(7);
    let mut b = a;
    b.artifact_generation += 5;
    b.load_generation += 5;
    assert_eq!(
        a.external_supersession_fingerprint(),
        b.external_supersession_fingerprint()
    );
}

#[test]
fn lane_fingerprint_matches_external_supersession_fingerprint() {
    let t = token(11);
    assert_eq!(t.lane_fingerprint(), t.external_supersession_fingerprint());
}

#[test]
fn with_overlay_identity_replaces_only_that_dimension() {
    let base = token(3);
    assert!(base.overlay_identity.is_none());

    let overlaid = base.with_overlay_identity(Some(StoreViewOverlayIdentity {
        session_id: Some(1),
        overlay_fingerprint: hash(2),
    }));
    assert!(overlaid.overlay_identity.is_some());
    // Discriminates: a broken with_overlay_identity that also touched
    // store_view_epoch (or any other field) would fail this.
    assert_eq!(overlaid.store_view_epoch, base.store_view_epoch);
    assert_eq!(overlaid.project_identity, base.project_identity);

    let cleared = overlaid.with_overlay_identity(None);
    assert!(cleared.overlay_identity.is_none());
}
