//! Committed Svelte golden inventory and the shipped `.svelte` route.
//! Same `goldens/manifest.json` and digest helpers as
//! [`bf2_seed_matrix`](super::bf2_seed_matrix) — no second verification
//! copy. Record projection differs (`generate` / `runes` / `dev`).
//!
//! Inventory: every `svelte/` entry hashes to the manifest digest;
//! fixtures are byte-identical (CRLF-normalised) to the hashed source;
//! client and server inventories are separate.
//!
//! Honest compile-option mapping: only `generate` →
//! [`CompileProfile::ssr`]. `runes` has no option (`runes: None`, inferred
//! from source — checked by
//! [`the_recorded_runes_axis_matches_what_the_shipped_route_infers`]).
//! `dev` has no option (`dev_codegen: false`); both `dev` arms drive the
//! same request. [`SvelteCell::dev`] is retained so reports match the
//! golden, not a pretend-different request.
//!
//! `cargo test -p verter_session --lib --features bf2-authoritative
//! svelte_official_conformance -- --nocapture`.
//!
//! Without the feature this module is not compiled. Read the
//! `running N tests` line, never the exit code.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::bf2_seed_matrix::{bool_member, harness_root, read_json, sha256_hex, str_member};
use super::*;

use crate::{HostConfig, HostError, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery};

/// Which backend the golden record asked the official compiler for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SvelteGenerate {
    Client,
    Server,
}

impl SvelteGenerate {
    fn parse(raw: &str) -> Self {
        match raw {
            "client" => Self::Client,
            "server" => Self::Server,
            other => panic!("the locked manifest names an unknown Svelte target `{other}`"),
        }
    }
}

/// The value of the `dev` axis that every public/default request implicitly
/// carries.
///
/// NOT a hand-declared constant: it is the axis value the shipped route always
/// issues, and [`the_dev_axis_has_no_public_spelling_and_only_its_implicit_value_is_reachable`]
/// proves both halves — that no public request can vary it, and that the OTHER
/// value is refused by the compiler entry one layer down while this one
/// compiles.
pub(super) const DEV_AXIS_IMPLICIT_VALUE: bool = false;

/// Whether a committed golden describes a request a public/default route can
/// actually issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RequestReachability {
    /// Every recorded option axis is expressible by a public request.
    Reachable,
    /// A recorded axis has NO public spelling, and the recorded value is not the
    /// one every public request implicitly carries — so no caller can ask for
    /// this golden's request at all. Such a golden is out of the reachable
    /// inventory by construction; it is not a failing cell.
    NoPublicSpelling {
        axis: &'static str,
        recorded: bool,
        implicit: bool,
    },
}

/// One Svelte conformance cell, exactly as the committed manifest defines it.
#[derive(Debug, Clone)]
pub(super) struct SvelteCell {
    /// The manifest's logical name, e.g. `svelte/basic-runes__client__runes1__dev0`.
    pub(super) golden_name: String,
    /// The record's own fixture path, harness-root-relative, e.g.
    /// `fixtures/svelte/basic-runes.svelte`.
    pub(super) fixture_path: String,
    /// The authored source the record hashes, already verified against disk.
    pub(super) source: String,
    pub(super) generate: SvelteGenerate,
    /// The record's `options.runes` REQUEST axis. Not expressible as a
    /// production compile option — see the module doc.
    pub(super) runes: bool,
    /// The record's `options.dev` REQUEST axis. Not expressible as a production
    /// compile option — see the module doc.
    pub(super) dev: bool,
}

