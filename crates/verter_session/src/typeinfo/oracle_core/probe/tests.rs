//! Discriminating, offline (no-tsgo) self-tests for probe synthesis.

use super::*;

#[test]
fn probe_name_is_ordinal_keyed() {
    assert_eq!(probe_name(0), "__oracle_probe__0");
    assert_eq!(probe_name(7), "__oracle_probe__7");
    assert_ne!(probe_name(0), probe_name(1));
}

#[test]
fn empty_type_args_uses_bare_symbol_rhs() {
    assert_eq!(
        resolve_expr_probe_rhs("ComposedProps", &[]).unwrap(),
        "ComposedProps"
    );
}

#[test]
fn nonempty_type_args_defers_to_printer_spike() {
    // The parameterized RHS printer is NOT spiked yet, so a non-empty type_args
    // row must DEFER (stay Ignored), never best-effort print.
    let err = resolve_expr_probe_rhs("GenericBox", &["string".to_string()]).unwrap_err();
    assert_eq!(err, ProbeRhsError::ParameterizedTypeArgsDeferred);
}

#[test]
fn probe_header_names_the_probe() {
    assert_eq!(
        probe_header(0, "ComposedProps"),
        "type __oracle_probe__0 = ComposedProps;"
    );
    // The header is what `probe_header_names_target` re-checks offline — it must
    // contain the exact probe name and the alias `=`.
    let h = probe_header(3, "Foo");
    assert!(h.contains("__oracle_probe__3"));
    assert!(h.starts_with("type __oracle_probe__3 = "));
}

#[test]
fn append_probe_offset_lands_on_probe_name() {
    let base = "type ComposedProps = { id: number };";
    let synth = append_probe(base, 0, "ComposedProps");
    // The synthesized source contains the original plus the appended probe.
    assert!(synth.source.starts_with(base));
    assert!(synth
        .source
        .contains("type __oracle_probe__0 = ComposedProps;"));
    // The recorded offset points AT the probe name (so the hover lands on the
    // alias symbol, not on `type ` or whitespace).
    assert_eq!(
        &synth.source[synth.probe_name_offset..synth.probe_name_offset + "__oracle_probe__0".len()],
        "__oracle_probe__0"
    );
}

#[test]
fn append_probe_inserts_newline_when_base_lacks_one() {
    // base WITHOUT a trailing newline must not merge into the probe line.
    let base = "type A = number;"; // no trailing \n
    let synth = append_probe(base, 1, "A");
    assert!(synth
        .source
        .contains("number;\ntype __oracle_probe__1 = A;\n"));
    // base WITH a trailing newline must not double it.
    let synth2 = append_probe("type A = number;\n", 1, "A");
    assert!(!synth2.source.contains("\n\ntype __oracle_probe__1"));
}

#[test]
fn evaluate_expr_scratch_is_prelude_plus_probe() {
    // The EvaluateExpr scratch model = eval_source prelude + trailing probe; the
    // same `append_probe` with `base = prelude` produces it.
    let prelude = "const f = (x: number) => x;";
    let synth = append_probe(prelude, 2, "typeof f");
    assert!(synth.source.starts_with(prelude));
    assert!(synth.source.contains("type __oracle_probe__2 = typeof f;"));
}
