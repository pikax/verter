//! Architecture guard — the upsert chokepoint ALWAYS canonicalizes a
//! supplied `canonical_id` (CROSS-PLATFORM PORTABILITY class).
//!
//! `VerterHost::resolve_upsert_canonical`
//! (`crates/verter_session/src/host_upsert.rs`) is the SINGLE derivation
//! both upsert entry points (`upsert_with_priority`,
//! `submit_upsert_batch`) use for the canonical id that enters host state
//! (the scheduler node, the self-alias mint, the `DerivedRawState` key,
//! the parse-domain fact registration). Its invariant: the resolved
//! canonical is `canonicalize_id`-normalized for EVERY request —
//! including one whose `canonical_id` was caller-SUPPLIED, not derived
//! from `input_id`.
//!
//! Without this, a caller-provided variant spelling of the same file —
//! the canonical example is an upper-case Windows drive letter `C:/...`
//! against the canonical `c:/...` — mints a SECOND host identity:
//! caller-wired alias routes (`set_import_dependencies`) live under the
//! lower-drive key while the compile path reads under the upper-drive
//! key, so cross-file macro types silently degrade to Unknown
//! (`HOST_MISSING_MACRO_TYPE_DEP`). The runtime regression test
//! (`runtime_render_lane_tests::runtime_render_upper_drive_input_resolves_macro_types_wired_under_lower_drive_routes`)
//! proves the end-to-end behavior; this static guard pins the chokepoint
//! SHAPE so the enforcement cannot quietly regress to a `None`-only
//! canonicalization, a verbatim supplied-id passthrough, or a debug-gated
//! form compiled out of release builds.
//!
//! Consistent with this block's other static guards
//! (`uniqueness_check_release_active`): it scans only production source,
//! extracts the SPECIFIC enforcing region (not a whole-file
//! `canonicalize_id` grep, which would pass trivially via the `None`-arm
//! call), and ships companion fixtures proving the analysis would FLAG
//! the regressions it exists to catch — so the guard is discriminating,
//! not a stub.

use std::path::PathBuf;

/// The production source file owning the canonical-id derivation.
fn host_upsert_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("host_upsert.rs")
}

/// The single derivation fn whose BOTH arms must canonicalize.
const DERIVATION_FN: &str = "fn resolve_upsert_canonical";

/// The two upsert entry points that must route through the derivation fn.
const ENTRY_FNS: &[&str] = &["fn upsert_with_priority", "fn submit_upsert_batch"];

/// Strip `//`/`///`/`//!` line comments and `/* */` block comments so a
/// helper name appearing only in documentation does not count as an
/// invocation. Conservative; sufficient for this file's source shape.
fn strip_comments(src: &str) -> String {
    // 1. Block comments.
    let mut no_block = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        no_block.push(bytes[i] as char);
        i += 1;
    }
    // 2. Line/doc comments.
    let mut out = String::with_capacity(no_block.len());
    for line in no_block.lines() {
        let code = match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        };
        out.push_str(code);
        out.push('\n');
    }
    out
}

