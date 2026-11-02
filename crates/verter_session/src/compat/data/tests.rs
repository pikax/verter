use std::path::PathBuf;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use super::snapshot::{
    BaselineEvidence, BaselineStatus, CompatSnapshot, NeedSnapshotInput, RecordAudit,
    ReleaseStatus, SnapshotInput, SnapshotPin, SnapshotRejection, SnapshotState, SupportAnswer,
    WebFeatureAnswer, WebFeatureMembers,
};
use super::support::{
    BrowserTarget, BrowserVersion, ConditionKind, FlagCondition, IndeterminateBoundary,
    IntervalStatus, SchemaMismatch, SinceBound, SupportCondition, SupportVerdict, UnknownReason,
    VersionAdded,
};
use super::vocabulary::{
    browserslist_family, BrowserslistQueryForm, QueryAdmission, UnsupportedQueryReason,
};

fn corpus(relative: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-corpora/web-compat")
        .join(relative);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn fixture_bytes() -> Vec<u8> {
    corpus("fixtures/compat-snapshot.fixture.jsonl")
}

fn sha256_label(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

fn split_header(bytes: &[u8]) -> (serde_json::Value, &[u8]) {
    let end = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("header line");
    (
        serde_json::from_slice(&bytes[..end]).expect("header json"),
        &bytes[end + 1..],
    )
}

/// The identity law, restated independently of the loader so a change to either
/// side is caught.
fn reseal(mut header: serde_json::Value, payload: &[u8]) -> Vec<u8> {
    header["payload_digest"] = sha256_label(payload).into();
    let text = |value: &serde_json::Value| value.as_str().expect("string field").to_owned();
    let mut parts = vec![
        "verter-compat-snapshot-id".to_owned(),
        header["format_version"].to_string(),
    ];
    for dataset in header["datasets"].as_array().expect("datasets") {
        for field in [
            "name",
            "release",
            "integrity",
            "schema_digest",
            "license_digest",
        ] {
            parts.push(text(&dataset[field]));
        }
    }
    parts.push(text(&header["recipe_digest"]));
    parts.push(text(&header["payload_digest"]));
    header["snapshot_id"] = sha256_label(parts.join("\0").as_bytes()).into();
    let mut sealed = serde_json::to_vec(&header).expect("header bytes");
    sealed.push(b'\n');
    sealed.extend_from_slice(payload);
    sealed
}

fn pin_of(bytes: &[u8]) -> SnapshotPin {
    let (header, _) = split_header(bytes);
    SnapshotPin::exact(header["snapshot_id"].as_str().expect("snapshot id"))
}

fn state_of(bytes: Vec<u8>) -> SnapshotState {
    let pin = pin_of(&bytes);
    SnapshotState::resolve(SnapshotInput::Bytes(Arc::from(bytes)), &pin)
}

fn ready(bytes: Vec<u8>) -> Box<CompatSnapshot> {
    match state_of(bytes) {
        SnapshotState::Ready(snapshot) => snapshot,
        other => panic!("expected an admitted snapshot, got {other:?}"),
    }
}

fn rejection_of(bytes: Vec<u8>, pin: &SnapshotPin) -> SnapshotRejection {
    match SnapshotState::resolve(SnapshotInput::Bytes(Arc::from(bytes)), pin) {
        SnapshotState::Rejected(rejection) => rejection,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

fn target(family: &str, version: &str) -> BrowserTarget {
    BrowserTarget::exact(family, version).expect("exact target")
}

fn version(spelling: &str) -> BrowserVersion {
    BrowserVersion::parse(spelling).expect("version")
}

fn fixture() -> Box<CompatSnapshot> {
    ready(fixture_bytes())
}

fn answer(snapshot: &CompatSnapshot, feature: &str, family: &str, at: &str) -> SupportAnswer {
    snapshot.support(feature, &target(family, at))
}

fn replace_in_payload(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let (header, payload) = split_header(bytes);
    let text = std::str::from_utf8(payload).expect("utf-8 payload");
    assert_eq!(text.matches(from).count(), 1, "{from} must be unique");
    reseal(header, text.replace(from, to).as_bytes())
}

#[test]
fn a_snapshot_is_shareable_across_query_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SnapshotState>();
    assert_send_sync::<SupportAnswer>();
}

#[test]
fn an_answer_names_the_record_and_the_facts_it_came_from() {
    let snapshot = fixture();
    let got = answer(&snapshot, "api.Boundary", "chrome", "19");
    assert_eq!(got.feature, "api.Boundary");
    assert_eq!(got.target, target("chrome", "19"));
    assert_eq!(got.snapshot.as_ref(), Some(snapshot.identity()));
    let facts = got.facts.expect("a record was read");
    assert_eq!(
        facts.mdn_url.as_deref(),
        Some("https://developer.mozilla.org/docs/Web/API/Boundary")
    );
    assert_eq!(
        facts.spec_urls,
        vec!["https://example.test/spec#boundary".to_owned()]
    );
    assert_eq!(facts.deprecated, Some(true));
    assert_eq!(facts.experimental, Some(false));

    // A web-extensions record is schema-valid without a status block.
    let extension = answer(&snapshot, "webextensions.api.runtime", "firefox", "60");
    assert_eq!(extension.facts.expect("a record").deprecated, None);
}

#[test]
fn removal_is_an_exclusive_upper_bound() {
    let snapshot = fixture();
    let verdict = |at: &str| answer(&snapshot, "api.Boundary", "chrome", at).verdict;
    assert_eq!(verdict("9"), SupportVerdict::Unsupported);
    assert_eq!(
        verdict("10"),
        SupportVerdict::Supported {
            since: SinceBound::Exact(version("10"))
        }
    );
    assert_eq!(
        verdict("19"),
        SupportVerdict::Supported {
            since: SinceBound::Exact(version("10"))
        }
    );
    assert_eq!(verdict("20"), SupportVerdict::Unsupported);
    assert_eq!(verdict("21"), SupportVerdict::Unsupported);
}

#[test]
fn alternative_statements_are_evaluated_separately_and_all_retained() {
    let snapshot = fixture();

    // Inside only the prefixed statement's interval: the spelling is the fact.
    let prefixed = answer(&snapshot, "api.Boundary.alternatives", "chrome", "30");
    assert_eq!(
        prefixed.verdict,
        SupportVerdict::Conditional {
            kinds: vec![ConditionKind::Prefixed]
        }
    );
    assert_eq!(prefixed.statements.len(), 2);
    assert_eq!(
        prefixed.statements[0].interval,
        IntervalStatus::BeforeAddition
    );
    assert_eq!(
        prefixed.statements[1].conditions,
        vec![SupportCondition::Prefixed("webkit".to_owned())]
    );

    // The prefixed statement's removal does not hide the unprefixed one.
    let unprefixed = answer(&snapshot, "api.Boundary.alternatives", "chrome", "38");
    assert_eq!(
        unprefixed.verdict,
        SupportVerdict::Supported {
            since: SinceBound::Exact(version("38"))
        }
    );
    assert_eq!(
        unprefixed.statements[1].interval,
        IntervalStatus::AtOrAfterRemoval
    );

    // A condition on a statement outside its interval does not colour the answer.
    let before = answer(&snapshot, "api.Boundary.alternatives", "chrome", "19");
    assert_eq!(before.verdict, SupportVerdict::Unsupported);
    assert_eq!(before.statements.len(), 2);

    let aliased = answer(&snapshot, "api.Boundary.alternatives", "firefox", "50");
    assert_eq!(
        aliased.verdict,
        SupportVerdict::Conditional {
            kinds: vec![ConditionKind::Aliased]
        }
    );
    assert_eq!(
        aliased.statements[0].conditions,
        vec![SupportCondition::Aliased("legacyAlternatives".to_owned())]
    );
}

#[test]
fn flags_and_partial_limits_survive_as_distinct_facts() {
    let snapshot = fixture();
    let feature = "api.Conditions.flag_with_value";

    let flagged = answer(&snapshot, feature, "chrome", "30");
    assert_eq!(
        flagged.verdict,
        SupportVerdict::Conditional {
            kinds: vec![ConditionKind::Flagged]
        }
    );
    assert_eq!(
        flagged.statements[0].conditions,
        vec![SupportCondition::Flagged(FlagCondition {
            flag_type: "preference".to_owned(),
            name: "#enable-feature".to_owned(),
            value_to_set: Some("Enabled".to_owned()),
        })]
    );

    // A record that is both partial and flagged keeps both.
    let both = answer(&snapshot, feature, "firefox", "60");
    assert_eq!(
        both.verdict,
        SupportVerdict::Conditional {
            kinds: vec![ConditionKind::Partial, ConditionKind::Flagged]
        }
    );
    assert_eq!(
        both.statements[0].conditions,
        vec![
            SupportCondition::Partial {
                notes: vec!["Only for documents.".to_owned()]
            },
            SupportCondition::Flagged(FlagCondition {
                flag_type: "runtime_flag".to_owned(),
                name: "--enable-feature".to_owned(),
                value_to_set: None,
            }),
        ]
    );

    let partial = answer(&snapshot, feature, "safari", "15.4");
    assert_eq!(
        partial.statements[0].conditions,
        vec![SupportCondition::Partial {
            notes: vec!["First limit.".to_owned(), "Second limit.".to_owned()]
        }]
    );
}

#[test]
fn an_undecidable_boundary_stays_unknown_at_the_target() {
    use IndeterminateBoundary as Boundary;
    let snapshot = fixture();
    let cases: [(&str, &str, &str, SupportVerdict, Option<Boundary>); 11] = [
        // `true`: supported since an unnamed version proves nothing at this one.
        (
            "api.Uncertain.added_unknown_version",
            "chrome",
            "100",
            SupportVerdict::Unknown(UnknownReason::Indeterminate),
            Some(Boundary::AdditionVersionUnknown),
        ),
        // `null`: the data itself says support is unknown.
        (
            "api.Uncertain.added_unknown_version",
            "firefox",
            "100",
            SupportVerdict::Unknown(UnknownReason::Indeterminate),
            Some(Boundary::SupportUnknown),
        ),
        // `false`: an explicit negative is not uncertainty.
        (
            "api.Uncertain.added_unknown_version",
            "safari",
            "15.4",
            SupportVerdict::Unsupported,
            None,
        ),
        // `≤37`: certain from 37, uncertain below it, never unsupported.
        (
            "api.Uncertain.added_at_or_before",
            "chrome",
            "37",
            SupportVerdict::Supported {
                since: SinceBound::AtOrBefore(version("37")),
            },
            None,
        ),
        (
            "api.Uncertain.added_at_or_before",
            "chrome",
            "38",
            SupportVerdict::Supported {
                since: SinceBound::AtOrBefore(version("37")),
            },
            None,
        ),
        (
            "api.Uncertain.added_at_or_before",
            "chrome",
            "36",
            SupportVerdict::Unknown(UnknownReason::Indeterminate),
            Some(Boundary::AdditionAtOrBefore),
        ),
        // `preview`: not in a stable release, undecidable in a prerelease one.
        (
            "api.Uncertain.added_in_preview",
            "chrome",
            "100",
            SupportVerdict::Unsupported,
            None,
        ),
        (
            "api.Uncertain.added_in_preview",
            "chrome",
            "101",
            SupportVerdict::Unknown(UnknownReason::Indeterminate),
            Some(Boundary::AdditionInPreview),
        ),
        // Removal in an unnamed version: uncertain once added, unsupported before.
        (
            "api.Uncertain.removed_unknown_version",
            "chrome",
            "100",
            SupportVerdict::Unknown(UnknownReason::Indeterminate),
            Some(Boundary::RemovalVersionUnknown),
        ),
        (
            "api.Uncertain.removed_unknown_version",
            "chrome",
            "9",
            SupportVerdict::Unsupported,
            None,
        ),
        (
            "api.Uncertain.removed_unknown_version",
            "chrome",
            "10",
            SupportVerdict::Unknown(UnknownReason::Indeterminate),
            Some(Boundary::RemovalVersionUnknown),
        ),
    ];
    for (feature, family, at, expected, boundary) in cases {
        let got = answer(&snapshot, feature, family, at);
        assert_eq!(got.verdict, expected, "{feature} {family} {at}");
        if let Some(boundary) = boundary {
            assert_eq!(
                got.statements[0].interval,
                IntervalStatus::Indeterminate(boundary),
                "{feature} {family} {at}"
            );
        }
    }
    let retained = answer(
        &snapshot,
        "api.Uncertain.added_unknown_version",
        "chrome",
        "100",
    );
    assert_eq!(retained.statements[0].added, VersionAdded::UnknownVersion);
}

#[test]
fn baseline_status_never_answers_for_a_target() {
    let snapshot = fixture();
    let widely = BaselineEvidence {
        web_feature: "widely-available".to_owned(),
        status: BaselineStatus::Widely {
            low_date: Some("2015-07-29".to_owned()),
            high_date: Some("2018-01-29".to_owned()),
        },
    };
    let expectations = [
        ("chrome", "100", SupportVerdict::Unsupported),
        (
            "firefox",
            "100",
            SupportVerdict::Conditional {
                kinds: vec![ConditionKind::Flagged],
            },
        ),
        (
            "safari",
            "15.4",
            SupportVerdict::Conditional {
                kinds: vec![ConditionKind::Partial],
            },
        ),
    ];
    for (family, at, expected) in expectations {
        let got = answer(&snapshot, "api.WidelyAvailable", family, at);
        assert_eq!(got.verdict, expected, "{family} {at}");
        assert_eq!(got.baseline, vec![widely.clone()], "{family} {at}");
    }

    let mapped = answer(&snapshot, "api.Boundary", "chrome", "19");
    assert_eq!(
        mapped.baseline,
        vec![
            BaselineEvidence {
                web_feature: "limited-availability".to_owned(),
                status: BaselineStatus::Limited,
            },
            BaselineEvidence {
                web_feature: "newly-available".to_owned(),
                status: BaselineStatus::Newly {
                    low_date: Some("2025-01-01".to_owned()),
                },
            },
        ]
    );
}

#[test]
fn missing_data_stays_unknown_and_names_what_is_absent() {
    let snapshot = fixture();
    let unknown = |feature: &str, family: &str, at: &str| {
        let got = answer(&snapshot, feature, family, at);
        assert!(got.statements.is_empty(), "{feature} {family} {at}");
        assert!(got.snapshot.is_some(), "the answering data stays named");
        got.verdict
    };
    assert_eq!(
        unknown("api.NotInThisDataset", "chrome", "100"),
        SupportVerdict::Unknown(UnknownReason::FeatureAbsent)
    );
    // `api.Boundary` was added at 10, yet a release the data has never heard of
    // proves nothing.
    assert_eq!(
        unknown("api.Boundary.alternatives", "chrome", "999"),
        SupportVerdict::Unknown(UnknownReason::TargetVersionAbsent)
    );
    assert_eq!(
        unknown("api.Boundary", "netscape", "4"),
        SupportVerdict::Unknown(UnknownReason::TargetFamilyAbsent)
    );
    // Another browser's record does not stand in for this one's.
    assert_eq!(
        unknown("api.Boundary", "firefox", "100"),
        SupportVerdict::Unknown(UnknownReason::NoRecordForFamily)
    );
}

#[test]
fn a_schema_mismatch_answers_unknown_without_disturbing_its_neighbours() {
    let snapshot = fixture();
    let feature = "api.Mismatch.one_browser";
    assert_eq!(
        answer(&snapshot, feature, "chrome", "100").verdict,
        SupportVerdict::Unknown(UnknownReason::SchemaMismatch(SchemaMismatch {
            instance_path: "/support/chrome".to_owned(),
            keyword: "anyOf".to_owned(),
        }))
    );
    assert!(matches!(
        answer(&snapshot, feature, "oculus", "23.0").verdict,
        SupportVerdict::Unknown(UnknownReason::SchemaMismatch(_))
    ));
    assert_eq!(
        answer(&snapshot, feature, "firefox", "100").verdict,
        SupportVerdict::Supported {
            since: SinceBound::Exact(version("50"))
        }
    );
    assert_eq!(
        answer(&snapshot, "api.Mismatch.whole_record", "chrome", "100").verdict,
        SupportVerdict::Unknown(UnknownReason::SchemaMismatch(SchemaMismatch {
            instance_path: "/mdn_url".to_owned(),
            keyword: "pattern".to_owned(),
        }))
    );
    assert_eq!(
        answer(&snapshot, "api.Mismatch.mirrored", "chrome", "100").verdict,
        SupportVerdict::Unknown(UnknownReason::SchemaMismatch(SchemaMismatch {
            instance_path: "/support/chrome".to_owned(),
            keyword: "mirror-unresolved".to_owned(),
        }))
    );
}

#[test]
fn records_are_parsed_on_demand_so_one_bad_record_is_contained() {
    // Loading succeeding proves no feature record was parsed during it.
    let bytes = replace_in_payload(
        &fixture_bytes(),
        "bcd\tapi.Boundary\t{",
        "bcd\tapi.Boundary\t{\"unexpected_field\":1,",
    );
    let snapshot = ready(bytes);
    assert!(matches!(
        answer(&snapshot, "api.Boundary", "chrome", "19").verdict,
        SupportVerdict::Unknown(UnknownReason::SchemaMismatch(SchemaMismatch { keyword, .. }))
            if keyword == "snapshot-format"
    ));
    assert_eq!(
        answer(&snapshot, "api.Boundary.alternatives", "chrome", "38").verdict,
        SupportVerdict::Supported {
            since: SinceBound::Exact(version("38"))
        }
    );

    // A version spelling outside the format never reaches a comparison.
    let bytes = replace_in_payload(
        &fixture_bytes(),
        "\"version_added\":\"≤37\"",
        "\"version_added\":\"≤latest\"",
    );
    assert!(matches!(
        answer(
            &ready(bytes),
            "api.Uncertain.added_at_or_before",
            "chrome",
            "100"
        )
        .verdict,
        SupportVerdict::Unknown(UnknownReason::SchemaMismatch(_))
    ));
}

#[test]
fn the_snapshot_input_state_is_never_an_empty_success() {
    let pin = SnapshotPin::admitted();
    let chrome = target("chrome", "100");

    let not_supplied = SnapshotState::resolve(SnapshotInput::NotSupplied, &pin);
    assert_eq!(
        not_supplied.support("api.Boundary", &chrome),
        Err(NeedSnapshotInput)
    );

    let absent = SnapshotState::resolve(SnapshotInput::ProvenAbsent, &pin);
    assert_eq!(
        absent
            .support("api.Boundary", &chrome)
            .expect("an answer")
            .verdict,
        SupportVerdict::Unknown(UnknownReason::SnapshotAbsent)
    );

    let rejected = SnapshotState::resolve(SnapshotInput::Bytes(Arc::from(&b"junk"[..])), &pin);
    assert_eq!(
        rejected
            .support("api.Boundary", &chrome)
            .expect("an answer")
            .verdict,
        SupportVerdict::Unknown(UnknownReason::SnapshotRejected)
    );
}

#[test]
fn changed_bytes_and_unadmitted_identities_are_rejected() {
    let bytes = fixture_bytes();
    let pin = pin_of(&bytes);
    let (header, payload) = split_header(&bytes);

    assert_eq!(
        rejection_of(b"no newline".to_vec(), &pin),
        SnapshotRejection::MissingHeader
    );

    // One flipped payload byte.
    let mut flipped = bytes.clone();
    let last = flipped.len() - 2;
    flipped[last] ^= 1;
    assert!(matches!(
        rejection_of(flipped, &pin),
        SnapshotRejection::PayloadDigestMismatch { .. }
    ));

    // A truncated snapshot.
    assert!(matches!(
        rejection_of(bytes[..bytes.len() - 100].to_vec(), &pin),
        SnapshotRejection::PayloadDigestMismatch { .. }
    ));

    // A CRLF checkout changes every line.
    let crlf: Vec<u8> = bytes
        .split_inclusive(|byte| *byte == b'\n')
        .flat_map(|line| {
            let body = line.strip_suffix(b"\n").unwrap_or(line);
            [body, b"\r\n"].concat()
        })
        .collect();
    assert!(matches!(
        rejection_of(crlf, &pin),
        SnapshotRejection::MissingHeader
            | SnapshotRejection::HeaderMismatch { .. }
            | SnapshotRejection::PayloadDigestMismatch { .. }
    ));

    // A self-consistent snapshot of other data is not the pinned one.
    let edited = replace_in_payload(
        &bytes,
        "\"version_removed\":\"20\"",
        "\"version_removed\":\"21\"",
    );
    assert!(matches!(
        rejection_of(edited, &pin),
        SnapshotRejection::UnpinnedSnapshot { .. }
    ));

    // A declared identity that does not derive from the header.
    let mut forged = header.clone();
    forged["snapshot_id"] = "sha256:forged".into();
    let mut forged_bytes = serde_json::to_vec(&forged).expect("header");
    forged_bytes.push(b'\n');
    forged_bytes.extend_from_slice(payload);
    assert!(matches!(
        rejection_of(forged_bytes, &SnapshotPin::exact("sha256:forged")),
        SnapshotRejection::IdentityMismatch { .. }
    ));

    // A future format is refused, not read as this one.
    let mut future = header.clone();
    future["format_version"] = 2.into();
    let future_bytes = reseal(future, payload);
    let future_pin = pin_of(&future_bytes);
    assert!(matches!(
        rejection_of(future_bytes, &future_pin),
        SnapshotRejection::UnsupportedFormat { version: 2, .. }
    ));

    // A header carrying a field this format does not define.
    let mut extended = header.clone();
    extended["unexpected"] = true.into();
    let extended_bytes = reseal(extended, payload);
    let extended_pin = pin_of(&extended_bytes);
    assert!(matches!(
        rejection_of(extended_bytes, &extended_pin),
        SnapshotRejection::HeaderMismatch { .. }
    ));
}

#[test]
fn a_structurally_invalid_payload_is_rejected_whole() {
    let bytes = fixture_bytes();
    let (header, payload) = split_header(&bytes);
    let text = std::str::from_utf8(payload).expect("utf-8");
    let lines: Vec<&str> = text.lines().collect();
    let first_bcd = lines
        .iter()
        .position(|line| line.starts_with("bcd\t"))
        .expect("a bcd record");
    let rejection = |mutated: Vec<&str>| {
        let mut payload = mutated.join("\n");
        payload.push('\n');
        let sealed = reseal(header.clone(), payload.as_bytes());
        let pin = pin_of(&sealed);
        rejection_of(sealed, &pin)
    };

    // Two records out of order would make binary search miss one silently.
    let mut swapped = lines.clone();
    swapped.swap(first_bcd, first_bcd + 1);
    assert!(matches!(
        rejection(swapped),
        SnapshotRejection::Layout { .. }
    ));

    let mut duplicated = lines.clone();
    duplicated.insert(first_bcd, lines[first_bcd]);
    assert!(matches!(
        rejection(duplicated),
        SnapshotRejection::Layout { .. }
    ));

    // Dropping a record is caught even though the remaining order is valid.
    let mut dropped = lines.clone();
    dropped.remove(first_bcd);
    assert!(matches!(
        rejection(dropped),
        SnapshotRejection::Layout { .. }
    ));

    let mut no_id = lines.clone();
    no_id[first_bcd] = "bcd";
    assert!(matches!(rejection(no_id), SnapshotRejection::Layout { .. }));

    let mut unterminated_payload = text.to_owned();
    unterminated_payload.pop();
    let sealed = reseal(header.clone(), unterminated_payload.as_bytes());
    let pin = pin_of(&sealed);
    assert!(matches!(
        rejection_of(sealed, &pin),
        SnapshotRejection::Layout { .. }
    ));

    // Browser release data is what every answer checks a target against.
    let browser = replace_in_payload(&bytes, "\"status\":\"esr\"", "\"status\":\"stable\"");
    let pin = pin_of(&browser);
    assert!(matches!(
        rejection_of(browser, &pin),
        SnapshotRejection::BrowserRecord { family, .. } if family == "firefox"
    ));
}

#[test]
fn identity_follows_content_so_an_edit_and_its_revert_are_exact() {
    let original = fixture_bytes();
    let edited = replace_in_payload(
        &original,
        "\"version_removed\":\"20\"",
        "\"version_removed\":\"21\"",
    );
    let reverted = replace_in_payload(
        &edited,
        "\"version_removed\":\"21\"",
        "\"version_removed\":\"20\"",
    );
    assert_ne!(pin_of(&original), pin_of(&edited));
    assert_eq!(pin_of(&original), pin_of(&reverted));

    let (first, second) = (fixture(), ready(edited));
    assert_ne!(first.identity(), second.identity());
    assert_eq!(
        answer(&first, "api.Boundary", "chrome", "20").verdict,
        SupportVerdict::Unsupported
    );
    assert_eq!(
        answer(&second, "api.Boundary", "chrome", "20").verdict,
        SupportVerdict::Supported {
            since: SinceBound::Exact(version("10"))
        }
    );

    // A fresh load answers identically to one that already answered, and names
    // the same data.
    let warm = fixture();
    let _ = answer(&warm, "api.Boundary.alternatives", "chrome", "30");
    let cold = fixture();
    assert_eq!(
        answer(&warm, "api.Boundary", "chrome", "19"),
        answer(&cold, "api.Boundary", "chrome", "19")
    );
    assert_eq!(cold.identity().datasets.len(), 2);
}

#[test]
fn web_feature_groupings_keep_baseline_apart_from_support() {
    let snapshot = fixture();
    let chrome = target("chrome", "100");

    match snapshot.web_feature("baseline-without-mapping", &chrome) {
        WebFeatureAnswer::Feature {
            baseline, members, ..
        } => {
            assert!(matches!(baseline, BaselineStatus::Widely { .. }));
            assert_eq!(members, WebFeatureMembers::NoCompatMapping);
        }
        other => panic!("unexpected {other:?}"),
    }

    match snapshot.web_feature("baseline-missing-record", &chrome) {
        WebFeatureAnswer::Feature {
            baseline, members, ..
        } => {
            assert!(matches!(baseline, BaselineStatus::Widely { .. }));
            let WebFeatureMembers::Members(members) = members else {
                panic!("the mapping exists");
            };
            assert_eq!(members.len(), 1);
            assert_eq!(members[0].0, "api.NotInThisDataset");
            assert_eq!(
                members[0].1.verdict,
                SupportVerdict::Unknown(UnknownReason::FeatureAbsent)
            );
        }
        other => panic!("unexpected {other:?}"),
    }

    match snapshot.web_feature("widely-available", &chrome) {
        WebFeatureAnswer::Feature {
            name,
            groups,
            members,
            ..
        } => {
            assert_eq!(name, "Widely available");
            assert_eq!(groups, vec!["fixture-group".to_owned()]);
            let WebFeatureMembers::Members(members) = members else {
                panic!("the mapping exists");
            };
            assert_eq!(members[0].1.verdict, SupportVerdict::Unsupported);
        }
        other => panic!("unexpected {other:?}"),
    }

    assert_eq!(
        snapshot.web_feature("moved-feature", &chrome),
        WebFeatureAnswer::Moved {
            target: "widely-available".to_owned()
        }
    );
    assert_eq!(
        snapshot.web_feature("split-feature", &chrome),
        WebFeatureAnswer::Split {
            targets: vec!["widely-available".to_owned(), "newly-available".to_owned()]
        }
    );
    assert_eq!(
        snapshot.web_feature("no-such-feature", &chrome),
        WebFeatureAnswer::Absent
    );
    assert!(matches!(
        snapshot.web_feature("malformed-feature", &chrome),
        WebFeatureAnswer::Rejected(SchemaMismatch { keyword, .. }) if keyword == "oneOf"
    ));
}

#[test]
fn a_target_is_one_exact_version_compared_numerically() {
    for spelling in [
        "90-100",
        "latest",
        "≤37",
        ">=100",
        "",
        "1.",
        ".1",
        "1..2",
        "99999999999",
    ] {
        assert!(
            BrowserTarget::exact("chrome", spelling).is_err(),
            "{spelling:?}"
        );
    }
    assert!(BrowserTarget::exact("", "100").is_err());

    assert!(version("1.10") > version("1.5"));
    assert!(version("10") > version("9"));
    assert_eq!(version("15.0"), version("15"));

    // Numeric, not lexical: 1.10 is inside an interval starting at 1.5.
    let snapshot = fixture();
    assert_eq!(
        answer(
            &snapshot,
            "api.Conditions.flag_with_value",
            "safari",
            "1.10"
        )
        .verdict,
        SupportVerdict::Conditional {
            kinds: vec![ConditionKind::Partial]
        }
    );

    let safari = snapshot.browser("safari").expect("safari");
    let ascending: Vec<&str> = safari
        .releases
        .iter()
        .map(|release| release.version.as_str())
        .collect();
    assert_eq!(ascending, ["1.5", "1.10", "15.4"]);
    assert_eq!(
        snapshot.browser("firefox").expect("firefox").releases[1].status,
        ReleaseStatus::Esr
    );
    let families: Vec<&str> = snapshot.browsers().map(|(family, _)| family).collect();
    assert_eq!(families, ["chrome", "firefox", "oculus", "safari"]);
}

#[test]
fn the_query_vocabulary_refuses_what_the_snapshot_cannot_answer() {
    use BrowserslistQueryForm as Form;
    use UnsupportedQueryReason as Reason;
    for form in [
        Form::ExactVersion,
        Form::LastVersions,
        Form::FirefoxEsr,
        Form::SinceDate,
    ] {
        assert_eq!(form.admission(), QueryAdmission::Admitted, "{form:?}");
    }
    for (form, reason) in [
        (Form::Defaults, Reason::UsageStatisticNotCarried),
        (Form::GlobalUsage, Reason::UsageStatisticNotCarried),
        (Form::Coverage, Reason::UsageStatisticNotCarried),
        (Form::RegionalUsage, Reason::RegionalStatisticNotCarried),
        (Form::Dead, Reason::DeadListNotCarried),
        (Form::LastYears, Reason::ClockDependent),
        (Form::SupportsFeature, Reason::CaniuseNotAdmitted),
        (Form::NodeRuntime, Reason::NodeReleaseScheduleNotCarried),
        (Form::Electron, Reason::ElectronMappingNotCarried),
        (Form::Extends, Reason::ResolvedByProjectModel),
    ] {
        assert_eq!(
            form.admission(),
            QueryAdmission::Unsupported(reason),
            "{form:?}"
        );
    }

    assert_eq!(browserslist_family("iOS"), Some("safari_ios"));
    assert_eq!(browserslist_family("and_chr"), Some("chrome_android"));
    assert_eq!(browserslist_family("op_mini"), None);
}

#[test]
fn the_admitted_snapshot_answers_from_the_pinned_upstream_records() {
    let bytes = corpus("pinned/compat-snapshot.jsonl");
    let state = SnapshotState::resolve(
        SnapshotInput::Bytes(Arc::from(bytes)),
        &SnapshotPin::admitted(),
    );
    let snapshot = match state {
        SnapshotState::Ready(snapshot) => snapshot,
        other => panic!("the committed snapshot must be the admitted one: {other:?}"),
    };

    let releases: Vec<(&str, &str)> = snapshot
        .identity()
        .datasets
        .iter()
        .map(|dataset| (dataset.name.as_str(), dataset.release.as_str()))
        .collect();
    assert_eq!(
        releases,
        [("browser-compat-data", "5.7.6"), ("web-features", "3.44.0")]
    );

    let fetch = answer(&snapshot, "api.fetch", "chrome", "100");
    assert_eq!(
        fetch.verdict,
        SupportVerdict::Supported {
            since: SinceBound::Exact(version("42"))
        }
    );
    assert!(!fetch.baseline.is_empty());
    assert_eq!(
        answer(&snapshot, "api.fetch", "chrome", "41").verdict,
        SupportVerdict::Unsupported
    );
    assert_eq!(
        answer(&snapshot, "api.fetch", "ie", "11").verdict,
        SupportVerdict::Unsupported
    );

    // The upstream record violating its own schema answers unknown for that
    // browser and normally for the others.
    let quarantined = "api.CanvasFilter.CanvasFilter";
    assert!(matches!(
        answer(&snapshot, quarantined, "oculus", "23.0").verdict,
        SupportVerdict::Unknown(UnknownReason::SchemaMismatch(_))
    ));
    assert_eq!(
        answer(&snapshot, quarantined, "chrome", "112").verdict,
        SupportVerdict::Supported {
            since: SinceBound::Exact(version("99"))
        }
    );

    // Every record of the pinned data is readable; the only unknowns the format
    // produces are the upstream schema failures generation recorded.
    let pinned_bytes = corpus("pinned/compat-snapshot.jsonl");
    let (header, _) = split_header(&pinned_bytes);
    let count = |name: &str| header["counts"][name].as_u64().expect("count") as usize;
    assert_eq!(
        snapshot.audit_records(),
        RecordAudit {
            features: count("bcd_features"),
            rejected_features: count("bcd_rejected_features"),
            rejected_families: count("bcd_rejected_families"),
            web_features: count("web_features"),
            format_mismatches: 0,
        }
    );

    // The dataset never releases a version past its pin.
    assert_eq!(
        answer(&snapshot, "api.fetch", "chrome", "9999").verdict,
        SupportVerdict::Unknown(UnknownReason::TargetVersionAbsent)
    );
    assert_eq!(
        answer(&snapshot, "api.not_a_real_feature", "chrome", "100").verdict,
        SupportVerdict::Unknown(UnknownReason::FeatureAbsent)
    );
}
