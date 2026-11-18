use super::PropsObject;

#[test]
fn duplicate_key_merge_handles_multiple_interleaved_groups() {
    // Two duplicate key groups, the second one's members sitting after the
    // first one's: collapsing the first group must not disturb the second.
    let mut props = PropsObject::new();
    props.push("\"onUpdate:a\"", "$event => (a = $event)");
    props.push("b", "val_b");
    props.push("\"onUpdate:a\"", "$event => (a2 = $event)");
    props.push("c", "val_c");
    props.push("\"onUpdate:d\"", "$event => (d = $event)");
    props.push("e", "val_e");
    props.push("f", "val_f");
    props.push("\"onUpdate:d\"", "$event => (d2 = $event)");

    props.merge_duplicate_keys();

    // Both groups collapse in place, each keeping its first occurrence's
    // position and both values in authoring order.
    assert_eq!(
        props.render_body(),
        concat!(
            r#""onUpdate:a": [$event => (a = $event), $event => (a2 = $event)], "#,
            "b: val_b, c: val_c, ",
            r#""onUpdate:d": [$event => (d = $event), $event => (d2 = $event)], "#,
            "e: val_e, f: val_f"
        )
    );
}

#[test]
fn duplicate_key_merge_distinguishes_quoted_from_bare_keys() {
    // `"foo"` and `foo` are emitted as authored; merging them would rewrite a
    // key the author spelled two different ways into one entry.
    let mut props = PropsObject::new();
    props.push("\"foo\"", "1");
    props.push("foo", "2");
    props.merge_duplicate_keys();

    assert_eq!(props.render_body(), r#""foo": 1, foo: 2"#);
}

#[test]
fn reserved_slot_never_filled_renders_nothing() {
    // `class`/`style` reserve a position before their merged value is known;
    // an abandoned reservation must not leave a stray separator behind.
    let mut props = PropsObject::new();
    props.push("a", "1");
    let _abandoned = props.reserve();
    props.push("b", "2");

    assert_eq!(props.render_body(), "a: 1, b: 2");
}