/// Extract the brace-delimited body of the named fn from `src`, INCLUDING
/// its signature line up to the matching closing brace. Returns `None` if
/// the fn or its opening brace is not found.
fn extract_fn_region(src: &str, fn_sig_needle: &str) -> Option<String> {
    let start = src.find(fn_sig_needle)?;
    let rest = &src[start..];
    let open_rel = rest.find('{')?;
    let mut depth: i32 = 0;
    let mut end_rel = None;
    for (off, ch) in rest[open_rel..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end_rel = Some(open_rel + off + ch.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }
    let end_rel = end_rel?;
    Some(rest[..end_rel].to_string())
}

/// Remove ALL whitespace so multi-line formatting of one expression
/// (rustfmt line breaks in a method chain) cannot hide an idiom from a
/// textual check. Rust whitespace between chain segments is
/// insignificant, so the stripped form is a faithful match target.
fn strip_all_whitespace(src: &str) -> String {
    src.chars().filter(|ch| !ch.is_whitespace()).collect()
}

/// Every invocation of `needle_fn(` in `region` (whitespace-stripped)
/// must pass EXACTLY `expected_arg` — returns the first offending
/// argument otherwise. `None` means all call sites conform (or none
/// exist; existence is asserted separately).
fn first_nonconforming_call_arg(
    stripped_region: &str,
    needle_fn: &str,
    expected_arg: &str,
) -> Option<String> {
    let needle = format!("{needle_fn}(");
    let mut from = 0;
    while let Some(rel) = stripped_region[from..].find(&needle) {
        let arg_start = from + rel + needle.len();
        let rest = &stripped_region[arg_start..];
        let end = rest.find(')').unwrap_or(rest.len());
        let arg = &rest[..end];
        if arg != expected_arg {
            return Some(arg.to_string());
        }
        from = arg_start + end;
    }
    None
}

/// Extract one match-arm body from `region`: the text between the `=>`
/// following `arm_pat` and the arm-terminating `,` (or end of region).
/// Sufficient for the single-expression arms this derivation fn carries.
fn match_arm_body<'a>(region: &'a str, arm_pat: &str) -> Option<&'a str> {
    let start = region.find(arm_pat)? + arm_pat.len();
    let rest = &region[start..];
    let arrow = rest.find("=>")?;
    let body = &rest[arrow + 2..];
    let end = body.find(',').unwrap_or(body.len());
    Some(&body[..end])
}

/// The verdict of analysing the derivation-fn region.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// BOTH the `Some(...)` (supplied) arm and the `None` (derived) arm
    /// invoke `canonicalize_id`, release-actively: GOOD.
    CanonicalizesBothArms,
    /// The supplied-`canonical_id` arm passes the caller's spelling
    /// through WITHOUT `canonicalize_id` — the drive-case identity-split
    /// regression this guard exists to catch.
    SuppliedIdVerbatim,
    /// Canonicalization exists but is gated behind a debug-only construct
    /// (`cfg!(debug_assertions)` / `#[cfg(debug_assertions)]` /
    /// `debug_assert*!`) — compiled out of release, so a release build
    /// re-splits the identity.
    DebugGated,
    /// The `None`/derived arm lost its canonicalization (or an arm is
    /// missing entirely) — the fn no longer enforces the invariant.
    NoCanonicalization,
}

/// Classify the derivation-fn region (already comment-stripped). The
/// single discriminating analysis exercised by BOTH the real guard and
/// the companion fixtures.
fn classify_derivation(region: &str) -> Verdict {
    if region.contains("cfg!(debug_assertions)")
        || region.contains("#[cfg(debug_assertions)]")
        || region.contains("debug_assert")
    {
        return Verdict::DebugGated;
    }
    let some_arm_canonicalizes = match_arm_body(region, "Some(")
        .map(|body| body.contains("canonicalize_id("))
        .unwrap_or(false);
    let none_arm_canonicalizes = match_arm_body(region, "None")
        .map(|body| body.contains("canonicalize_id("))
        .unwrap_or(false);
    match (some_arm_canonicalizes, none_arm_canonicalizes) {
        (true, true) => Verdict::CanonicalizesBothArms,
        (false, _) => Verdict::SuppliedIdVerbatim,
        (true, false) => Verdict::NoCanonicalization,
    }
}

/// The legacy asymmetric derivation idiom: a supplied `canonical_id`
/// cloned into host state verbatim, with `canonicalize_id` reserved for
/// the `None` fallback. Checked whitespace-collapsed so rustfmt line
/// breaks cannot hide it.
const LEGACY_ASYMMETRIC_IDIOM: &str = ".canonical_id.clone().unwrap_or_else(";

