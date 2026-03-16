use super::*;

#[test]
fn get_returns_none_for_unknown_file() {
    let store = OverlayStore::new();
    assert!(
        store.get("src/foo.vue").is_none(),
        "unknown file should return None"
    );
    assert!(
        !store.has_overlay("src/foo.vue"),
        "unknown file should not have overlay"
    );
}

#[test]
fn set_returns_overlay_content() {
    let mut store = OverlayStore::new();
    let content: Arc<str> = Arc::from("<template>hello</template>");
    store.set("src/foo.vue".to_string(), content.clone());

    let result = store.get("src/foo.vue");
    assert_eq!(result.as_deref(), Some("<template>hello</template>"));
    assert!(store.has_overlay("src/foo.vue"));
}

#[test]
fn clear_removes_overlay() {
    let mut store = OverlayStore::new();
    store.set("src/foo.vue".to_string(), Arc::from("content"));

    assert!(
        store.clear("src/foo.vue"),
        "clear should return true when overlay existed"
    );
    assert!(
        store.get("src/foo.vue").is_none(),
        "cleared overlay should return None"
    );
    assert!(
        !store.has_overlay("src/foo.vue"),
        "cleared overlay should not exist"
    );
}

#[test]
fn clear_returns_false_for_unknown() {
    let mut store = OverlayStore::new();
    assert!(
        !store.clear("src/foo.vue"),
        "clear on unknown should return false"
    );
}

#[test]
fn set_overwrites_existing_overlay() {
    let mut store = OverlayStore::new();
    store.set("src/foo.vue".to_string(), Arc::from("old content"));
    store.set("src/foo.vue".to_string(), Arc::from("new content"));

    assert_eq!(store.get("src/foo.vue").as_deref(), Some("new content"));
    assert_ne!(
        store.get("src/foo.vue").as_deref(),
        Some("old content"),
        "old content must not survive overwrite"
    );
    assert_eq!(store.len(), 1, "overwrite should not increase count");
}

#[test]
fn multiple_overlays_independent() {
    let mut store = OverlayStore::new();
    store.set("src/a.vue".to_string(), Arc::from("content a"));
    store.set("src/b.vue".to_string(), Arc::from("content b"));

    assert_eq!(store.len(), 2);
    assert_eq!(store.get("src/a.vue").as_deref(), Some("content a"));
    assert_eq!(store.get("src/b.vue").as_deref(), Some("content b"));

    store.clear("src/a.vue");
    assert!(store.get("src/a.vue").is_none(), "cleared a should be gone");
    assert_eq!(
        store.get("src/b.vue").as_deref(),
        Some("content b"),
        "b should survive"
    );
    assert_eq!(store.len(), 1);
}

#[test]
fn empty_store_properties() {
    let store = OverlayStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
}
