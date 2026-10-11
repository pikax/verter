use super::observe;

#[test]
fn deep_answers_decode_render_and_drop_on_a_small_stack() {
    let depth = 1_000;
    let wire = format!(
        "{}{{\"kind\":\"primitive\",\"name\":\"string\"}}{}",
        "{\"kind\":\"array\",\"element\":".repeat(depth),
        "}".repeat(depth)
    );
    let observed = std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || observe(Ok(wire.as_bytes())))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(observed.error, None);
    assert_eq!(
        observed.text,
        Some(format!(
            "{}string{}",
            "Array<".repeat(depth),
            ">".repeat(depth)
        ))
    );
    assert_eq!(observed.unknown_leaves, 0);
}

#[test]
fn malformed_deep_answers_and_trailing_data_are_errors() {
    let deep = format!("{}0{}", "[".repeat(1_000), "]".repeat(999));
    for wire in [deep, r#"{"kind":"primitive","name":"string"} false"#.into()] {
        let observed = observe(Ok(wire.as_bytes()));
        assert!(observed
            .error
            .as_ref()
            .unwrap()
            .starts_with("decode the answer:"));
        assert_eq!(observed.text, None);
    }
}