#[test]
fn upsert_always_canonicalizes_supplied_canonical_id() {
    let path = host_upsert_src();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("guard cannot read {}: {e}", path.display()));
    let stripped = strip_comments(&text);
    let no_ws = strip_all_whitespace(&stripped);

    // 1. The single derivation fn exists and BOTH arms canonicalize,
    //    release-actively.
    let region = extract_fn_region(&stripped, DERIVATION_FN).unwrap_or_else(|| {
        panic!(
            "guard could not locate `{DERIVATION_FN}` in {} — the \
             canonical-id derivation chokepoint was renamed or removed. If \
             it moved, update this guard to track its new home; the \
             supplied-canonical_id canonicalization must remain a single \
             release-active chokepoint.",
            path.display()
        )
    });
    match classify_derivation(&region) {
        Verdict::CanonicalizesBothArms => {}
        Verdict::SuppliedIdVerbatim => panic!(
            "portability guard: `{DERIVATION_FN}` in {} lets a SUPPLIED \
             `canonical_id` reach host state without `canonicalize_id`. A \
             caller-provided variant spelling (upper-case Windows drive \
             letter) would mint a second host identity for the same file, \
             splitting alias routes and derived caches — the \
             HOST_MISSING_MACRO_TYPE_DEP degradation class. Canonicalize \
             BOTH arms.\nfn region:\n{region}",
            path.display()
        ),
        Verdict::DebugGated => panic!(
            "portability guard: `{DERIVATION_FN}` in {} gates its \
             canonicalization behind a debug-only construct. The invariant \
             guards production bundler input and MUST hold in release \
             builds — remove the debug gate.\nfn region:\n{region}",
            path.display()
        ),
        Verdict::NoCanonicalization => panic!(
            "portability guard: `{DERIVATION_FN}` in {} no longer \
             canonicalizes the derived (`None`) arm — every arm of the \
             derivation must produce a `canonicalize_id`-normalized \
             canonical.\nfn region:\n{region}",
            path.display()
        ),
    }

    // 2. BOTH upsert entry points route through the single derivation fn
    //    — no entry point re-derives a canonical on its own.
    for entry_fn in ENTRY_FNS {
        let entry_region = extract_fn_region(&stripped, entry_fn).unwrap_or_else(|| {
            panic!(
                "guard could not locate `{entry_fn}` in {} — if the upsert \
                 entry point was renamed, update this guard; its canonical \
                 derivation must keep routing through `{DERIVATION_FN}`.",
                path.display()
            )
        });
        assert!(
            entry_region.contains("resolve_upsert_canonical"),
            "portability guard: `{entry_fn}` in {} no longer derives its \
             canonical through `{DERIVATION_FN}` — a second derivation \
             path can silently drop the supplied-id canonicalization.\n\
             fn region:\n{entry_region}",
            path.display()
        );
    }

    // 3. The legacy asymmetric idiom is absent file-wide: no
    //    `*.canonical_id.clone().unwrap_or_else(...)` derivation survives
    //    anywhere in the production file (whitespace-stripped, so rustfmt
    //    line breaks cannot hide it).
    assert!(
        !no_ws.contains(LEGACY_ASYMMETRIC_IDIOM),
        "portability guard: {} still contains the legacy asymmetric \
         canonical-id derivation `{LEGACY_ASYMMETRIC_IDIOM}...` — a \
         supplied canonical_id must never bypass `canonicalize_id`.",
        path.display()
    );

    // 4. The parse-domain producer runs on the RESOLVED canonical for
    //    every request — never on a caller-supplied spelling. Every
    //    `register_facts_for_new_content(...)` call site inside
    //    `submit_upsert_batch` must take EXACTLY `&canonical_id` (the
    //    resolved canonical); any other argument (e.g. a `Some`-arm
    //    binding of the supplied id) re-introduces the asymmetric /
    //    unnormalized producer.
    let submit_region = extract_fn_region(&stripped, "fn submit_upsert_batch")
        .expect("submit_upsert_batch located by step 2");
    let submit_no_ws = strip_all_whitespace(&submit_region);
    assert!(
        submit_no_ws.contains("register_facts_for_new_content(&canonical_id)"),
        "portability guard: `fn submit_upsert_batch` must run \
         `register_facts_for_new_content(&canonical_id)` on the RESOLVED \
         canonical for every request.\nfn region:\n{submit_region}"
    );
    if let Some(bad_arg) = first_nonconforming_call_arg(
        &submit_no_ws,
        "register_facts_for_new_content",
        "&canonical_id",
    ) {
        panic!(
            "portability guard: `fn submit_upsert_batch` calls \
             `register_facts_for_new_content({bad_arg})` — the parse-domain \
             producer must fire on the RESOLVED canonical (`&canonical_id`) \
             for EVERY request, uniformly across supplied and derived \
             canonical_id requests, never on a caller-supplied spelling.\n\
             fn region:\n{submit_region}"
        );
    }
}

