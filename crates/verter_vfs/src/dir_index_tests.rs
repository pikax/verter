use super::DirIndex;

#[test]
fn file_exists_returns_none_for_unindexed_or_dirty_directories() {
    let mut index = DirIndex::new();

    assert_eq!(index.file_exists("/workspace/src/App.vue"), None);

    index.refresh("/workspace/src", vec!["App.vue".to_string()]);
    assert_eq!(index.file_exists("/workspace/src/App.vue"), Some(true));

    index.mark_dirty("/workspace/src");
    assert_eq!(index.file_exists("/workspace/src/App.vue"), None);
}

#[test]
fn refresh_and_mark_dirty_under_track_directory_membership() {
    let mut index = DirIndex::new();
    index.refresh(
        "/workspace/src",
        vec!["App.vue".to_string(), "types.ts".to_string()],
    );
    index.refresh("/workspace/src/nested", vec!["Child.vue".to_string()]);

    assert_eq!(index.file_exists("/workspace/src/App.vue"), Some(true));
    assert_eq!(index.file_exists("/workspace/src/missing.vue"), Some(false));
    assert_eq!(
        index.file_exists("/workspace/src/nested/Child.vue"),
        Some(true)
    );

    index.mark_dirty_under("/workspace/src");

    assert_eq!(index.file_exists("/workspace/src/App.vue"), None);
    assert_eq!(index.file_exists("/workspace/src/nested/Child.vue"), None);
}