impl SvelteCell {
    /// The production compile profile this cell's axes map onto.
    ///
    /// `filename` is deliberately left `None`: the host derives it as
    /// `profile.filename.clone().or_else(|| Some(snapshot.canonical_id.clone()))`
    /// (`virtual_file_pipeline.rs:2855-2858`), and the canonical id used by
    /// [`Self::compile_through_shipped_route`] is the record's own fixture path,
    /// whose basename is what the emitted map names
    /// (`crates/verter_compiler/src/svelte/runtime/output.rs:203`).
    ///
    /// `source_map: true` is the golden generator's own axis: every Svelte
    /// record is produced with `sourceMap: true`
    /// (`packages/framework-conformance-harness/bin/generate-goldens.mjs:196`),
    /// and the harness's mapping axis reads `sourceMapRequested: true` for every
    /// Svelte golden (`src/check-candidate.mjs:97`).
    pub(super) fn compile_profile(&self) -> CompileProfile {
        CompileProfile {
            ssr: self.generate == SvelteGenerate::Server,
            source_map: true,
            ..CompileProfile::default()
        }
    }

    /// Whether a public/default route can issue this cell's recorded request.
    ///
    /// DERIVED from the per-axis expressibility this module proves, never a
    /// hand-listed cell set:
    ///
    /// * `generate` IS expressible — it is [`CompileProfile::ssr`], which the
    ///   host threads into the neutral `RuntimeCompileOptions.ssr`
    ///   (`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2862`).
    ///   Both of its values are therefore reachable requests.
    /// * `runes` is not a request axis at all: the carrier hardcodes
    ///   `runes: None` (`crates/verter_compiler/src/svelte/carrier.rs:369`) and
    ///   the mode is inferred from the source. It constrains no cell, and
    ///   [`the_recorded_runes_axis_matches_what_the_shipped_route_infers`]
    ///   proves the inference agrees with every recorded value.
    /// * `dev` has NO public spelling on any transport, so only its implicit
    ///   value ([`DEV_AXIS_IMPLICIT_VALUE`]) is reachable.
    pub(super) fn reachability(&self) -> RequestReachability {
        if self.dev != DEV_AXIS_IMPLICIT_VALUE {
            return RequestReachability::NoPublicSpelling {
                axis: "dev",
                recorded: self.dev,
                implicit: DEV_AXIS_IMPLICIT_VALUE,
            };
        }
        RequestReachability::Reachable
    }

    /// Drive this cell through the genuine shipped `.svelte` route and report
    /// what production returns.
    pub(super) fn compile_through_shipped_route(&self) -> SvelteRouteOutcome {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(self.fixture_path.clone()),
                input_id: self.fixture_path.clone(),
                source: Arc::from(self.source.as_str()),
                file_language: verter_language::FileLanguage::svelte(),
                aliases: Vec::new(),
            })
            .unwrap_or_else(|error| panic!("{}: upsert failed: {error:?}", self.golden_name));

        match host.get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(self.fixture_path.clone()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: self.compile_profile(),
        }) {
            Ok(response) => SvelteRouteOutcome::Emitted {
                code: response.code.to_string(),
                source_map: response.source_map.as_ref().map(|map| map.to_string()),
            },
            Err(HostError::RuntimeSurfaceRefused {
                diagnostic_code,
                message,
                ..
            }) => SvelteRouteOutcome::Refused {
                diagnostic_code,
                message,
            },
            Err(HostError::MissingVirtualNode { .. }) => SvelteRouteOutcome::MissingNode,
            Err(other) => panic!(
                "{}: the shipped route failed with an outcome this inventory does not model: \
                 {other:?}",
                self.golden_name
            ),
        }
    }
}

/// What the shipped `.svelte` route returns for one cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SvelteRouteOutcome {
    /// A runtime `Main` module was produced.
    Emitted {
        code: String,
        source_map: Option<String>,
    },
    /// The carrier explicitly refused the runtime surface, with its typed
    /// per-surface code and message.
    Refused {
        diagnostic_code: String,
        message: String,
    },
    /// No `Main` node and no refusal signal — a shape neither the client nor
    /// the server arm is expected to produce.
    MissingNode,
}