// ---------------------------------------------------------------------------
// Discriminating-property fixtures: the SAME analysis used by the guard
// above must PASS the real both-arms form and FLAG each regression shape.
// If these hold, the guard is discriminating — it would catch a change
// that downgraded the production chokepoint.
// ---------------------------------------------------------------------------

/// (a) The real both-arms canonicalizing form classifies as
/// `CanonicalizesBothArms`.
#[test]
fn fixture_both_arms_canonicalize_passes() {
    let good = r#"
        fn resolve_upsert_canonical(req: &UpsertRequest) -> String {
            match &req.canonical_id {
                Some(id) => canonicalize_id(id).into_owned(),
                None => canonicalize_id(&req.input_id).into_owned(),
            }
        }
    "#;
    let region = extract_fn_region(&strip_comments(good), DERIVATION_FN).expect("fn region");
    assert_eq!(classify_derivation(&region), Verdict::CanonicalizesBothArms);
}

/// (b) A supplied-id VERBATIM passthrough (`Some(id) => id.clone()`) is
/// flagged — this is the drive-case identity-split regression the guard
/// exists to catch. Critically, the region still TEXTUALLY contains
/// `canonicalize_id(` (in the `None` arm), so a naive whole-region
/// "contains canonicalize_id" grep would PASS it; the per-arm analysis
/// must FLAG it.
#[test]
fn fixture_supplied_id_verbatim_is_flagged() {
    let verbatim = r#"
        fn resolve_upsert_canonical(req: &UpsertRequest) -> String {
            match &req.canonical_id {
                Some(id) => id.clone(),
                None => canonicalize_id(&req.input_id).into_owned(),
            }
        }
    "#;
    let region = extract_fn_region(&strip_comments(verbatim), DERIVATION_FN).expect("fn region");
    // Sanity: the region DOES contain `canonicalize_id(` (the None arm),
    // so region-wide substring grepping is fooled.
    assert!(
        region.contains("canonicalize_id("),
        "precondition: the verbatim form still contains canonicalize_id in the None arm"
    );
    assert_eq!(
        classify_derivation(&region),
        Verdict::SuppliedIdVerbatim,
        "a Some-arm verbatim passthrough MUST be flagged — the guard would \
         FAIL on this, catching the regression"
    );
}

/// (c) A debug-gated canonicalization is flagged: `cfg!(debug_assertions)`
/// gating means release builds re-split the identity.
#[test]
fn fixture_debug_gated_canonicalization_is_flagged() {
    let gated = r#"
        fn resolve_upsert_canonical(req: &UpsertRequest) -> String {
            match &req.canonical_id {
                Some(id) => {
                    if cfg!(debug_assertions) {
                        canonicalize_id(id).into_owned()
                    } else {
                        id.clone()
                    }
                },
                None => canonicalize_id(&req.input_id).into_owned(),
            }
        }
    "#;
    let region = extract_fn_region(&strip_comments(gated), DERIVATION_FN).expect("fn region");
    assert_eq!(classify_derivation(&region), Verdict::DebugGated);
}

