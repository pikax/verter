//! Architecture guard — no production cache-admission path may publish
//! a carrier that roots a value on a `RouteGeneration` dependency.
//!
//! `DepVersion::RouteGeneration` has no authoritative validating source:
//! there is no production emitter and no real route-generation counter.
//! A cache entry rooted on a `RouteGeneration` dependency could not
//! detect a content edit to the route-observed file — it would validate
//! as always-valid and serve stale indefinitely.
//!
//! Two distinct roles handle a `DepVersion` set, and the guard holds
//! each to its OWN obligation:
//!
//! - **Entry producers** build a fact signature / cache-admission
//!   carrier for a shared cache entry. A `RouteGeneration` dependency
//!   MUST abort the WHOLE entry — a statement-position `return None` /
//!   `ComputeAdmission::ReturnOnly` that declines shared admission
//!   entirely. A per-item `filter_map`/closure `=> None` drop is NOT
//!   sufficient here: it would silently drop only that one fact and
//!   still publish a carrier whose signature is missing the dependency,
//!   so the entry would validate as always-valid.
//! - **Converters** translate a `DepVersion` set into a `FactVersionRef`
//!   set without themselves admitting anything. `RouteGeneration` has no
//!   `FactVersionRef` representation, so a converter legitimately
//!   filter-drops it; the whole-carrier refusal is enforced by the entry
//!   producers that consume the legacy `DepSignature` rail. A converter
//!   must still demonstrably HANDLE the variant (mention it) so a
//!   future `DepVersion` addition cannot be dropped unnoticed.
//!
//! This guard extracts each producer's brace-balanced body and applies
//! the classification-appropriate check. A self-test exercises the
//! discriminator against synthetic bodies — crucially, a synthetic
//! ENTRY PRODUCER whose only `RouteGeneration` handling is a
//! `filter_map => None` drop flips the guard RED, while a
//! statement-position `return None` / `ReturnOnly` passes.

use std::fs;
use std::path::PathBuf;

/// Read a `verter_session` source file relative to `src/`.
fn read_session_source(relative: &str) -> String {
    let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(cargo_manifest_dir);
    path.push("src");
    path.push(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

/// Extract the brace-balanced body (including the outer `{ }`) of the
/// first occurrence of `needle`.
fn extract_balanced_body<'a>(src: &'a str, needle: &str) -> &'a str {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in source"));
    let after = &src[start..];
    let open = after
        .find('{')
        .unwrap_or_else(|| panic!("expected an opening brace after `{needle}`"));
    let bytes = after.as_bytes();
    let mut depth = 0usize;
    let mut idx = open;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &after[open..=idx];
                }
            }
            _ => {}
        }
        idx += 1;
    }
    panic!("expected a brace-balanced body for `{needle}`");
}