/// The pinned official Svelte package version every committed golden was
/// generated against.
///
/// The committed golden RECORDS are the authority reachable from here — each is
/// digest-verified against the manifest before it is read — so
/// [`pinned_svelte_domain`] derives the value from them and
/// [`every_committed_golden_names_the_same_pinned_official_domain`] additionally
/// pins that derived value, so a wholesale re-pin of all twelve records is still
/// caught rather than silently followed.
pub(super) const SVELTE_PINNED_PACKAGE_VERSION: &str = "5.56.10";

/// The pinned official domain the committed Svelte goldens agree on.
///
/// Every record must name the SAME domain; a manifest whose records straddle
/// two pins is not one domain and fails here.
pub(super) fn pinned_svelte_domain() -> Value {
    let mut domains: Vec<Value> = read_svelte_conformance_matrix()
        .iter()
        .map(|cell| {
            read_svelte_golden_record(&cell.golden_name)
                .get("domain")
                .cloned()
                .unwrap_or_else(|| panic!("{}: the record names no domain", cell.golden_name))
        })
        .collect();
    let first = domains.pop().expect("the manifest holds Svelte cells");
    for domain in &domains {
        assert_eq!(
            domain, &first,
            "the committed Svelte goldens do not all name the same official domain"
        );
    }
    first
}