/// (d) A `None`-arm regression (derived id no longer canonicalized) is
/// flagged as `NoCanonicalization`.
#[test]
fn fixture_none_arm_regression_is_flagged() {
    let none_verbatim = r#"
        fn resolve_upsert_canonical(req: &UpsertRequest) -> String {
            match &req.canonical_id {
                Some(id) => canonicalize_id(id).into_owned(),
                None => req.input_id.clone(),
            }
        }
    "#;
    let region =
        extract_fn_region(&strip_comments(none_verbatim), DERIVATION_FN).expect("fn region");
    assert_eq!(classify_derivation(&region), Verdict::NoCanonicalization);
}

/// (e) The whitespace-stripped legacy-idiom check catches the historical
/// asymmetric derivation even when rustfmt splits it across lines.
#[test]
fn fixture_legacy_asymmetric_idiom_detected_across_line_breaks() {
    let legacy = r#"
        let canonical_id = req
            .canonical_id
            .clone()
            .unwrap_or_else(|| canonicalize_id(&req.input_id).into_owned());
    "#;
    let stripped = strip_all_whitespace(&strip_comments(legacy));
    assert!(
        stripped.contains(LEGACY_ASYMMETRIC_IDIOM),
        "the whitespace-stripped scan must detect the multi-line legacy \
         idiom; stripped: {stripped}"
    );
}

/// (e2) The per-call-site argument check flags a producer invoked on a
/// supplied-spelling binding instead of the resolved canonical — in any
/// conditional shape, not just the historical `if let Some(ref id)` form.
#[test]
fn fixture_producer_on_supplied_spelling_is_flagged() {
    let asymmetric = r#"
        fn submit_upsert_batch(&self) {
            if let Some(id) = &req.canonical_id {
                self.register_facts_for_new_content(id);
            }
        }
    "#;
    let stripped = strip_all_whitespace(&strip_comments(asymmetric));
    assert_eq!(
        first_nonconforming_call_arg(&stripped, "register_facts_for_new_content", "&canonical_id"),
        Some("id".to_string()),
        "a producer call on a supplied-spelling binding must be flagged"
    );
    // The conforming resolved-canonical form passes.
    let conforming = strip_all_whitespace(
        "fn submit_upsert_batch(&self) { self.register_facts_for_new_content(&canonical_id); }",
    );
    assert_eq!(
        first_nonconforming_call_arg(
            &conforming,
            "register_facts_for_new_content",
            "&canonical_id"
        ),
        None,
        "the resolved-canonical producer call must conform"
    );
}

/// (f) Comment-only mentions of the legacy idiom or a debug gate do NOT
/// trip the guard — the analysis strips comments first.
#[test]
fn fixture_comment_mentions_are_ignored() {
    let documented = r#"
        fn resolve_upsert_canonical(req: &UpsertRequest) -> String {
            // Historically `.canonical_id.clone().unwrap_or_else(` let a
            // supplied spelling through; cfg!(debug_assertions) gating is
            // equally forbidden. debug_assert! must never gate this.
            match &req.canonical_id {
                Some(id) => canonicalize_id(id).into_owned(),
                None => canonicalize_id(&req.input_id).into_owned(),
            }
        }
    "#;
    let stripped = strip_comments(documented);
    let region = extract_fn_region(&stripped, DERIVATION_FN).expect("fn region");
    assert_eq!(
        classify_derivation(&region),
        Verdict::CanonicalizesBothArms,
        "comment-only mentions must be stripped before analysis"
    );
    assert!(
        !strip_all_whitespace(&stripped).contains(LEGACY_ASYMMETRIC_IDIOM),
        "comment-only mention of the legacy idiom must not trip the file-wide scan"
    );
}