/// Collapse every run of ASCII whitespace in `body` to a single space.
/// The scan tokens below are then matched against this normalised form,
/// so a reformat that changes indentation / line breaks cannot mutate a
/// match (a whitespace-sensitive raw `contains` would mis-fire).
fn normalize_whitespace(body: &str) -> String {
    body.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strip Rust comments (`// line` and `/* block */`) from `body`. A
/// comment that incidentally mentions a refusal-shape keyword
/// (e.g. `// see RouteGenerationDependency for the rail shape`)
/// must not satisfy a substring predicate on the actual code — the
/// guard is structural, not lexical-substring.
fn strip_comments(body: &str) -> String {
    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    let mut in_str = false;
    let mut in_char = false;
    let mut esc = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            out.push(b as char);
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            out.push(b as char);
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'\'' {
                in_char = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_str = true;
            out.push('"');
            i += 1;
            continue;
        }
        if b == b'\'' {
            // Heuristic: a lifetime (`'a`) is rare in body
            // expressions; treat any `'` as char-literal start. The
            // worst case is over-stripping, which keeps the guard
            // strictly stricter (not weaker).
            in_char = true;
            out.push('\'');
            i += 1;
            continue;
        }
        // Line comment.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            // Skip to end of line.
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Block comment (no nesting tracking — Rust's nested
        // `/* /* */ */` is rare in scanned bodies; the inner `*/`
        // closes the outer here, leaving the trailing `*/` as raw
        // tokens that cannot satisfy any refusal-shape predicate).
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

/// Whether `body` carries a WHOLE-CARRIER `RouteGeneration` refusal: it
/// mentions the `RouteGeneration` variant AND, in the same body, aborts
/// the entire carrier via a statement-position `return` refusal whose
/// PAYLOAD names the `RouteGenerationDependency` reason (or, for the
/// `Option`-returning shape, returns bare `None`).
///
/// The discriminating point is the `return` keyword. A per-item
/// `filter_map`/closure drop is written `=> None` (a bare arm
/// expression, NO `return`), so a body whose only `RouteGeneration`
/// handling is a `filter_map => None` does NOT satisfy this — exactly
/// the silent-drop shape the guard must catch on an entry producer.
///
/// Four accepted refusal shapes (each anchored to the SAME return
/// expression — no cross-statement substring AND):
///
///   * `return None` — `Option`-returning producer aborts the entry.
///   * `return ComputeAdmission::ReturnOnly(...)` —
///     `ComputeAdmission`-returning producer routes the value through
///     the non-cacheable arm.
///   * `return Err(...RouteGenerationDependency...)` —
///     `Result`-returning producer aborts the entry with the typed
///     reason. The regex anchors `RouteGenerationDependency` to
///     appear INSIDE the `Err(...)` parenthesis group (`[^)]*` between
///     the open `(` and the discriminator), so a body containing
///     `return Err(OtherError)` in one expression and a comment / use
///     of `RouteGenerationDependency` elsewhere does NOT satisfy this
///     predicate.
///   * `return SignatureAdmission::NonCacheable(...RouteGenerationDependency...)`
///     — `SignatureAdmission`-returning producer. Same anchoring.
///
/// Comments are stripped before the regex applies so a comment
/// incidentally containing a refusal-shape keyword cannot satisfy
/// a predicate on the code.
fn body_has_whole_carrier_route_refusal(body: &str) -> bool {
    let stripped = strip_comments(body);
    let norm = normalize_whitespace(&stripped);
    if !norm.contains("RouteGeneration") {
        return false;
    }
    // A whole-carrier refusal is a `return`-prefixed statement that
    // aborts the entry. `::` spacing is collapsed first so a reformat
    // cannot break the `ComputeAdmission::ReturnOnly` or
    // `NonAdmissionReason::RouteGenerationDependency` match. A per-item
    // `filter_map` drop is a bare `=> None` arm — no `return` — so it
    // does NOT satisfy any branch.
    let norm_tight = norm.replace(" :: ", "::");
    if norm_tight.contains("return None") {
        return true;
    }
    if norm_tight.contains("return ComputeAdmission::ReturnOnly") {
        return true;
    }
    // `return Err(<NonAdmissionReason expression>)` is the
    // `Result`-returning typed-reason refusal shape. The reason MUST
    // be `RouteGenerationDependency` AND must live INSIDE the same
    // `Err(...)` parenthesis group (anchored via the `[^)]*`
    // character class). A body with `return Err(OtherError)` in one
    // expression and an unrelated `RouteGenerationDependency` mention
    // elsewhere does NOT match.
    let return_err_typed = regex::Regex::new(r"return\s+Err\s*\(\s*[^)]*RouteGenerationDependency")
        .expect("anchored regex literal compiles");
    if return_err_typed.is_match(&norm_tight) {
        return true;
    }
    // `return SignatureAdmission::NonCacheable(<NonAdmissionReason>)`
    // is the `SignatureAdmission`-returning typed-reason refusal shape.
    // Same SAME-EXPRESSION anchoring as the `Err` case above.
    let return_sig_admission_typed = regex::Regex::new(
        r"return\s+SignatureAdmission::NonCacheable\s*\(\s*[^)]*RouteGenerationDependency",
    )
    .expect("anchored regex literal compiles");
    if return_sig_admission_typed.is_match(&norm_tight) {
        return true;
    }
    false
}

/// Whether `body` (a converter) demonstrably HANDLES the
/// `RouteGeneration` variant — it must mention it so a future
/// `DepVersion` addition cannot be dropped unnoticed. A converter does
/// NOT need a whole-carrier refusal (it admits nothing; the entry
/// producers that consume its output carry the refusal).
fn body_handles_route_generation(body: &str) -> bool {
    normalize_whitespace(body).contains("RouteGeneration")
}

/// The role a `DepVersion`-handling producer plays — fixes its
/// `RouteGeneration` obligation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ProducerKind {
    /// Builds a shared-cache admission carrier. MUST whole-carrier-refuse.
    EntryProducer,
    /// Translates `DepVersion` → `FactVersionRef` without admitting.
    /// Legitimately filter-drops `RouteGeneration`; must still handle it.
    Converter,
}

struct Producer {
    /// Source file relative to `src/`.
    file: &'static str,
    /// A substring that uniquely identifies the producer's signature
    /// line.
    signature: &'static str,
    kind: ProducerKind,
}

/// The full producer roster — explicit per-producer classification.
fn producers() -> [Producer; 5] {
    [
        // The 5 ENTRY PRODUCERS — each builds a shared-cache admission
        // carrier and MUST abort the whole entry on `RouteGeneration`.
        Producer {
            file: "component_meta_materialize.rs",
            signature: "pub fn fact_signature_from_fence(",
            kind: ProducerKind::EntryProducer,
        },
        Producer {
            file: "component_meta_materialize.rs",
            signature: "fn materialize_structure_read_set(",
            kind: ProducerKind::EntryProducer,
        },
        Producer {
            file: "component_meta_caches.rs",
            signature: "fn trace_ref_cycle_compute<C>(",
            kind: ProducerKind::EntryProducer,
        },
        Producer {
            file: "resolver_core/component_meta_query_engine/mod.rs",
            signature: "pub(crate) fn engine_fact_signature_for_materialize_memo(",
            kind: ProducerKind::EntryProducer,
        },
        // The CONVERTER — `dep_signature_to_fact_signature` translates a
        // `DepSignature` into a `FactVersionRef` set. `RouteGeneration`
        // has no `FactVersionRef` representation, so it legitimately
        // filter-drops it; the whole-carrier refusal is enforced by the
        // entry producers above. The guard must NOT demand a refusal
        // from it, only that it still handles the variant.
        Producer {
            file: "fact_signature_helpers.rs",
            signature: "pub(crate) fn dep_signature_to_fact_signature(",
            kind: ProducerKind::Converter,
        },
    ]
}

/// Every entry producer MUST whole-carrier-refuse `RouteGeneration`; the
/// converter MUST still handle the variant.
#[test]
fn no_admitted_carrier_roots_on_route_generation() {
    for producer in producers() {
        let src = read_session_source(producer.file);
        let body = extract_balanced_body(&src, producer.signature);
        match producer.kind {
            ProducerKind::EntryProducer => assert!(
                body_has_whole_carrier_route_refusal(body),
                "ENTRY PRODUCER `{}` in `{}` MUST refuse a `RouteGeneration` dependency \
                 with a WHOLE-CARRIER refusal — a statement-position `return None` / \
                 `return ComputeAdmission::ReturnOnly(..)` that aborts the entire cache \
                 entry. A per-item `filter_map`/closure `=> None` drop is NOT sufficient: \
                 it drops only that one fact and still publishes a carrier whose \
                 signature cannot catch a content edit. Body:\n{body}",
                producer.signature,
                producer.file,
            ),
            ProducerKind::Converter => assert!(
                body_handles_route_generation(body),
                "CONVERTER `{}` in `{}` MUST demonstrably handle the `RouteGeneration` \
                 variant (mention it) so a future `DepVersion` addition cannot be \
                 dropped unnoticed. A converter legitimately filter-drops \
                 `RouteGeneration` — the whole-carrier refusal is enforced by the entry \
                 producers — but it must still match the variant. Body:\n{body}",
                producer.signature,
                producer.file,
            ),
        }
    }
}

/// Self-test: the discriminator distinguishes a WHOLE-CARRIER refusal
/// from a per-item `filter_map` drop. The decisive case — a synthetic
/// ENTRY PRODUCER whose only `RouteGeneration` handling is a
/// `filter_map => None` drop — MUST flip `body_has_whole_carrier_route_refusal`
/// RED, while a statement-position `return None` / `ReturnOnly` passes.
#[test]
fn route_generation_scanner_discriminates() {
    // --- WHOLE-CARRIER refusals — entry-producer-grade. ---
    // A statement-position `return None` inside a `for`-loop match arm.
    let return_none_arm =
        "fn p() { for (_, v) in fence { match v { DepVersion::RouteGeneration(_) => { return None; } _ => {} } } Some(out) }";
    assert!(
        body_has_whole_carrier_route_refusal(return_none_arm),
        "self-test: a body that aborts via statement-position `return None` in a \
         RouteGeneration arm MUST be recognised as a whole-carrier refusal",
    );
    // An `if any(RouteGeneration) { return None; }` guard.
    let return_none_guard =
        "fn p() { if legacy.iter().any(|(_, v)| matches!(v, DepVersion::RouteGeneration(_))) { return None; } Some(x) }";
    assert!(
        body_has_whole_carrier_route_refusal(return_none_guard),
        "self-test: an `if any(RouteGeneration) {{ return None; }}` whole-carrier guard \
         MUST be recognised as a refusal",
    );
    // A `return ComputeAdmission::ReturnOnly(..)` whole-carrier refusal.
    let return_only =
        "fn p() { if fence.iter().any(|(_, v)| matches!(v, DepVersion::RouteGeneration(_))) { return ComputeAdmission::ReturnOnly(value()); } admit() }";
    assert!(
        body_has_whole_carrier_route_refusal(return_only),
        "self-test: a body that aborts via `return ComputeAdmission::ReturnOnly(..)` on \
         RouteGeneration MUST be recognised as a whole-carrier refusal",
    );
    // A `return Err(NonAdmissionReason::RouteGenerationDependency)`
    // whole-carrier refusal — the `Result`-returning producer shape
    // used by `materialize_structure_read_set` to thread the typed
    // reason back to the caller.
    let return_err_typed =
        "fn p() { for v in fence { match v { DepVersion::RouteGeneration(_) => { return Err(NonAdmissionReason::RouteGenerationDependency); } _ => {} } } Ok(out) }";
    assert!(
        body_has_whole_carrier_route_refusal(return_err_typed),
        "self-test: a `Result`-returning entry producer that aborts via \
         `return Err(NonAdmissionReason::RouteGenerationDependency)` on \
         RouteGeneration MUST be recognised as a whole-carrier refusal — \
         the typed reason flows back to the caller, who routes through \
         the cooperative refusal arm.",
    );
    // A bare `return Err(_)` without the `RouteGenerationDependency`
    // discriminator is NOT accepted — a producer that returns a
    // generic error without classifying the cause has not honoured
    // the typed-reason contract.
    let return_err_untyped =
        "fn p() { for v in fence { match v { DepVersion::RouteGeneration(_) => { return Err(()); } _ => {} } } Ok(out) }";
    assert!(
        !body_has_whole_carrier_route_refusal(return_err_untyped),
        "self-test: a `Result`-returning producer that returns a bare \
         `Err(_)` WITHOUT the `RouteGenerationDependency` discriminator \
         has NOT honoured the typed-reason contract — the typed reason \
         must reach the caller, not a generic error.",
    );
    // A `return SignatureAdmission::NonCacheable(NonAdmissionReason::RouteGenerationDependency)`
    // whole-carrier refusal — the `SignatureAdmission`-returning
    // producer shape used by `engine_fact_signature_for_materialize_memo`.
    let return_sig_admission_typed =
        "fn p() { for v in fence { match v { DepVersion::RouteGeneration(_) => { return SignatureAdmission::NonCacheable(NonAdmissionReason::RouteGenerationDependency); } _ => {} } } SignatureAdmission::Cacheable(out) }";
    assert!(
        body_has_whole_carrier_route_refusal(return_sig_admission_typed),
        "self-test: a `SignatureAdmission`-returning entry producer that \
         aborts via \
         `return SignatureAdmission::NonCacheable(NonAdmissionReason::RouteGenerationDependency)` \
         MUST be recognised as a whole-carrier refusal — the typed \
         reason flows to the caller, who routes through the cooperative \
         refusal arm.",
    );
    // A bare `return SignatureAdmission::NonCacheable(_)` without the
    // `RouteGenerationDependency` discriminator is NOT accepted.
    let return_sig_admission_untyped =
        "fn p() { for v in fence { match v { DepVersion::RouteGeneration(_) => { return SignatureAdmission::NonCacheable(other_reason); } _ => {} } } SignatureAdmission::Cacheable(out) }";
    assert!(
        !body_has_whole_carrier_route_refusal(return_sig_admission_untyped),
        "self-test: a `SignatureAdmission`-returning producer that \
         refuses with a non-RouteGenerationDependency reason has NOT \
         honoured the typed-reason contract for the `RouteGeneration` \
         variant — the typed reason must be `RouteGenerationDependency`.",
    );

    // --- THE DECISIVE CASE — a per-item `filter_map => None` drop. ---
    // This is exactly `dep_signature_to_fact_signature`'s legitimate
    // converter shape. On an ENTRY PRODUCER it is the silent-drop
    // violation: the guard MUST classify it as NOT a whole-carrier
    // refusal so a `filter_map`-only entry producer flips RED.
    let filter_map_drop =
        "fn p() { sig.iter().filter_map(|(c, v)| match v { DepVersion::WholeHash(h) => Some(f(h)), DepVersion::RouteGeneration(_) => None }).collect() }";
    assert!(
        !body_has_whole_carrier_route_refusal(filter_map_drop),
        "self-test (decisive): a body whose ONLY RouteGeneration handling is a \
         `filter_map`/closure `=> None` drop is NOT a whole-carrier refusal — it drops \
         one fact and still returns a Vec. If this were accepted, an entry producer \
         could omit the route dependency without the guard catching it.",
    );
    // The converter check, however, DOES accept the same body — a
    // converter only has to handle the variant.
    assert!(
        body_handles_route_generation(filter_map_drop),
        "self-test: the converter check accepts a `filter_map => None` body — a \
         converter legitimately filter-drops RouteGeneration; it only has to mention \
         the variant",
    );

    // --- A silently-dropping match arm (no return, no token). ---
    let dropping_arm =
        "fn p() { match v { DepVersion::RouteGeneration(_) => {} _ => keep() } finish() }";
    assert!(
        !body_has_whole_carrier_route_refusal(dropping_arm),
        "self-test: a body that mentions RouteGeneration but neither returns nor carries \
         a refusal token MUST fail the whole-carrier check",
    );

    // --- A body that does not mention the variant at all. ---
    let absent = "fn p() { if cond { return None; } Some(x) }";
    assert!(
        !body_has_whole_carrier_route_refusal(absent),
        "self-test: a body that never mentions RouteGeneration is not a refusal — the \
         `return None` must be reachable from RouteGeneration handling",
    );
    assert!(
        !body_handles_route_generation(absent),
        "self-test: a body that never mentions RouteGeneration does not handle it",
    );

    // --- Whitespace robustness — a reformat must not change the verdict. ---
    let reformatted =
        "fn p() {\n    match v {\n        DepVersion :: RouteGeneration ( _ )\n            => {\n                return None ;\n            }\n        _ => {}\n    }\n}";
    assert!(
        body_has_whole_carrier_route_refusal(reformatted),
        "self-test: whitespace normalisation MUST make the whole-carrier check robust to \
         a reformat (line breaks / spacing around `::` and `;`)",
    );

    // --- ANCHORING: `return Err(...)` and `RouteGenerationDependency`
    //     in different expressions MUST NOT satisfy the predicate.
    // The substring `return Err` appears in one expression (a
    // generic Err) and `RouteGenerationDependency` appears as a
    // string-literal or unrelated identifier elsewhere. A
    // substring-AND check would falsely accept this body; the
    // anchored regex
    // `return Err\s*\(\s*[^)]*RouteGenerationDependency` rejects it
    // by requiring the discriminator to live INSIDE the same
    // `Err(...)` parenthesis group.
    let return_err_unrelated_mention = "fn p() { \
        for v in fence { \
            match v { \
                DepVersion::RouteGeneration(_) => { return Err(GenericError); } \
                _ => {} \
            } \
        } \
        let label = \"RouteGenerationDependency\"; \
        let _ = label; \
        Ok(out) \
    }";
    assert!(
        !body_has_whole_carrier_route_refusal(return_err_unrelated_mention),
        "self-test: `return Err(GenericError)` paired with an \
         unrelated `RouteGenerationDependency` mention elsewhere in \
         the body MUST NOT satisfy the predicate — the anchored regex \
         requires `RouteGenerationDependency` to live INSIDE the same \
         `Err(...)` parenthesis as the `return`. A substring-AND check \
         would falsely accept this shape; the anchored predicate rejects \
         it.",
    );
    // Same anchoring for the SignatureAdmission shape.
    let return_sig_admission_unrelated_mention = "fn p() { \
        for v in fence { \
            match v { \
                DepVersion::RouteGeneration(_) => { return SignatureAdmission::NonCacheable(OtherReason); } \
                _ => {} \
            } \
        } \
        let label = \"RouteGenerationDependency\"; \
        let _ = label; \
        SignatureAdmission::Cacheable(out) \
    }";
    assert!(
        !body_has_whole_carrier_route_refusal(return_sig_admission_unrelated_mention),
        "self-test: `return SignatureAdmission::NonCacheable(OtherReason)` \
         paired with an unrelated `RouteGenerationDependency` mention \
         elsewhere MUST NOT satisfy the predicate — anchored regex.",
    );

    // --- COMMENT STRIPPING: a comment that mentions
    //     `RouteGenerationDependency` cannot satisfy a code predicate. ---
    let comment_only_mention = "fn p() { \
        for v in fence { \
            match v { \
                DepVersion::RouteGeneration(_) => { return Err(GenericError); } \
                _ => {} \
            } \
        } \
        // see RouteGenerationDependency for the rail shape \
        Ok(out) \
    }";
    assert!(
        !body_has_whole_carrier_route_refusal(comment_only_mention),
        "self-test: a `// comment` that incidentally mentions \
         `RouteGenerationDependency` MUST be stripped before the \
         predicate runs — a code-level `Err(GenericError)` paired with \
         a comment-only mention does NOT satisfy the typed-reason \
         contract.",
    );
    let block_comment_only_mention = "fn p() { \
        for v in fence { \
            match v { \
                DepVersion::RouteGeneration(_) => { return Err(GenericError); } \
                _ => {} \
            } \
        } \
        /* RouteGenerationDependency description here */ \
        Ok(out) \
    }";
    assert!(
        !body_has_whole_carrier_route_refusal(block_comment_only_mention),
        "self-test: a `/* block comment */` that incidentally \
         mentions `RouteGenerationDependency` MUST also be stripped — \
         block comments and line comments are both lexically removed \
         before the structural predicate applies.",
    );

    // --- Positive case: anchored regex must still accept the
    //     well-formed typed refusal. ---
    let return_err_anchored = "fn p() { \
        for v in fence { \
            match v { \
                DepVersion::RouteGeneration(_) => { \
                    return Err(NonAdmissionReason::RouteGenerationDependency); \
                } \
                _ => {} \
            } \
        } \
        Ok(out) \
    }";
    assert!(
        body_has_whole_carrier_route_refusal(return_err_anchored),
        "self-test: a well-formed `return Err(NonAdmissionReason::\
         RouteGenerationDependency)` — typed reason INSIDE the Err(...) \
         parenthesis — MUST satisfy the anchored regex.",
    );
    let return_sig_admission_anchored = "fn p() { \
        for v in fence { \
            match v { \
                DepVersion::RouteGeneration(_) => { \
                    return SignatureAdmission::NonCacheable(NonAdmissionReason::RouteGenerationDependency); \
                } \
                _ => {} \
            } \
        } \
        SignatureAdmission::Cacheable(out) \
    }";
    assert!(
        body_has_whole_carrier_route_refusal(return_sig_admission_anchored),
        "self-test: a well-formed `return SignatureAdmission::\
         NonCacheable(NonAdmissionReason::RouteGenerationDependency)` \
         MUST satisfy the anchored regex.",
    );

    // Sanity: the scanned producers exist.
    assert!(
        read_session_source("component_meta_materialize.rs")
            .contains("pub fn fact_signature_from_fence("),
        "fact_signature_from_fence must be present in component_meta_materialize.rs",
    );
    assert!(
        read_session_source("fact_signature_helpers.rs")
            .contains("pub(crate) fn dep_signature_to_fact_signature("),
        "dep_signature_to_fact_signature must be present in fact_signature_helpers.rs",
    );
}
