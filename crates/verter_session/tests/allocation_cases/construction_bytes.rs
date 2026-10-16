//! Construction bytes against the allocator: a connected demand reserves
//! the bytes its constructions allocate before it builds them, so what the
//! allocator hands out on the demand's thread stays within what the ledger
//! charged, plus the fixed cost of setting the demand up.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use super::{peak_live_bytes, reset_alloc_counter};

const FILE: &str = "/construction_bytes.ts";

/// `S` a union of `count` single-property objects, `T` the same objects
/// written in reverse: each arm of `S` scans `T` up to its partner.
fn reversed_unions(count: usize) -> String {
    let source: Vec<String> = (0..count).map(|i| format!("{{ p{i}: {i} }}")).collect();
    let target: Vec<String> = (0..count)
        .rev()
        .map(|i| format!("{{ p{i}: number }}"))
        .collect();
    format!(
        "export type S = {};\nexport type T = {};\n",
        source.join(" | "),
        target.join(" | ")
    )
}

fn host_with(source: &str) -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: false,
            footprint_capture: false,
            ..HostConfig::default()
        },
        workspace,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(FILE.into()),
        input_id: FILE.into(),
        source: Arc::from(source),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    host
}

#[test]
fn repeated_callbacks_allocate_in_the_linear_class() {
    let mut measurements = Vec::new();
    for count in [10, 100, 128, 256, 500, 512] {
        let parameters = (0..count)
            .map(|i| format!("f{i}: (x: T) => void"))
            .collect::<Vec<_>>()
            .join(", ");
        let arguments = vec![r#"(x: "a") => {}"#; count].join(", ");
        let source = format!(
            "type Init = 0; declare function co<T>({parameters}): T; const r = co({arguments}); export type Result = typeof r;"
        );
        let host = host_with(&source);
        let (init, _) = host
            .resolve_named_symbol_with_audit(FILE, "Init", None)
            .into_parts();
        assert!(init.unwrap().is_some());
        reset_alloc_counter();
        let (answer, _) = host
            .resolve_named_symbol_with_audit(FILE, "Result", None)
            .into_parts();
        let bytes = super::alloc_bytes();
        let peak = peak_live_bytes();
        let answer = answer.unwrap().expect("the callback result resolves");
        let json = host.project_node_to_type_expr_json_bytes(&answer).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
        let expression = verter_type_expr::type_expr_from_json(&value).unwrap();
        assert_eq!(
            expression,
            verter_type_expr::TypeExpr::Literal(verter_type_expr::LiteralValue::String("a".into()))
        );
        measurements.push((bytes, peak));
    }
    eprintln!("callback (allocated bytes, peak live bytes): {measurements:?}");
    for adjacent in [
        [measurements[2], measurements[3]],
        [measurements[3], measurements[5]],
    ] {
        assert!(adjacent[0].0 > 0 && adjacent[1].0 > adjacent[0].0);
        assert!(
            adjacent[1].0 < 3 * adjacent[0].0,
            "doubling callbacks allocates a quadratic volume: {measurements:?}"
        );
    }
}

/// The fixed cost of setting one connected demand up beside what it
/// constructs: lowering the two names and opening the relation.
const DEMAND_SETUP_BYTES: i64 = 4 << 20;

/// Relate `S` to `T` over `count` arms each, with the declarations already
/// read, under `limit` construction bytes when given: whether `S` holds, the
/// bytes the demand charged, and the peak live bytes the allocator handed
/// out on this thread meanwhile.
fn relate_measured(count: usize, limit: Option<u64>) -> (bool, u64, i64) {
    let host = host_with(&reversed_unions(count));
    let _ = verter_session::for_tests::relate_named_types_for_tests(&host, FILE, "S", "S", None);
    let _ = verter_session::for_tests::relate_named_types_for_tests(&host, FILE, "T", "T", None);
    reset_alloc_counter();
    let (assignable, _work, charged) =
        verter_session::for_tests::relate_named_types_for_tests(&host, FILE, "S", "T", limit);
    (assignable, charged, peak_live_bytes())
}

/// Two hundred object arms against the same arms reversed relate about
/// 20,100 structured pairs, each reserving its bytes before it is related:
/// the peak the relation holds live stays within what it charged.
///
/// Measured on TypeScript 7.0.2 (all four settings agree): `[S] extends [T] ?
/// 1 : 2` is `1` over 200 reversed arms.
#[test]
fn a_relation_holds_no_more_than_it_reserved() {
    let (assignable, charged, peak) = relate_measured(200, None);
    assert!(assignable, "every arm of S fits an arm of T");
    assert!(
        peak <= charged as i64 + DEMAND_SETUP_BYTES,
        "peak live {peak} bytes past the {charged} bytes reserved"
    );
}

/// The relation shape that allocates the most per unit of work, 600 reversed
/// object arms (about 180,000 structured pairs), under a 32 MiB construction
/// allowance: the relation stops at the allowance, typed, and never holds
/// more live than the allowance plus the demand's setup.
#[test]
fn the_worst_relation_shape_stops_within_its_byte_allowance() {
    let limit = 32 << 20;
    let (assignable, charged, peak) = relate_measured(600, Some(limit));
    assert!(
        !assignable,
        "the allowance refuses the relation before it completes"
    );
    assert!(
        charged <= limit,
        "{charged} bytes charged past the {limit}-byte allowance"
    );
    assert!(
        peak <= limit as i64 + DEMAND_SETUP_BYTES,
        "peak live {peak} bytes past the {limit}-byte allowance"
    );
}

/// `type R = \`${A}-${B}\`` over `a` string literals in `A` and `b` in `B`.
fn template_over(a: usize, b: usize) -> String {
    let union = |prefix: &str, count: usize| {
        (0..count)
            .map(|i| format!("\"{prefix}{i}\""))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    format!(
        "export type A = {};\nexport type B = {};\nexport type R = `${{A}}-${{B}}`;\n",
        union("a", a),
        union("b", b)
    )
}

/// 369 × 271 = 99,999 concatenations, each reserving its pieces and the
/// literal node it interns before it is built: the peak the reduction
/// holds live stays within what it charged.
///
/// Measured on TypeScript 7.0.2 (all four settings agree): `IsAny<R>` is
/// `"not-any"` and `"a0-b0" extends R ? 1 : 2` is `1`.
#[test]
fn a_template_holds_no_more_than_it_reserved() {
    let host = host_with(&template_over(369, 271));
    let _ = verter_session::for_tests::reduce_named_template_for_tests(&host, FILE, "A", None);
    let _ = verter_session::for_tests::reduce_named_template_for_tests(&host, FILE, "B", None);
    reset_alloc_counter();
    let (complete, charged) =
        verter_session::for_tests::reduce_named_template_for_tests(&host, FILE, "R", None);
    let peak = peak_live_bytes();
    assert!(
        complete,
        "99,999 concatenations are within the checker's limit"
    );
    assert!(
        peak <= charged as i64 + DEMAND_SETUP_BYTES,
        "peak live {peak} bytes past the {charged} bytes reserved"
    );
}

/// `S` and `T` unions of `count` single-property objects aligned arm for
/// arm; the target property is `number` when `related` and `string` when
/// not, so the relation holds or fails at the first arm.
fn aligned_unions(count: usize, related: bool) -> String {
    let source: Vec<String> = (0..count).map(|i| format!("{{ p{i}: {i} }}")).collect();
    let property = if related { "number" } else { "string" };
    let target: Vec<String> = (0..count)
        .map(|i| format!("{{ p{i}: {property} }}"))
        .collect();
    format!(
        "export type S = {};\nexport type T = {};\n",
        source.join(" | "),
        target.join(" | ")
    )
}

/// Bytes the allocator hands out on this thread relating aligned `S` to
/// `T` over `count` arms, with the declarations already read, and whether
/// `S` holds.
fn aligned_relation_bytes(count: usize, related: bool) -> (bool, u64) {
    let host = host_with(&aligned_unions(count, related));
    let _ = verter_session::for_tests::relate_named_types_for_tests(&host, FILE, "S", "S", None);
    let _ = verter_session::for_tests::relate_named_types_for_tests(&host, FILE, "T", "T", None);
    reset_alloc_counter();
    let (assignable, _work, _charged) =
        verter_session::for_tests::relate_named_types_for_tests(&host, FILE, "S", "T", None);
    (assignable, super::alloc_bytes())
}

/// Aligned unions of 200, 400 and 800 arms, true and false: the relation
/// decides as the checker does, and each doubling of the arms at most about
/// doubles the bytes allocated, so neither series allocates superlinearly.
#[test]
fn aligned_relation_allocation_grows_linearly_with_the_arms() {
    for related in [true, false] {
        let bytes = [200, 400, 800].map(|count| {
            let (assignable, bytes) = aligned_relation_bytes(count, related);
            assert_eq!(assignable, related, "{count} arms");
            bytes
        });
        for step in 0..2 {
            assert!(
                bytes[step + 1] * 10 <= bytes[step] * 25,
                "doubling the arms at most about doubles the bytes allocated \
                 (related {related}): {bytes:?}"
            );
        }
    }
}
