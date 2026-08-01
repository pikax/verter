use std::collections::HashMap;
use std::fmt::Write;

use super::{
    lower_carrier_specifiers_in_module_positions_observed, prev_significant_byte,
    SignificantByteScanWork,
};

/// The observed running value is production state, while
/// `prev_significant_byte` is the unchanged from-byte-zero oracle.
///
/// Mutation recipe: after the production string arm calls
/// `skip_insignificant`, assign `significant.last_significant = Some(quote)`;
/// prove this test fails on `closed_strings_and_templates`, restore the source,
/// and rerun this test plus the scan-work control.
#[test]
fn running_significant_byte_matches_rescan_oracle_at_every_identifier_lookup() {
    let corpus = [
        (
            "comments_around_dot",
            "loader.import(\"./A.vue\");\n\
             loader /* before dot */ . import(\"./B.vue\");\n\
             loader. /* after dot */ import(\"./C.vue\");\n\
             loader. // after dot\n import(\"./D.vue\");\n\
             loader // before dot\n . /* after dot */ import(\"./E.vue\");\n\
             const unqualified = 1; // comment . import\n\
             import(\"./F.vue\");",
        ),
        (
            "multiline_block_comments",
            "const before = 1;\n\
             loader. /* first line\n . // still block text\n */ import(\"./A.vue\");\n\
             const after = 2; /* first\n second */ import(\"./B.vue\");",
        ),
        (
            "closed_strings_and_templates",
            "const single = '. // import(\"./fake-a.vue\")';\n\
             const double = \". /* export from fake */ //\";\n\
             const escaped = \"quote: \\\" . // still string\";\n\
             const no_semicolon = \". // import\"\n\
             import(\"./after-string.vue\");\n\
             const template = `first\n. // import(\"./fake-b.vue\")`;\n\
             const escaped_tick = `first \\` . // still template`;\n\
             const template_no_semicolon = `. // export`\n\
             export { template_no_semicolon } from \"./after-template.vue\";\n\
             import(\"./real.vue\");",
        ),
        (
            "template_interpolation_is_opaque",
            "const value = `head ${obj.import(\"./fake.vue\") + ({ deep: { dot: \".//\" } })} tail`;\n\
             export { value } from \"./real.vue\";",
        ),
        (
            "unterminated_runs_to_eof",
            "const before = loader.import(\"./A.vue\");\n\
             const marker = 1; /* unterminated . // import(\"./fake.vue\")",
        ),
        (
            "unterminated_line_comment",
            "const before = 1;\n\
             loader.import(\"./A.vue\"); // unterminated . import(\"./fake.vue\")",
        ),
        (
            "ordinary_punctuation_and_whitespace",
            "const quotient = left / right;\n\
             const optional = loader?.import(\"./A.vue\");\n\
             export\t{\n value\n}\tfrom /* gap */ \"./B.vue\";",
        ),
    ];

    let mut saw_member_access = false;
    let mut saw_non_member_access = false;

    for (case, source) in corpus {
        let mut lookups = 0usize;
        let (output, work) = lower_carrier_specifiers_in_module_positions_observed(
            source,
            &HashMap::new(),
            |start, running| {
                lookups += 1;
                let oracle = prev_significant_byte(source.as_bytes(), start);
                assert_eq!(
                    running,
                    oracle,
                    "running significant byte diverged from the rescan oracle \
                     in {case} at byte {start} ({:?})",
                    &source[start..]
                );
                saw_member_access |= running == Some(b'.');
                saw_non_member_access |= running != Some(b'.');
            },
        );

        assert_eq!(
            output, source,
            "an empty carrier map must preserve the lexical corpus for {case}"
        );
        assert_eq!(
            work.identifier_lookups, lookups,
            "the counter must cover every observed production lookup for {case}"
        );
        assert!(
            lookups > 0,
            "the differential corpus case must exercise at least one identifier lookup: {case}"
        );
    }

    assert!(
        saw_member_access,
        "the corpus must exercise the member-access decision"
    );
    assert!(
        saw_non_member_access,
        "the corpus must also exercise genuine unqualified keywords"
    );
}

/// The old implementation traversed `0..start` for every identifier lookup, so
/// summing the observed start offsets is its exact deterministic byte-traversal
/// count. The running implementation classifies each input byte once.
///
/// Mutation recipe: in `previous_for_identifier`, add `start` to
/// `self.work.bytes_scanned` to model the old prefix rescan at every lookup and
/// require this test's exact once-per-byte assertion to fail; restore and rerun
/// both tests in this module.
#[test]
fn running_scan_work_is_linear_on_a_large_generated_sfc_validation_fixture() {
    let small = generated_validation_fixture(256);
    let large = generated_validation_fixture(512);

    let (small_old, small_new) = measured_scan_work(&small);
    let (large_old, large_new) = measured_scan_work(&large);

    assert_eq!(
        small_new.bytes_scanned,
        small.len(),
        "the running state must classify every byte exactly once"
    );
    assert_eq!(
        large_new.bytes_scanned,
        large.len(),
        "doubling the generated SFC body must still classify every byte once"
    );
    assert!(
        large_old >= small_old * 3,
        "the from-zero oracle must expose super-linear growth when the fixture doubles: \
         small={small_old}, large={large_old}"
    );
    assert!(
        large_new.bytes_scanned <= small_new.bytes_scanned * 3,
        "the running counter must remain linear when the fixture doubles: \
         small={}, large={}",
        small_new.bytes_scanned,
        large_new.bytes_scanned
    );
    assert!(
        large_old > large_new.bytes_scanned * 100,
        "the realistic many-identifier fixture must discriminate a quadratic rescan: \
         before={large_old}, after={}",
        large_new.bytes_scanned
    );

    eprintln!(
        "significant-byte scan work: before={large_old} bytes, after={} bytes, \
         identifier_lookups={}",
        large_new.bytes_scanned, large_new.identifier_lookups
    );
}

fn measured_scan_work(source: &str) -> (usize, SignificantByteScanWork) {
    let mut legacy_bytes_scanned = 0usize;
    let (output, running_work) = lower_carrier_specifiers_in_module_positions_observed(
        source,
        &HashMap::new(),
        |start, _| legacy_bytes_scanned += start,
    );
    assert!(
        output == source,
        "the counter fixture must not change while its scan work is measured \
         (input={} bytes, output={} bytes)",
        source.len(),
        output.len()
    );
    (legacy_bytes_scanned, running_work)
}

fn generated_validation_fixture(rows: usize) -> String {
    let mut source = String::with_capacity(rows * 256);
    source.push_str(
        "import type { ComponentPublicInstance } from \"vue\";\n\
         import Child from \"/workspace/src/Child.vue\";\n\
         const __VLS_ctx = {} as ComponentPublicInstance;\n\
         export function __VLS_template() {\n",
    );

    for row in 0..rows {
        writeln!(
            source,
            "  const row_{row} = __VLS_ctx.items[{row}]; \
             (<Child label={{row_{row}.label}} onClick={{() => \
             __VLS_ctx.select(row_{row}.id)}} />);"
        )
        .expect("writing generated validation fixture cannot fail");
    }

    source.push_str("}\nexport default {} as typeof __VLS_template;\n");
    source
}