/// Read ONE golden record by its logical name, verifying its digest.
///
/// The record is content-addressed by the digest the manifest lists, and that
/// digest is verified here — a record whose bytes do not hash to its manifest
/// name is not the locked record.
pub(super) fn read_svelte_golden_record(golden_name: &str) -> Value {
    let goldens = harness_root().join("goldens");
    let manifest = read_json(&goldens.join("manifest.json"));
    let digest = str_member(&manifest, &["entries", golden_name], golden_name);
    let record_path = goldens.join("records").join(format!("{digest}.json"));
    let record_bytes = std::fs::read(&record_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", record_path.display()));
    let actual = sha256_hex(&record_bytes);
    assert_eq!(
        actual,
        digest,
        "{golden_name}: the record at {} hashes to {actual}, not the digest the manifest names",
        record_path.display()
    );
    serde_json::from_slice(&record_bytes)
        .unwrap_or_else(|error| panic!("{}: not JSON: {error}", record_path.display()))
}

/// Read the Svelte cells out of the committed golden manifest.
///
/// Each record is content-addressed by the digest the manifest lists and that
/// digest is VERIFIED here; each fixture on disk is checked against the authored
/// source the record hashes.
pub(super) fn read_svelte_conformance_matrix() -> Vec<SvelteCell> {
    let goldens = harness_root().join("goldens");
    let manifest = read_json(&goldens.join("manifest.json"));
    let entries = manifest
        .get("entries")
        .and_then(Value::as_object)
        .expect("the golden manifest carries an `entries` map");

    let mut cells: Vec<SvelteCell> = entries
        .iter()
        .filter(|(name, _)| name.starts_with("svelte/"))
        .map(|(name, digest)| {
            let digest = digest
                .as_str()
                .unwrap_or_else(|| panic!("{name}: the manifest entry is not a digest string"));
            let record_path = goldens.join("records").join(format!("{digest}.json"));
            let record_bytes = std::fs::read(&record_path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", record_path.display()));
            let actual = sha256_hex(&record_bytes);
            assert_eq!(
                actual,
                digest,
                "{name}: the record at {} hashes to {actual}, not the digest the manifest names",
                record_path.display()
            );
            let record: Value = serde_json::from_slice(&record_bytes)
                .unwrap_or_else(|error| panic!("{}: not JSON: {error}", record_path.display()));

            let fixture_path = str_member(&record, &["fixture", "path"], name);
            assert!(
                fixture_path.starts_with("fixtures/svelte/"),
                "{name}: the record's fixture path `{fixture_path}` is not a Svelte fixture"
            );

            // The fixture the cell will be compiled from must be the exact one
            // the golden was generated from; otherwise the candidate and the
            // oracle would be describing different authored sources. Checked-out
            // text may carry either line ending, so the CRLF-normalised form is
            // accepted too — the same tolerance the Vue reader allows.
            let on_disk = harness_root().join(&fixture_path);
            let bytes = std::fs::read(&on_disk)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", on_disk.display()));
            let recorded = str_member(&record, &["fixture", "sha256"], name);
            let normalized = String::from_utf8(bytes.clone())
                .expect("the fixture is UTF-8")
                .replace("\r\n", "\n");
            assert!(
                sha256_hex(&bytes) == recorded || sha256_hex(normalized.as_bytes()) == recorded,
                "{name}: {} does not match the authored source the golden records \
                 ({recorded}) — the candidate would describe a different fixture",
                on_disk.display()
            );

            SvelteCell {
                golden_name: name.clone(),
                fixture_path,
                source: normalized,
                generate: SvelteGenerate::parse(&str_member(
                    &record,
                    &["options", "generate"],
                    name,
                )),
                runes: bool_member(&record, &["options", "runes"], name),
                dev: bool_member(&record, &["options", "dev"], name),
            }
        })
        .collect();

    cells.sort_by(|left, right| left.golden_name.cmp(&right.golden_name));
    cells
}

/// Every committed CLIENT cell, reachable or not.
pub(super) fn svelte_client_cells() -> Vec<SvelteCell> {
    read_svelte_conformance_matrix()
        .into_iter()
        .filter(|cell| cell.generate == SvelteGenerate::Client)
        .collect()
}

/// Every committed SERVER cell, reachable or not.
pub(super) fn svelte_server_cells() -> Vec<SvelteCell> {
    read_svelte_conformance_matrix()
        .into_iter()
        .filter(|cell| cell.generate == SvelteGenerate::Server)
        .collect()
}

/// The REACHABLE client inventory: the client cells a public/default route can
/// actually request. Derived through [`SvelteCell::reachability`], never a
/// hand-listed set.
pub(super) fn reachable_client_requests() -> Vec<SvelteCell> {
    svelte_client_cells()
        .into_iter()
        .filter(|cell| cell.reachability() == RequestReachability::Reachable)
        .collect()
}

/// The REACHABLE server inventory, derived the same way.
pub(super) fn reachable_server_requests() -> Vec<SvelteCell> {
    svelte_server_cells()
        .into_iter()
        .filter(|cell| cell.reachability() == RequestReachability::Reachable)
        .collect()
}

/// The committed goldens NO public/default route can request, with the axis
/// that puts each out of the inventory.
pub(super) fn unreachable_goldens() -> Vec<(SvelteCell, RequestReachability)> {
    read_svelte_conformance_matrix()
        .into_iter()
        .filter_map(|cell| match cell.reachability() {
            RequestReachability::Reachable => None,
            other => Some((cell, other)),
        })
        .collect()
}

// The inventory itself

#[test]
fn the_committed_manifest_holds_twelve_svelte_cells_split_six_client_six_server() {
    let cells = read_svelte_conformance_matrix();
    assert_eq!(
        cells.len(),
        12,
        "the committed manifest holds {} Svelte cells",
        cells.len()
    );
    assert_eq!(svelte_client_cells().len(), 6, "client cells");
    assert_eq!(svelte_server_cells().len(), 6, "server cells");
    assert_eq!(
        svelte_client_cells().len() + svelte_server_cells().len(),
        cells.len(),
        "some cell is neither a client nor a server cell"
    );

    // The axis space is covered exactly once: 3 fixtures × 2 targets × 2 dev
    // arms. `runes` is a property OF the fixture, not an independent axis.
    let axes: BTreeSet<(String, SvelteGenerate, bool)> = cells
        .iter()
        .map(|cell| (cell.fixture_path.clone(), cell.generate, cell.dev))
        .collect();
    assert_eq!(
        axes.len(),
        12,
        "the 12 cells cover only {} distinct axis combinations — some combination is duplicated \
         and some is missing",
        axes.len()
    );
}

#[test]
fn every_cell_axis_agrees_with_the_manifest_name_that_encodes_it() {
    // The logical name encodes the same axes the record's own `options` carry.
    // The record is the source of truth here; the two disagreeing would mean the
    // committed manifest is internally inconsistent, which must be loud rather
    // than silently resolved in favour of whichever was read.
    for cell in read_svelte_conformance_matrix() {
        let target = match cell.generate {
            SvelteGenerate::Client => "__client__",
            SvelteGenerate::Server => "__server__",
        };
        assert!(
            cell.golden_name.contains(target),
            "{}: the manifest name and the record's own `options.generate` disagree",
            cell.golden_name
        );
        assert_eq!(
            cell.golden_name.contains("__runes1__"),
            cell.runes,
            "{}: the manifest name and the record's own `options.runes` disagree",
            cell.golden_name
        );
        assert_eq!(
            cell.golden_name.ends_with("__dev1"),
            cell.dev,
            "{}: the manifest name and the record's own `options.dev` disagree",
            cell.golden_name
        );
    }
}

/// Compile one committed fixture straight through the compiler's own client
/// entry with an explicit `dev_codegen` request.
///
/// The carrier hardcodes `dev_codegen: false`
/// (`crates/verter_compiler/src/svelte/carrier.rs:377`), so this is the only way
/// to observe what the OTHER value of the axis does. It is deliberately one
/// layer below the shipped route: the point is that no public request can reach
/// this call with `dev_codegen: true`.
fn compile_client_with_dev_codegen(
    source: &str,
    dev_codegen: bool,
) -> Result<
    verter_compiler::svelte::runtime::ClientModule,
    verter_compiler::svelte::runtime::ClientCompileError,
> {
    let parsed = verter_compiler::svelte::parser::parse_svelte(source);
    let allocator = oxc_allocator::Allocator::new();
    let opts = verter_compiler::svelte::runtime::SvelteRuntimeOptions {
        filename: Some("fixtures/svelte/basic-runes.svelte".to_string()),
        dev_codegen,
        ..verter_compiler::svelte::runtime::SvelteRuntimeOptions::default()
    };
    verter_compiler::svelte::runtime::compile_client(
        source, &parsed, &opts, &allocator, false, true,
    )
}

#[test]
fn the_generate_axis_is_expressible_by_the_shipped_profile() {
    for cell in read_svelte_conformance_matrix() {
        assert_eq!(
            cell.compile_profile().ssr,
            cell.generate == SvelteGenerate::Server,
            "{}: the profile's ssr axis does not follow the record's `generate`",
            cell.golden_name
        );
        assert!(
            cell.compile_profile().source_map,
            "{}: every Svelte golden is generated with a map, so the candidate must request one",
            cell.golden_name
        );
    }
}

/// The `dev` axis has NO public spelling, and only its implicit value is a
/// reachable request. Both halves are proven, not declared.
///
/// This is the test that must fail if the axis later becomes expressible
/// without the reachable inventory following:
///
/// * (a) NO PUBLIC SPELLING — the two `dev` arms of one `(fixture, generate)`
///   pair produce IDENTICAL production requests. Adding a `dev` field to
///   `CompileProfile` (and threading it) makes the two profiles differ and
///   fails here.
/// * (b) THE OTHER VALUE IS NOT A REACHABLE SUCCESS — driving the compiler's
///   own client entry with `dev_codegen: !DEV_AXIS_IMPLICIT_VALUE` fails closed
///   with the typed dev-mode refusal
///   (`crates/verter_compiler/src/svelte/runtime/parse_refusal.rs:67-71`). If
///   the backend starts emitting dev output, this fails.
/// * (c) THE IMPLICIT VALUE IS the reachable one — the same entry at
///   `DEV_AXIS_IMPLICIT_VALUE` compiles a supported fixture. If the implicit
///   value flips, this fails.
#[test]
fn the_dev_axis_has_no_public_spelling_and_only_its_implicit_value_is_reachable() {
    // (a)
    let cells = read_svelte_conformance_matrix();
    let mut pairs_checked = 0usize;
    for implicit_arm in cells
        .iter()
        .filter(|cell| cell.dev == DEV_AXIS_IMPLICIT_VALUE)
    {
        let other_arm = cells
            .iter()
            .find(|cell| {
                cell.dev != DEV_AXIS_IMPLICIT_VALUE
                    && cell.fixture_path == implicit_arm.fixture_path
                    && cell.generate == implicit_arm.generate
            })
            .unwrap_or_else(|| {
                panic!(
                    "{}: no opposite-`dev` twin, so this pair cannot prove the axis is \
                     inexpressible",
                    implicit_arm.golden_name
                )
            });
        let (left, right) = (implicit_arm.compile_profile(), other_arm.compile_profile());
        assert_eq!(
            (left.ssr, left.source_map, left.is_production, left.filename),
            (
                right.ssr,
                right.source_map,
                right.is_production,
                right.filename
            ),
            "{} and {} produce DIFFERENT production requests — the `dev` axis has become \
             expressible, so the reachable inventory must map it instead of classifying the \
             non-implicit arm out",
            implicit_arm.golden_name,
            other_arm.golden_name
        );
        pairs_checked += 1;
    }
    assert_eq!(
        pairs_checked, 6,
        "expected one pair per (fixture, generate) combination, checked {pairs_checked}"
    );

    // (b) and (c), over a fixture the client backend supports.
    let supported = cells
        .iter()
        .find(|cell| cell.fixture_path.ends_with("basic-runes.svelte"))
        .expect("the manifest carries the basic-runes fixture");
    let refused = compile_client_with_dev_codegen(&supported.source, !DEV_AXIS_IMPLICIT_VALUE);
    assert!(
        matches!(
            refused,
            Err(
                verter_compiler::svelte::runtime::ClientCompileError::Unsupported(
                    verter_compiler::svelte::runtime::UnsupportedSvelteRuntimeSurface::DevMode { .. }
                )
            )
        ),
        "the non-implicit `dev` value is no longer refused ({refused:?}); if it now compiles it \
         is a reachable request and the inventory must include its goldens"
    );
    let compiled = compile_client_with_dev_codegen(&supported.source, DEV_AXIS_IMPLICIT_VALUE);
    assert!(
        compiled.is_ok(),
        "the implicit `dev` value no longer compiles this fixture ({compiled:?}), so the \
         recorded implicit value is wrong"
    );
}

/// Nothing expressible is wrongly excluded from the reachable inventory.
///
/// The complement of the test above: every cell whose axes are ALL expressible
/// must BE in the reachable inventory, and every value of an EXPRESSIBLE axis
/// must be represented there. Misclassifying `generate` as inexpressible would
/// drop the server requests and fail here.
#[test]
fn every_cell_whose_axes_are_all_expressible_is_in_the_reachable_inventory() {
    let cells = read_svelte_conformance_matrix();
    let reachable: BTreeSet<String> = cells
        .iter()
        .filter(|cell| cell.reachability() == RequestReachability::Reachable)
        .map(|cell| cell.golden_name.clone())
        .collect();
    let expected: BTreeSet<String> = cells
        .iter()
        .filter(|cell| cell.dev == DEV_AXIS_IMPLICIT_VALUE)
        .map(|cell| cell.golden_name.clone())
        .collect();
    assert_eq!(
        reachable, expected,
        "the reachable inventory is not exactly the cells whose only inexpressible axis carries \
         its implicit value"
    );

    // Both values of the EXPRESSIBLE `generate` axis survive into the reachable
    // inventory. A regression that classified `ssr` inexpressible would empty
    // one of these.
    assert_eq!(
        reachable_client_requests().len(),
        3,
        "reachable client requests: {:?}",
        reachable_client_requests()
            .iter()
            .map(|cell| cell.golden_name.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        reachable_server_requests().len(),
        3,
        "reachable server requests: {:?}",
        reachable_server_requests()
            .iter()
            .map(|cell| cell.golden_name.clone())
            .collect::<Vec<_>>()
    );

    // And the complement is accounted for: every remaining golden is out for a
    // NAMED axis, with its recorded and implicit values recorded.
    let unreachable = unreachable_goldens();
    assert_eq!(
        unreachable.len(),
        6,
        "goldens with no public spelling: {:?}",
        unreachable
            .iter()
            .map(|(cell, _)| cell.golden_name.clone())
            .collect::<Vec<_>>()
    );
    for (cell, why) in &unreachable {
        assert_eq!(
            why,
            &RequestReachability::NoPublicSpelling {
                axis: "dev",
                recorded: !DEV_AXIS_IMPLICIT_VALUE,
                implicit: DEV_AXIS_IMPLICIT_VALUE,
            },
            "{}: unexpected out-of-inventory reason",
            cell.golden_name
        );
    }
    assert_eq!(
        reachable.len() + unreachable.len(),
        cells.len(),
        "some cell is neither reachable nor accounted for as unreachable"
    );
}

#[test]
fn the_recorded_runes_axis_matches_what_the_shipped_route_infers() {
    // The shipped route hardcodes `runes: None`
    // (`crates/verter_compiler/src/svelte/carrier.rs:369`), so the mode is
    // inferred from the component source. This asserts the inference agrees with
    // the axis the golden recorded, for every cell — so a candidate is never
    // silently compiled in the other mode from the one the golden was generated
    // under.
    for cell in read_svelte_conformance_matrix() {
        let language = verter_language::FileLanguage::svelte();
        let provenance = crate::types::MetaProvenance::default();
        let (_snapshot, artifact) = crate::parse::carrier_parse_snapshot(
            &cell.fixture_path,
            &cell.source,
            verter_semantic::analysis::AnalysisScope::LSP,
            &language,
            &provenance,
        )
        .unwrap_or_else(|| panic!("{}: no Svelte carrier artifact", cell.golden_name));

        // The mode classifier reads the component's SCRIPTS, so it runs over the
        // carrier's own position-preserving eval source — the same bytes the
        // host's Svelte snapshot builder analyses — rather than a second
        // hand-rolled blanking of the carrier text.
        let compiler = crate::parse::carrier_compiler_registry()
            .compiler_for_carrier_language(
                language.adapter_id().expect("svelte has an adapter id"),
                language
                    .carrier_language_id()
                    .expect("svelte has a carrier language id"),
            )
            .expect("the registry serves the Svelte carrier");
        let eval_source = compiler.eval_source(&cell.source, &artifact);
        let allocator = oxc_allocator::Allocator::new();
        let parsed_program = oxc_parser::Parser::new(
            &allocator,
            eval_source.as_ref(),
            oxc_span::SourceType::mjs(),
        )
        .parse();
        let inferred =
            crate::parse::svelte_component_runes_mode(&artifact, &parsed_program.program);
        assert_eq!(
            inferred, cell.runes,
            "{}: the golden was generated with runes={}, but the shipped route's own mode \
             inference reads the same source as runes={inferred}",
            cell.golden_name, cell.runes
        );
    }
}

/// Every committed golden names the SAME pinned official domain, and that
/// domain is the pinned `svelte@5.56.10`.
#[test]
fn every_committed_golden_names_the_same_pinned_official_domain() {
    let domain = pinned_svelte_domain();
    assert_eq!(
        domain["packageVersion"].as_str(),
        Some(SVELTE_PINNED_PACKAGE_VERSION),
        "the committed goldens were generated against a different official Svelte package \
         version than the pinned one"
    );
    assert_eq!(
        domain["upstream"].as_str(),
        Some("https://github.com/sveltejs/svelte"),
        "the committed goldens name a different upstream"
    );
    assert!(
        domain["commit"]
            .as_str()
            .is_some_and(|commit| commit.len() == 40),
        "the pinned domain carries no full commit sha: {domain}"
    );
}
