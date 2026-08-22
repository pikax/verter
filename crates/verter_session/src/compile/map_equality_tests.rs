//! Cross-implementation exact-equality harness.
//!
//! Runs [`assemble_vue_main_module`] and the independent JS reference
//! (`assembled-map-composition-reference.mjs`) on the same input and
//! asserts identical code bytes, decoded map (field- and position-exact,
//! ordered segments), and fail-closed rejection identity.
//!
//! Implementations were written independently from
//! `assembled-map-composition-layer1.md`, not from each other. This
//! harness adds no semantics: it bridges representations and compares.
//!
//! [`AssembleInput`] is the spec DTO, projected both ways: JSON
//! (re-validated on arrival) and production host structs (no second
//! compose path). Both artifacts decode through the same
//! [`validate_and_decode`].

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use verter_compiler::framework_common::{
    RuntimeCompileOutput, RuntimeCustomBlock, RuntimeOutputDescriptor, RuntimeScriptBlock,
    RuntimeStyleBlock, RuntimeTemplateBlock, SourceMapFidelity, TemplateRenderExport,
};

use super::map_input::{validate_and_decode, UncomposableCode, UncomposableFamily};
use super::{assemble_vue_main_module, AssembleMapFailure, VueMainAssemblyFailure};
use crate::types::{CompileProfile, FileMeta, HmrStrategy};

// The input DTO (§3.3)

#[derive(Debug, Clone)]
struct ScriptFragment {
    code: String,
    /// RAW, unparsed. `""` means "no map".
    source_map: String,
}

#[derive(Debug, Clone)]
struct TemplateFragment {
    code: String,
    imports: Vec<String>,
    ssr_imports: Vec<String>,
    /// RAW, unparsed. `""` means "no map".
    source_map: String,
}

/// Pre-assembly input DTO of §3.3: every input the assembler reads, and
/// nothing else. No placement/offset/splice/cursor — those are derived as
/// each side writes (§6.3).
#[derive(Debug, Clone)]
struct AssembleInput {
    canonical_id: String,
    style_count: u32,
    custom_block_count: u32,
    style_langs: Vec<Option<String>>,
    custom_types: Vec<String>,
    script: Option<ScriptFragment>,
    template: Option<TemplateFragment>,
    scope_id: String,
    runtime_module_name: Option<String>,
    is_production: bool,
    ssr: bool,
    ssr_module_id: Option<String>,
    /// Mirrors `CompileProfile::emit_ssr_module_registration` (see that
    /// field's own doc comment) — whether SSR assembly wraps `setup()` with
    /// the `useSSRContext`/`ssrContext.modules` registration. Only
    /// meaningful when `ssr` is `true`.
    emit_ssr_module_registration: bool,
    hmr_strategy: HmrStrategy,
    source_map_requested: bool,
    /// The pre-assembly AUTHORED-fragment inventory (§3.4), never the presence
    /// of a compiled block.
    authored_script: bool,
    authored_template: bool,
}

impl Default for AssembleInput {
    /// The minimal admissible instance: a mapless, blockless, dev, non-SSR
    /// module with maps requested. Every case below states only what it varies.
    fn default() -> Self {
        Self {
            canonical_id: "Comp.vue".to_string(),
            style_count: 0,
            custom_block_count: 0,
            style_langs: Vec::new(),
            custom_types: Vec::new(),
            script: None,
            template: None,
            scope_id: String::new(),
            runtime_module_name: None,
            is_production: false,
            ssr: false,
            ssr_module_id: None,
            // Matches `CompileProfile::default()` — every existing caller not
            // exercising this knob keeps today's byte-identical output.
            emit_ssr_module_registration: true,
            hmr_strategy: HmrStrategy::None,
            source_map_requested: true,
            authored_script: false,
            authored_template: false,
        }
    }
}

impl AssembleInput {
    /// A script fragment that is both authored and present.
    fn with_script(mut self, code: &str, source_map: &str) -> Self {
        self.script = Some(ScriptFragment {
            code: code.to_string(),
            source_map: source_map.to_string(),
        });
        self.authored_script = true;
        self
    }

    /// A present script fragment that is NOT authored — the compiler-synthesised
    /// block of a template-only cell (§3.4).
    fn with_synthetic_script(mut self, code: &str, source_map: &str) -> Self {
        self.script = Some(ScriptFragment {
            code: code.to_string(),
            source_map: source_map.to_string(),
        });
        self.authored_script = false;
        self
    }

    fn with_template(mut self, code: &str, source_map: &str) -> Self {
        self.template = Some(TemplateFragment {
            code: code.to_string(),
            imports: Vec::new(),
            ssr_imports: Vec::new(),
            source_map: source_map.to_string(),
        });
        self.authored_template = true;
        self
    }

    fn with_template_imports(mut self, imports: &[&str], ssr_imports: &[&str]) -> Self {
        let template = self
            .template
            .as_mut()
            .expect("set the template fragment before its imports");
        template.imports = imports.iter().map(|name| name.to_string()).collect();
        template.ssr_imports = ssr_imports.iter().map(|name| name.to_string()).collect();
        self
    }

    /// §3.3 JSON transport. Exact field list — extra or missing members fail
    /// loudly in the reference.
    fn to_dto_json(&self) -> Value {
        json!({
            "canonicalId": self.canonical_id,
            "styleCount": self.style_count,
            "customBlockCount": self.custom_block_count,
            "styleLangs": self.style_langs,
            "customTypes": self.custom_types,
            "script": self.script.as_ref().map(|script| json!({
                "code": script.code,
                "sourceMap": script.source_map,
            })),
            "template": self.template.as_ref().map(|template| json!({
                "code": template.code,
                "imports": template.imports,
                "ssrImports": template.ssr_imports,
                "sourceMap": template.source_map,
            })),
            "scopeId": self.scope_id,
            "runtimeModuleName": self.runtime_module_name,
            "isProduction": self.is_production,
            "ssr": self.ssr,
            "ssrModuleId": self.ssr_module_id,
            "emitSsrModuleRegistration": self.emit_ssr_module_registration,
            "hmrStrategy": match self.hmr_strategy {
                HmrStrategy::Vite => "vite",
                HmrStrategy::Webpack => "webpack",
                HmrStrategy::None => "none",
            },
            "sourceMapRequested": self.source_map_requested,
            "authored": {
                "script": self.authored_script,
                "template": self.authored_template,
            },
        })
    }

    /// Same inputs as the production triple. Style/custom blocks are
    /// placeholders: assembler reads COUNT; ids come from `meta`. Lengths
    /// may differ (§3.3 note 1).
    fn to_production_inputs(&self) -> (RuntimeCompileOutput, FileMeta, CompileProfile) {
        let compiled = RuntimeCompileOutput {
            script: self.script.as_ref().map(|script| RuntimeScriptBlock {
                code: script.code.clone(),
                source_map: script.source_map.clone(),
                setup: true,
                output_descriptor: descriptor(&script.code),
                generated_template_hole: None,
                runtime_imports: Vec::new(),
                sfc_export_placement: super::map_compose::literal_scan_placement_for_fixture(
                    &script.code,
                ),
            }),
            template: self.template.as_ref().map(|template| RuntimeTemplateBlock {
                code: template.code.clone(),
                source_map: template.source_map.clone(),
                imports: template.imports.clone(),
                ssr_imports: template.ssr_imports.clone(),
                render_export: if self.ssr {
                    TemplateRenderExport::SsrRender
                } else {
                    TemplateRenderExport::Render
                },
                output_descriptor: descriptor(&template.code),
            }),
            styles: (0..self.style_count)
                .map(|_| RuntimeStyleBlock {
                    code: String::new(),
                    source_map: None,
                    lang: None,
                    scope_hash: None,
                    has_global: false,
                    output_descriptor: descriptor(""),
                })
                .collect(),
            custom_blocks: (0..self.custom_block_count)
                .map(|_| RuntimeCustomBlock {
                    block_type: String::new(),
                    content: String::new(),
                })
                .collect(),
            scope_id: self.scope_id.clone(),
            ..RuntimeCompileOutput::default()
        };

        let meta = FileMeta {
            has_script: self.authored_script,
            has_template: self.authored_template,
            style_langs: self.style_langs.clone(),
            custom_types: self.custom_types.clone(),
            ..FileMeta::default()
        };

        let profile = CompileProfile {
            is_production: self.is_production,
            ssr: self.ssr,
            ssr_module_id: self.ssr_module_id.clone(),
            emit_ssr_module_registration: self.emit_ssr_module_registration,
            hmr_strategy: self.hmr_strategy,
            runtime_module_name: self.runtime_module_name.clone(),
            source_map: self.source_map_requested,
            ..CompileProfile::default()
        };

        (compiled, meta, profile)
    }
}

impl AssembleInput {
    /// DTO from a real production triple. Tests DTO completeness as well as
    /// composition: an omitted assembler input would compose different modules.
    fn from_production_inputs(
        canonical_id: &str,
        compiled: &RuntimeCompileOutput,
        meta: &FileMeta,
        profile: &CompileProfile,
    ) -> Self {
        Self {
            canonical_id: canonical_id.to_string(),
            style_count: u32::try_from(compiled.styles.len()).expect("style count fits a uint32"),
            custom_block_count: u32::try_from(compiled.custom_blocks.len())
                .expect("custom-block count fits a uint32"),
            style_langs: meta.style_langs.clone(),
            custom_types: meta.custom_types.clone(),
            script: compiled.script.as_ref().map(|script| ScriptFragment {
                code: script.code.clone(),
                source_map: script.source_map.clone(),
            }),
            template: compiled.template.as_ref().map(|template| TemplateFragment {
                code: template.code.clone(),
                imports: template.imports.clone(),
                ssr_imports: template.ssr_imports.clone(),
                source_map: template.source_map.clone(),
            }),
            scope_id: compiled.scope_id.clone(),
            runtime_module_name: profile.runtime_module_name.clone(),
            is_production: profile.is_production,
            ssr: profile.ssr,
            ssr_module_id: profile.ssr_module_id.clone(),
            emit_ssr_module_registration: profile.emit_ssr_module_registration,
            hmr_strategy: profile.hmr_strategy,
            source_map_requested: profile.source_map,
            authored_script: meta.has_script,
            authored_template: meta.has_template,
        }
    }
}

fn descriptor(code: &str) -> RuntimeOutputDescriptor {
    RuntimeOutputDescriptor::generated(
        code,
        None,
        &[("test:space", "test:artifact")],
        SourceMapFidelity::Approximate,
    )
}

// The compared artifact (§7.1)

/// One decoded segment. Authored fields are separate so a partly-null
/// group is a mismatch, not inexpressible.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ComparedSegment {
    gen_line: u32,
    gen_col: u32,
    src_idx: Option<u32>,
    src_line: Option<u32>,
    src_col: Option<u32>,
    name_idx: Option<u32>,
}

/// Decoded map of §7.1: members whose value varies, plus ordered segments.
/// `version`/`file`/`debugId`/unknowns are asserted per side in
/// [`compared_artifact`] (equality would pass if both sides were wrong the
/// same way). `ignore_list` is the logical member; JSON spelling is not
/// compared (§7.8).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ComparedArtifact {
    source_root: Option<String>,
    names: Vec<String>,
    sources: Vec<String>,
    sources_content: Option<Vec<Option<String>>>,
    ignore_list: Option<Vec<u32>>,
    mappings: String,
    segments: Vec<ComparedSegment>,
}

/// The §7.1 member names, plus the two serialization spellings of the ignore
/// list and the two generated-side members §7.2 drops.
const KNOWN_ARTIFACT_MEMBERS: &[&str] = &[
    "version",
    "file",
    "sourceRoot",
    "names",
    "sources",
    "sourcesContent",
    "ignoreList",
    "x_google_ignoreList",
    "mappings",
    "debugId",
];

/// Decode one side's emitted artifact into the compared shape.
///
/// `side` names the implementation, so any loud failure here says which one
/// produced the offending artifact. `code` is that side's own assembled code:
/// running the artifact through [`validate_and_decode`] against it proves the
/// artifact is a well-formed flat v3 map whose coordinates are all in bounds of
/// the module it describes.
fn compared_artifact(side: &str, raw: &str, code: &str) -> ComparedArtifact {
    let document: Value = serde_json::from_str(raw)
        .unwrap_or_else(|error| panic!("{side}: emitted artifact is not JSON: {error}\n{raw}"));
    let object = document
        .as_object()
        .unwrap_or_else(|| panic!("{side}: emitted artifact is not a JSON object\n{raw}"));

    // §7.1 — `version` is always present and always 3.
    let version = object
        .get("version")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("{side}: artifact has no integral `version`\n{raw}"));
    assert_eq!(version, 3, "{side}: §7.1 fixes `version` at 3\n{raw}");

    // §7.2 — metadata describing the GENERATED document is dropped, because the
    // document it described no longer exists. Inheriting a fragment's would be a
    // false claim, so this is asserted per side rather than compared.
    for member in ["file", "debugId"] {
        assert!(
            !object.contains_key(member),
            "{side}: §7.2 drops the generated-side member `{member}`, but it is present\n{raw}"
        );
    }
    let unknown: Vec<&String> = object
        .keys()
        .filter(|member| !KNOWN_ARTIFACT_MEMBERS.contains(&member.as_str()))
        .collect();
    assert!(
        unknown.is_empty(),
        "{side}: §7.1's schema admits no member outside it, but the artifact carries \
         {unknown:?}\n{raw}"
    );

    // Present-and-a-string, or absent. §7.5 admits no third shape on the
    // OUTPUT side, so a present non-string is a defect in the side that emitted
    // it rather than something to normalise away.
    let source_root = object.get("sourceRoot").map(|value| {
        value
            .as_str()
            .unwrap_or_else(|| panic!("{side}: `sourceRoot` is present but is not a string\n{raw}"))
            .to_string()
    });

    let string_rows = |member: &str| -> Vec<String> {
        object
            .get(member)
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{side}: `{member}` is absent or not an array\n{raw}"))
            .iter()
            .map(|row| {
                row.as_str()
                    .unwrap_or_else(|| panic!("{side}: `{member}` has a non-string row\n{raw}"))
                    .to_string()
            })
            .collect()
    };

    let sources_content = object.get("sourcesContent").map(|value| {
        value
            .as_array()
            .unwrap_or_else(|| panic!("{side}: `sourcesContent` is not an array\n{raw}"))
            .iter()
            .map(|row| {
                if row.is_null() {
                    None
                } else {
                    Some(
                        row.as_str()
                            .unwrap_or_else(|| {
                                panic!("{side}: `sourcesContent` row is neither string nor null")
                            })
                            .to_string(),
                    )
                }
            })
            .collect()
    });

    // §7.3 / §7.8 — one logical member, two accepted spellings. A side emitting
    // BOTH would be publishing the field twice, so that is refused here rather
    // than silently reading whichever came first.
    let standard = object.get("ignoreList");
    let extension = object.get("x_google_ignoreList");
    assert!(
        standard.is_none() || extension.is_none(),
        "{side}: artifact carries BOTH ignore-list spellings\n{raw}"
    );
    let ignore_list =
        standard.or(extension).map(|value| {
            value
                .as_array()
                .unwrap_or_else(|| panic!("{side}: the ignore list is not an array\n{raw}"))
                .iter()
                .map(|entry| {
                    u32::try_from(entry.as_u64().unwrap_or_else(|| {
                        panic!("{side}: ignore-list entry is not a uint\n{raw}")
                    }))
                    .unwrap_or_else(|_| panic!("{side}: ignore-list entry exceeds uint32\n{raw}"))
                })
                .collect()
        });

    let mappings = object
        .get("mappings")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{side}: `mappings` is absent or not a string\n{raw}"))
        .to_string();

    let decoded = validate_and_decode(raw, code).unwrap_or_else(|failure| {
        panic!(
            "{side}: its own emitted artifact does not validate against its own code: {}\n{raw}",
            failure.as_str()
        )
    });

    ComparedArtifact {
        source_root,
        names: string_rows("names"),
        sources: string_rows("sources"),
        sources_content,
        ignore_list,
        mappings,
        segments: decoded
            .segments
            .iter()
            .map(|segment| ComparedSegment {
                gen_line: segment.generated_line,
                gen_col: segment.generated_column,
                src_idx: segment.payload.map(|payload| payload.source_index),
                src_line: segment.payload.map(|payload| payload.source_line),
                src_col: segment.payload.map(|payload| payload.source_column),
                name_idx: segment.payload.and_then(|payload| payload.name_index),
            })
            .collect(),
    }
}

// The outcome, on both sides

/// What one implementation produced for one input: the §4.2 fail-closed kinds,
/// or a composed module.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ComposeOutcome {
    /// `map` is `None` only when no map was requested (§7.7) — asserted
    /// positively on both sides, never inferred from an omitted check.
    Composed {
        code: String,
        map: Option<ComparedArtifact>,
    },
    MissingRequiredInputMap {
        fragment: String,
    },
    UncomposableInputMap {
        fragment: String,
        family: String,
        code: String,
    },
    /// A fragment-grammar, composition, or publication (final-parse/
    /// undeclared-helper) failure — no frozen `expected_outcome` variant
    /// produces this, so it always diverges when compared; the ONLY
    /// production outcome variant with no reference-side analogue. See
    /// `V19`'s doc note in `vector_inventory.rs` for the one frozen vector
    /// that reaches it today.
    AssemblyFailed {
        detail: String,
    },
}

/// The family's stable spelling. Exhaustive, so a new family cannot reach the
/// comparison without a spelling.
fn family_str(family: UncomposableFamily) -> &'static str {
    match family {
        UncomposableFamily::MalformedJson => "U1",
        UncomposableFamily::Version => "U2",
        UncomposableFamily::WireData => "U3",
        UncomposableFamily::TableRows => "U4",
        UncomposableFamily::IndexedMap => "U5",
        UncomposableFamily::DanglingIndex => "U6",
        UncomposableFamily::Coordinate => "U7",
        UncomposableFamily::CrossFragmentMetadata => "U8",
    }
}

/// The sub-code alone (`"U1.1"`), split off the stable diagnostic spelling
/// (`"U1.1 map-bytes-not-json"`).
fn sub_code_str(code: UncomposableCode) -> String {
    code.as_str()
        .split_whitespace()
        .next()
        .expect("the diagnostic spelling always starts with its sub-code")
        .to_string()
}

fn production_outcome(input: &AssembleInput) -> ComposeOutcome {
    let (compiled, meta, profile) = input.to_production_inputs();
    match assemble_vue_main_module(&input.canonical_id, &compiled, &meta, &profile) {
        Ok(assembled) => {
            let map = assembled
                .source_map
                .as_ref()
                .map(|raw| compared_artifact("production", raw, &assembled.code));
            ComposeOutcome::Composed {
                code: assembled.code,
                map,
            }
        }
        Err(VueMainAssemblyFailure::InputMap(AssembleMapFailure::MissingRequiredInputMap {
            fragment,
        })) => ComposeOutcome::MissingRequiredInputMap {
            fragment: fragment.as_str().to_string(),
        },
        Err(VueMainAssemblyFailure::InputMap(AssembleMapFailure::UncomposableInputMap {
            fragment,
            code,
        })) => {
            // The family is read off the taxonomy's own classifier, then
            // cross-checked against the sub-code's numeric prefix — so a
            // sub-code filed under the wrong family fails here rather than
            // being laundered through a textual prefix.
            let family = family_str(code.family());
            let sub_code = sub_code_str(code);
            assert_eq!(
                Some(family),
                sub_code.split('.').next(),
                "production: sub-code {sub_code} is classified under family {family}"
            );
            ComposeOutcome::UncomposableInputMap {
                fragment: fragment.as_str().to_string(),
                family: family.to_string(),
                code: sub_code,
            }
        }
        Err(VueMainAssemblyFailure::InputMap(AssembleMapFailure::InvalidSfcExportPlacement {
            reason,
        })) => {
            // Every fixture's `sfc_export_placement` fact is derived from
            // its OWN `code` by `map_compose::literal_scan_placement_for_fixture`
            // (test-only, mirrors the same text), so it always matches —
            // this arm has no reference-side equivalent to compare against
            // and firing here is a defect in that fixture-construction
            // helper, not a real production outcome under test.
            panic!(
                "no cross-implementation fixture declares an inconsistent __sfc__ fact: {reason:?}"
            );
        }
        // A fragment/composition/publication failure has no reference-side
        // analogue and no frozen `expected_outcome` variant, so it always
        // diverges when compared — the divergence is what a caller (e.g.
        // `vector_inventory.rs`'s V19 exclusion) decides how to treat, not
        // a harness-level panic. One frozen vector today (V19) reaches this
        // deliberately: it embeds the literal text `export default
        // _sfc_main;` inside its OWN template code to simulate the retired
        // write-grammar rules W-13/W-13' (see that vector's doc note) —
        // composed alongside this assembler's own always-present trailing
        // `export default _sfc_main`, the result genuinely has two default
        // exports. The FORMER hardcoded-permissive-TSX final-parse check
        // never caught this (TSX's `Unambiguous` module-kind does not
        // enforce the single-default-export rule the way an explicit
        // `Module` dialect does); this dialect-accurate parse now correctly
        // does — proof the accurate dialect is doing real work, not a
        // regression to work around.
        Err(other) => ComposeOutcome::AssemblyFailed {
            detail: format!("{other:?}"),
        },
    }
}

// The reference, as a subprocess

fn driver_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/framework-conformance-harness/bin/compose-assembled-map.mjs")
}

/// Run the reference over a whole batch in ONE subprocess, returning its
/// results in input order.
///
/// A missing Node is a hard failure, not a skip. This harness IS the acceptance
/// evidence that the two implementations agree; a run that silently passes
/// without the reference having executed would report exactly the agreement it
/// never checked.
fn reference_outcomes(inputs: &[AssembleInput]) -> Vec<ComposeOutcome> {
    let driver = driver_path();
    assert!(
        driver.exists(),
        "the reference driver is missing at {}",
        driver.display()
    );

    let batch = json!({
        "cases": inputs.iter().map(AssembleInput::to_dto_json).collect::<Vec<_>>(),
    })
    .to_string();

    let mut child = Command::new("node")
        .arg(&driver)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| {
            panic!(
                "cannot run the independent JavaScript reference: `node` failed to start ({error}).\n\
                 This harness compares the production assembler against that reference; without it \
                 nothing is compared.\n\
                 Install Node (the workspace already requires it for `scripts/gate.mjs`) and re-run."
            )
        });
    child
        .stdin
        .as_mut()
        .expect("stdin was piped")
        .write_all(batch.as_bytes())
        .expect("the driver reads its whole input before writing");
    let output = child.wait_with_output().expect("the driver terminates");
    assert!(
        output.status.success(),
        "the reference driver exited with {}:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let document: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "the reference driver did not emit JSON ({error}):\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    let results = document
        .get("results")
        .and_then(Value::as_array)
        .expect("the driver emits a `results` array");
    assert_eq!(
        results.len(),
        inputs.len(),
        "the reference returned {} results for {} cases",
        results.len(),
        inputs.len()
    );

    results.iter().map(reference_outcome).collect()
}

fn reference_outcome(result: &Value) -> ComposeOutcome {
    let outcome = result
        .get("outcome")
        .and_then(Value::as_str)
        .expect("every reference result carries an `outcome`");
    match outcome {
        "composed" => {
            let code = result
                .get("code")
                .and_then(Value::as_str)
                .expect("a composed result carries `code`")
                .to_string();
            let map_value = result.get("map").expect("a composed result carries `map`");
            let map = if map_value.is_null() {
                None
            } else {
                // §8 — the provenance tag is never serialized. Asserted on the
                // artifact itself, because a tag that leaked into a member
                // would otherwise ride through the comparison as ordinary data.
                let members = map_value
                    .as_object()
                    .expect("the reference's map is an object");
                for member in ["origin", "provenance", "origins"] {
                    assert!(
                        !members.contains_key(member),
                        "reference: the artifact carries the composition-time member `{member}`"
                    );
                }
                let raw = serde_json::to_string(map_value).expect("the map re-serializes");
                let artifact = compared_artifact("reference", &raw, &code);

                // The reference also surfaces its own ordered segment sequence
                // so a comparator need not re-decode. Cross-check it against
                // what its own `mappings` decodes to: an encoder bug that
                // cancelled out against a decoder bug would otherwise be
                // invisible.
                let claimed: Vec<ComparedSegment> = result
                    .get("segments")
                    .and_then(Value::as_array)
                    .expect("a composed result carries `segments`")
                    .iter()
                    .map(claimed_segment)
                    .collect();
                assert_eq!(
                    artifact.segments, claimed,
                    "reference: its surfaced segment sequence disagrees with its own `mappings`"
                );

                let provenance = result
                    .get("provenance")
                    .and_then(Value::as_array)
                    .expect("a composed result carries `provenance`");
                assert_eq!(
                    provenance.len(),
                    claimed.len(),
                    "reference: one provenance tag per segment"
                );
                Some(artifact)
            };
            ComposeOutcome::Composed { code, map }
        }
        "MissingRequiredInputMap" => ComposeOutcome::MissingRequiredInputMap {
            fragment: string_member(result, "fragment"),
        },
        "UncomposableInputMap" => ComposeOutcome::UncomposableInputMap {
            fragment: string_member(result, "fragment"),
            family: string_member(result, "family"),
            code: string_member(result, "code"),
        },
        "MalformedAssembleInput" => panic!(
            "the bridge produced a DTO the reference rejects as out of scope: {}",
            string_member(result, "message")
        ),
        other => panic!("the reference reported an unknown outcome kind `{other}`"),
    }
}

fn claimed_segment(value: &Value) -> ComparedSegment {
    let field = |name: &str| -> Option<u32> {
        let raw = value.get(name).unwrap_or(&Value::Null);
        if raw.is_null() {
            None
        } else {
            Some(u32::try_from(raw.as_u64().expect("a segment field is a uint")).expect("uint32"))
        }
    };
    ComparedSegment {
        gen_line: field("genLine").expect("`genLine` is never null"),
        gen_col: field("genCol").expect("`genCol` is never null"),
        src_idx: field("srcIdx"),
        src_line: field("srcLine"),
        src_col: field("srcCol"),
        name_idx: field("nameIdx"),
    }
}

fn string_member(value: &Value, member: &str) -> String {
    value
        .get(member)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the reference result is missing the string member `{member}`"))
        .to_string()
}

// The assertion

/// Run every case through BOTH implementations and assert exact equality.
///
/// The whole batch goes through one reference subprocess, and every case is
/// compared before the first failure is raised, so one run reports every
/// divergence rather than only the earliest. The agreed outcomes are returned
/// so a caller can additionally assert WHAT was agreed on — an equality suite
/// that never looks at the outcomes cannot tell "both sides refused this input
/// for the stated reason" from "both sides refused every input for one reason".
#[track_caller]
fn assert_cross_implementation_equality(cases: &[(&str, AssembleInput)]) -> Vec<ComposeOutcome> {
    assert!(!cases.is_empty(), "an empty case set proves nothing");

    let inputs: Vec<AssembleInput> = cases.iter().map(|(_, input)| input.clone()).collect();
    let reference = reference_outcomes(&inputs);

    let mut divergences = Vec::new();
    let mut agreed = Vec::with_capacity(cases.len());
    for ((id, input), expected) in cases.iter().zip(reference) {
        let actual = production_outcome(input);
        if actual == expected {
            agreed.push(actual);
        } else {
            divergences.push(format!(
                "── {id} ──\n  production: {actual:#?}\n  reference:  {expected:#?}"
            ));
        }
    }

    assert!(
        divergences.is_empty(),
        "the production assembler and the independent JavaScript reference disagree on {} of {} \
         cases:\n\n{}",
        divergences.len(),
        cases.len(),
        divergences.join("\n\n")
    );
    agreed
}

// Map fixtures

/// One INPUT segment: `(genLine, genCol, authored)`, the authored group being
/// `(srcIdx, srcLine, srcCol, nameIdx)` and absent for a sourceless segment.
type InSeg = (u32, u32, Option<(u32, u32, u32, Option<u32>)>);

/// A source-bearing input segment against source row 0.
fn seg(line: u32, column: u32, src_line: u32, src_col: u32) -> InSeg {
    (line, column, Some((0, src_line, src_col, None)))
}

/// A source-bearing input segment carrying a name.
fn named(line: u32, column: u32, src_line: u32, src_col: u32, name: u32) -> InSeg {
    (line, column, Some((0, src_line, src_col, Some(name))))
}

fn sourceless(line: u32, column: u32) -> InSeg {
    (line, column, None)
}

/// Encode a declarative segment list to a `mappings` string.
///
/// Only the INPUT encoding is mechanical here. Nothing about an EXPECTED output
/// is computed by this harness at all — the expectation for every case is
/// whatever the independent reference produced, so an encoder slip shows up as
/// a rejected or mismatched input rather than as a self-fulfilling expectation.
fn encode_mappings(segments: &[InSeg]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn field(delta: i64, out: &mut String) {
        let mut word = if delta < 0 {
            (((-delta) as u64) << 1) | 1
        } else {
            (delta as u64) << 1
        };
        loop {
            let mut digit = (word & 31) as usize;
            word >>= 5;
            if word > 0 {
                digit |= 32;
            }
            out.push(ALPHABET[digit] as char);
            if word == 0 {
                return;
            }
        }
    }

    let mut out = String::new();
    let (mut line, mut column) = (0u32, 0i64);
    let (mut source, mut src_line, mut src_col, mut name) = (0i64, 0i64, 0i64, 0i64);
    let mut first_on_line = true;

    for &(generated_line, generated_column, authored) in segments {
        while line < generated_line {
            out.push(';');
            line += 1;
            column = 0;
            first_on_line = true;
        }
        if !first_on_line {
            out.push(',');
        }
        first_on_line = false;

        field(i64::from(generated_column) - column, &mut out);
        column = i64::from(generated_column);

        if let Some((index, authored_line, authored_column, name_index)) = authored {
            field(i64::from(index) - source, &mut out);
            source = i64::from(index);
            field(i64::from(authored_line) - src_line, &mut out);
            src_line = i64::from(authored_line);
            field(i64::from(authored_column) - src_col, &mut out);
            src_col = i64::from(authored_column);
            if let Some(index) = name_index {
                field(i64::from(index) - name, &mut out);
                name = i64::from(index);
            }
        }
    }
    out
}

/// A flat v3 input map, built member by member.
#[derive(Debug, Clone, Default)]
struct MapBuilder {
    sources: Vec<String>,
    names: Vec<String>,
    sources_content: Option<Vec<Option<String>>>,
    /// `Some(Value::Null)` emits an explicit JSON null, which §7.5 normalises
    /// to absent — distinct from `Some(Value::String(""))`.
    source_root: Option<Value>,
    ignore_list: Option<Vec<u32>>,
    segments: Vec<InSeg>,
}

impl MapBuilder {
    fn new(sources: &[&str]) -> Self {
        Self {
            sources: sources.iter().map(|source| source.to_string()).collect(),
            ..Self::default()
        }
    }

    fn names(mut self, names: &[&str]) -> Self {
        self.names = names.iter().map(|name| name.to_string()).collect();
        self
    }

    fn sources_content(mut self, rows: &[Option<&str>]) -> Self {
        self.sources_content = Some(
            rows.iter()
                .map(|row| row.map(|text| text.to_string()))
                .collect(),
        );
        self
    }

    fn source_root(mut self, root: Value) -> Self {
        self.source_root = Some(root);
        self
    }

    fn ignore_list(mut self, entries: &[u32]) -> Self {
        self.ignore_list = Some(entries.to_vec());
        self
    }

    fn segments(mut self, segments: &[InSeg]) -> Self {
        self.segments = segments.to_vec();
        self
    }

    fn build(&self) -> String {
        let mut object = serde_json::Map::new();
        object.insert("version".to_string(), json!(3));
        if let Some(root) = &self.source_root {
            object.insert("sourceRoot".to_string(), root.clone());
        }
        object.insert("sources".to_string(), json!(self.sources));
        object.insert("names".to_string(), json!(self.names));
        if let Some(rows) = &self.sources_content {
            object.insert("sourcesContent".to_string(), json!(rows));
        }
        if let Some(entries) = &self.ignore_list {
            object.insert("ignoreList".to_string(), json!(entries));
        }
        object.insert(
            "mappings".to_string(),
            json!(encode_mappings(&self.segments)),
        );
        Value::Object(object).to_string()
    }
}

/// A flat v3 map over `Comp.vue` carrying the given segments.
fn map_of(segments: &[InSeg]) -> String {
    MapBuilder::new(&["Comp.vue"]).segments(segments).build()
}

// Cases — the harness's own smoke test

/// One script fragment, one segment, maps on. If this cannot be made to agree,
/// nothing below means anything.
#[test]
fn a_minimal_mapped_script_composes_identically_on_both_sides() {
    assert_cross_implementation_equality(&[(
        "minimal-mapped-script",
        AssembleInput::default().with_script("const x = 1\n", &map_of(&[seg(0, 0, 1, 0)])),
    )]);
}

// Cases — the enumerated coverage vectors

/// Every composing seed vector, at the ASSEMBLED-module level.
///
/// The vectors were authored one layer in — over a single fragment's chain,
/// with no assembly writes, no placement and no boundary segment. Running them
/// through the mandated entry point on both sides asserts something strictly
/// stronger than either implementation's own reproduction of them: not that
/// each matches its own independently derived expectation, but that the two
/// agree with each other on the same input, through the full assembly.
#[test]
fn every_composing_seed_vector_agrees_across_implementations() {
    let v1_v2_script_segments = [
        seg(0, 0, 1, 0),
        seg(0, 6, 1, 6),
        seg(1, 0, 2, 0),
        seg(1, 15, 2, 15),
    ];

    assert_cross_implementation_equality(&[
        (
            // Rename token geometry, plus a TERMINAL removal — which therefore
            // has no following-chunk token.
            "V1 rename-geometry-and-terminal-removal",
            AssembleInput::default().with_script(
                "const __sfc__ = {}\nexport default __sfc__;\n",
                &map_of(&v1_v2_script_segments),
            ),
        ),
        (
            // A NON-terminal removal DOES have a following-chunk token.
            "V2 non-terminal-removal-following-token",
            AssembleInput::default().with_script(
                "const __sfc__ = {}\nexport default __sfc__;\nconst tail = 1\n",
                &map_of(&[
                    seg(0, 0, 1, 0),
                    seg(0, 6, 1, 6),
                    seg(1, 0, 2, 0),
                    seg(1, 15, 2, 15),
                    seg(2, 6, 3, 6),
                ]),
            ),
        ),
        (
            // Two segments on ONE generated coordinate keep their wire order. A
            // multiset or column-sorted comparator cannot tell this from its
            // swap, which is exactly why the ordered sequence is compared.
            "V3 equal-coordinate-order",
            AssembleInput::default().with_script(
                "const x = 1\n",
                &map_of(&[seg(0, 0, 1, 0), seg(0, 0, 5, 5)]),
            ),
        ),
        (
            // Tables are a stable append with NO deduplication, even when both
            // fragments declare identical spellings; template indices shift by
            // the append offset.
            "V4 no-dedup-stable-append",
            AssembleInput::default()
                .with_script(
                    "const __sfc__ = {}\n",
                    &MapBuilder::new(&["Comp.vue"])
                        .names(&["count"])
                        .segments(&[named(0, 6, 1, 6, 0)])
                        .build(),
                )
                .with_template(
                    "function render() {}\n",
                    &MapBuilder::new(&["Comp.vue"])
                        .names(&["count"])
                        .segments(&[named(0, 9, 9, 2, 0)])
                        .build(),
                ),
        ),
        (
            // Columns are UTF-16 code units — not code points, not UTF-8 bytes.
            "V5 astral-utf16-columns",
            AssembleInput::default().with_script(
                "const \u{1d400} = __sfc__\n",
                &map_of(&[seg(0, 0, 1, 0), seg(0, 11, 1, 11)]),
            ),
        ),
        (
            // A CR retained before an LF occupies a real column; lines split on
            // LF only.
            "V6 cr-occupies-a-column",
            AssembleInput::default().with_script(
                "const a = 1\r\nconst __sfc__ = {}\r\n",
                &map_of(&[seg(0, 0, 1, 0), seg(1, 6, 2, 6)]),
            ),
        ),
        (
            // A sourceless segment is a BARRIER: a lookup at or after it yields
            // a sourceless result rather than inheriting the previous source.
            "V7 sourceless-barrier",
            AssembleInput::default().with_script(
                "const __sfc__ = {}\n",
                &map_of(&[seg(0, 0, 1, 0), sourceless(0, 3)]),
            ),
        ),
    ]);
}

/// The composing fail-closed seed vector: a template-only cell whose compiler
/// produced a SYNTHETIC script block. Requiredness comes from the authored
/// inventory, so the synthetic block's empty map is not a missing required map
/// and composition proceeds.
#[test]
fn the_synthetic_script_seed_vector_agrees_across_implementations() {
    assert_cross_implementation_equality(&[(
        "F7 synthetic-script-is-not-a-missing-required-map",
        AssembleInput::default()
            .with_synthetic_script("", "")
            .with_template("function render() {}\n", &map_of(&[seg(0, 9, 9, 2)])),
    )]);
}

// Cases — the composition's own hard geometry

/// The boundary-segment condition and the two topologies that separate it from
/// the predicate it is easily confused with.
///
/// The condition is that the fragment's FINAL code ends with a newline —
/// equivalently, that its newline patch does not fire. It is deliberately NOT
/// "the end cursor column is zero" (§6.4, `DECISION` D-6), and these cases are
/// where those two disagree.
#[test]
fn boundary_segment_topologies_agree_across_implementations() {
    assert_cross_implementation_equality(&[
        (
            // An EMPTY present fragment (§6.4 case 4′). The cursor is at column
            // 0 yet the newline patch DOES fire, so no boundary is emitted —
            // firing one would land on the fragment's own carried segment's
            // coordinate and, being placed after it, shadow a faithfully
            // composed authored position.
            "empty-present-fragment",
            AssembleInput::default().with_script("", &map_of(&[seg(0, 0, 1, 0)])),
        ),
        (
            // The same fragment with a template after it, so the shadowed
            // coordinate is one an assembly-owned write actually occupies.
            "empty-present-fragment-followed-by-template",
            AssembleInput::default()
                .with_script("", &map_of(&[seg(0, 0, 1, 0)]))
                .with_template("function render() {}\n", &map_of(&[seg(0, 9, 9, 2)])),
        ),
        (
            // A fragment NOT ending in LF: the patch fires and no boundary is
            // emitted.
            "script-without-trailing-newline",
            AssembleInput::default().with_script("const x = 1", &map_of(&[seg(0, 6, 1, 6)])),
        ),
        (
            // A script whose only content is the removal pattern: after pass 2
            // the final code is EMPTY even though the input ended in LF, so the
            // condition must be read off the FINAL bytes.
            "removal-empties-the-fragment",
            AssembleInput::default().with_script(
                "export default __sfc__;\n",
                &map_of(&[seg(0, 0, 1, 0), seg(0, 15, 1, 15)]),
            ),
        ),
        (
            // A template not ending in LF, so W-12 fires on the template side.
            "template-without-trailing-newline",
            AssembleInput::default()
                .with_template("function render() {}", &map_of(&[seg(0, 9, 9, 2)])),
        ),
    ]);
}

/// The two authorized script rewrites, at the geometries the chain algebra
/// turns on.
#[test]
fn rewrite_geometries_agree_across_implementations() {
    assert_cross_implementation_equality(&[
        (
            // A MID-LINE removal: the pattern starts partway through a line, so
            // the removal is not the terminal one and leaves a live prefix.
            "mid-line-removal",
            AssembleInput::default().with_script(
                "const a = 1; export default __sfc__;\nconst tail = 1\n",
                &map_of(&[
                    seg(0, 0, 1, 0),
                    seg(0, 13, 1, 13),
                    seg(0, 28, 1, 28),
                    seg(1, 6, 2, 6),
                ]),
            ),
        ),
        // A declared `SfcExportPlacement` fact carries exactly ONE
        // `export_statement_range` — a real producer never emits more than
        // one terminal default export, so a two-export-statement fixture
        // has no fact that could declare it faithfully and is out of this
        // suite's scope. `authored_text_matching_the_landmarks_is_left_untouched`
        // in `map_tests.rs` covers the adjacent, more precise concern: an
        // UNDECLARED occurrence of the landmark text is left untouched
        // rather than incidentally swept up.
        (
            // `___sfc__` contains the literal `__sfc__` at offset 1.
            // `rewrite_script` never scans for this — it routes through a
            // producer-declared `SfcExportPlacement` fact. What this case
            // exercises is that `to_production_inputs`' own literal-scan
            // fixture helper (`map_compose::literal_scan_placement_for_fixture`,
            // test-only) finds the same non-identifier-aware match a real
            // producer's declared fact never would, and `rewrite_script` then
            // faithfully applies exactly what THAT fact declares — reproducing
            // the pinned bytes for this specific fixture without production
            // itself ever scanning.
            "rename-is-not-identifier-aware",
            AssembleInput::default().with_script(
                "const ___sfc__ = __sfc__\n",
                &map_of(&[seg(0, 0, 1, 0), seg(0, 6, 1, 6), seg(0, 17, 1, 17)]),
            ),
        ),
        (
            // Several rename replacements on ONE line, so the running column
            // shift compounds within a line.
            "multiple-same-line-renames",
            AssembleInput::default().with_script(
                "const a = __sfc__, b = __sfc__, c = __sfc__\n",
                &map_of(&[
                    seg(0, 0, 1, 0),
                    seg(0, 10, 1, 10),
                    seg(0, 23, 1, 23),
                    seg(0, 36, 1, 36),
                ]),
            ),
        ),
        (
            // Two distinct segments strictly INSIDE one rename range.
            "segments-inside-a-rename-range",
            AssembleInput::default().with_script(
                "const __sfc__ = {}\n",
                &map_of(&[seg(0, 6, 1, 6), seg(0, 8, 1, 8), seg(0, 10, 1, 10)]),
            ),
        ),
        (
            // A SOURCELESS segment inside a rename range, so the barrier
            // interacts with the replacement geometry.
            "sourceless-inside-a-rename-range",
            AssembleInput::default().with_script(
                "const __sfc__ = {}\n",
                &map_of(&[seg(0, 0, 1, 0), sourceless(0, 8), seg(0, 14, 1, 14)]),
            ),
        ),
        (
            // A rename on a CRLF line, so the CR's column participates in the
            // replacement shift rather than only in a line split.
            "rename-on-a-crlf-line",
            AssembleInput::default().with_script(
                "const __sfc__ = {}\r\nconst tail = 1\r\n",
                &map_of(&[seg(0, 6, 1, 6), seg(0, 18, 1, 18), seg(1, 6, 2, 6)]),
            ),
        ),
        (
            // An astral character BEFORE a rename, so a UTF-16 column and the
            // replacement shift compose.
            "astral-before-a-rename",
            AssembleInput::default().with_script(
                "const \u{1d400} = __sfc__\n",
                &map_of(&[seg(0, 0, 1, 0), seg(0, 6, 1, 6), seg(0, 11, 1, 11)]),
            ),
        ),
    ]);
}

/// Table composition, the ignore list, and `sourcesContent` — the members whose
/// presence and index remapping §7.3–§7.5 fix.
#[test]
fn table_composition_agrees_across_implementations() {
    assert_cross_implementation_equality(&[
        (
            // Duplicate spellings across fragments contribute two rows each;
            // the template's indices shift by the append offset.
            "duplicate-table-spellings",
            AssembleInput::default()
                .with_script(
                    "const x = 1\n",
                    &MapBuilder::new(&["a.vue", "b.vue"])
                        .names(&["one", "two"])
                        .segments(&[named(0, 0, 1, 0, 1)])
                        .build(),
                )
                .with_template(
                    "function render() {}\n",
                    &MapBuilder::new(&["a.vue", "b.vue"])
                        .names(&["one", "two"])
                        .segments(&[named(0, 9, 9, 2, 0)])
                        .build(),
                ),
        ),
        (
            // Ignore-list entries are carried, each shifted by that fragment's
            // source-table base offset. Ignore status is a property of a ROW:
            // the same spelling appears twice, ignored in one fragment only.
            "ignore-list-index-shift",
            AssembleInput::default()
                .with_script(
                    "const x = 1\n",
                    &MapBuilder::new(&["shared.vue", "only-script.vue"])
                        .ignore_list(&[1])
                        .segments(&[seg(0, 0, 1, 0)])
                        .build(),
                )
                .with_template(
                    "function render() {}\n",
                    &MapBuilder::new(&["shared.vue"])
                        .ignore_list(&[0])
                        .segments(&[seg(0, 9, 9, 2)])
                        .build(),
                ),
        ),
        (
            // An EMPTY declared ignore list: the composed member is absent
            // because the resulting list is empty, not because no input
            // declared one.
            "empty-declared-ignore-list",
            AssembleInput::default().with_script(
                "const x = 1\n",
                &MapBuilder::new(&["Comp.vue"])
                    .ignore_list(&[])
                    .segments(&[seg(0, 0, 1, 0)])
                    .build(),
            ),
        ),
        (
            // `sourcesContent` is present iff SOME row is non-null, and one
            // fragment declaring it while the other does not still yields one
            // parallel array over the composed sources table.
            "partial-sources-content",
            AssembleInput::default()
                .with_script(
                    "const x = 1\n",
                    &MapBuilder::new(&["a.vue"])
                        .sources_content(&[Some("<template/>")])
                        .segments(&[seg(0, 0, 1, 0)])
                        .build(),
                )
                .with_template(
                    "function render() {}\n",
                    &MapBuilder::new(&["b.vue"])
                        .segments(&[seg(0, 9, 9, 2)])
                        .build(),
                ),
        ),
        (
            // An all-null `sourcesContent`: declared, but no row carries
            // content, so the composed member is ABSENT.
            "all-null-sources-content",
            AssembleInput::default().with_script(
                "const x = 1\n",
                &MapBuilder::new(&["a.vue"])
                    .sources_content(&[None])
                    .segments(&[seg(0, 0, 1, 0)])
                    .build(),
            ),
        ),
        (
            // A table row NO segment references is still contributed.
            "unreferenced-table-rows",
            AssembleInput::default().with_script(
                "const x = 1\n",
                &MapBuilder::new(&["a.vue", "unreferenced.vue"])
                    .names(&["unreferenced"])
                    .segments(&[seg(0, 0, 1, 0)])
                    .build(),
            ),
        ),
        (
            // Agreeing `sourceRoot`s carry through.
            "agreeing-source-root",
            AssembleInput::default()
                .with_script(
                    "const x = 1\n",
                    &MapBuilder::new(&["a.vue"])
                        .source_root(json!("/src"))
                        .segments(&[seg(0, 0, 1, 0)])
                        .build(),
                )
                .with_template(
                    "function render() {}\n",
                    &MapBuilder::new(&["b.vue"])
                        .source_root(json!("/src"))
                        .segments(&[seg(0, 9, 9, 2)])
                        .build(),
                ),
        ),
        (
            // An EMPTY `sourceRoot` is a declared value, not the identity root:
            // it is carried, and the composed member is present and empty.
            "empty-source-root-is-declared",
            AssembleInput::default().with_script(
                "const x = 1\n",
                &MapBuilder::new(&["a.vue"])
                    .source_root(json!(""))
                    .segments(&[seg(0, 0, 1, 0)])
                    .build(),
            ),
        ),
        (
            // An explicit JSON null normalises to ABSENT, so it agrees with a
            // fragment that declares no root at all.
            "null-source-root-normalises-to-absent",
            AssembleInput::default()
                .with_script(
                    "const x = 1\n",
                    &MapBuilder::new(&["a.vue"])
                        .source_root(Value::Null)
                        .segments(&[seg(0, 0, 1, 0)])
                        .build(),
                )
                .with_template(
                    "function render() {}\n",
                    &MapBuilder::new(&["b.vue"])
                        .segments(&[seg(0, 9, 9, 2)])
                        .build(),
                ),
        ),
        (
            // Generated-side metadata is DROPPED, never inherited: the input
            // declares `file`, `debugId` and an unknown extension member, and
            // none of the three may appear in the artifact.
            "generated-side-metadata-is-dropped",
            AssembleInput::default().with_script("const x = 1\n", &{
                let mut object = serde_json::Map::new();
                object.insert("version".to_string(), json!(3));
                object.insert("file".to_string(), json!("Comp.vue"));
                object.insert("debugId".to_string(), json!("0123abcd"));
                object.insert("x_verter_unknown_member".to_string(), json!([1, 2, 3]));
                object.insert("sources".to_string(), json!(["a.vue"]));
                object.insert("names".to_string(), json!([] as [&str; 0]));
                object.insert(
                    "mappings".to_string(),
                    json!(encode_mappings(&[seg(0, 0, 1, 0)])),
                );
                Value::Object(object).to_string()
            }),
        ),
    ]);
}

/// A present fragment whose map is legitimately absent contributes NOTHING to
/// the map, while its code is still written and (for the script) still
/// rewritten (§5.8).
#[test]
fn mapless_present_fragments_agree_across_implementations() {
    assert_cross_implementation_equality(&[
        (
            // A present, synthetic, MAPLESS script beside a mapped template.
            "mapless-script-mapped-template",
            AssembleInput::default()
                .with_synthetic_script("const __sfc__ = {}\nexport default __sfc__;\n", "")
                .with_template("function render() {}\n", &map_of(&[seg(0, 9, 9, 2)])),
        ),
        (
            // A mapped script beside a present, synthetic, MAPLESS template.
            // The template is not authored, so its empty map is not required.
            "mapped-script-mapless-template",
            {
                let mut input = AssembleInput::default()
                    .with_script("const x = 1\n", &map_of(&[seg(0, 0, 1, 0)]))
                    .with_template("function render() {}\n", "");
                input.authored_template = false;
                input
            },
        ),
        (
            // ZERO contributing maps with a map REQUESTED still yields a map —
            // the empty artifact — never "no map" (§7.7).
            "zero-contributing-maps-still-yields-a-map",
            AssembleInput::default().with_synthetic_script("const __sfc__ = {}\n", ""),
        ),
        (
            // A module with no fragments at all, maps requested.
            "no-fragments-maps-requested",
            AssembleInput::default(),
        ),
    ]);
}

// Cases — the assembler write grammar

/// Every write-site axis that moves a fragment's placement or changes the
/// module's bytes. Placement is DERIVED as each side writes, so a divergence in
/// the write grammar shows up as a coordinate divergence in the map.
#[test]
fn write_grammar_axes_agree_across_implementations() {
    let script_map = map_of(&[seg(0, 0, 1, 0), seg(0, 6, 1, 6)]);
    let template_map = map_of(&[seg(0, 9, 9, 2)]);
    let with_both = |input: AssembleInput| {
        // The template's generated code must actually match its own `ssr`
        // axis — real Vue codegen never pairs an SSR profile with a
        // VDOM-shaped `function render(` body (the two backends emit
        // different function names), so a vector that combines them is
        // not a case production has to agree with the reference on.
        let template_code = if input.ssr {
            "function ssrRender() {}\n"
        } else {
            "function render() {}\n"
        };
        input
            .with_script("const __sfc__ = {}\n", &script_map)
            .with_template(template_code, &template_map)
    };

    assert_cross_implementation_equality(&[
        (
            // Style virtual imports push both fragments down; the lang comes
            // from `styleLangs` with a `"css"` fallback, and the two lengths
            // legitimately differ.
            "style-imports",
            with_both(AssembleInput {
                style_count: 3,
                style_langs: vec![Some("scss".to_string()), None],
                ..AssembleInput::default()
            }),
        ),
        (
            // Custom-block imports, their invocations, and the `"custom"`
            // fallback type.
            "custom-block-imports",
            with_both(AssembleInput {
                custom_block_count: 2,
                custom_types: vec!["i18n".to_string()],
                ..AssembleInput::default()
            }),
        ),
        (
            "styles-and-custom-blocks-together",
            with_both(AssembleInput {
                style_count: 1,
                style_langs: vec![Some("less".to_string())],
                custom_block_count: 1,
                custom_types: vec!["docs".to_string()],
                ..AssembleInput::default()
            }),
        ),
        (
            // Template runtime imports, including the `_`-prefixed alias form.
            "template-runtime-imports",
            with_both(AssembleInput::default()).with_template_imports(
                &["_createElementVNode", "openBlock", "_toDisplayString"],
                &[],
            ),
        ),
        (
            "template-ssr-imports",
            with_both(AssembleInput {
                ssr: true,
                ..AssembleInput::default()
            })
            .with_template_imports(&["_createVNode"], &["_ssrRenderAttrs", "ssrInterpolate"]),
        ),
        (
            // A non-default runtime module name reaches both the template
            // import and the SSR context import.
            "custom-runtime-module-name",
            with_both(AssembleInput {
                runtime_module_name: Some("@vue/runtime-dom".to_string()),
                ssr: true,
                ..AssembleInput::default()
            })
            .with_template_imports(&["_createVNode"], &[]),
        ),
        (
            // Production strips `__file` and HMR.
            "production",
            with_both(AssembleInput {
                is_production: true,
                hmr_strategy: HmrStrategy::Vite,
                ..AssembleInput::default()
            }),
        ),
        (
            "hmr-vite",
            with_both(AssembleInput {
                hmr_strategy: HmrStrategy::Vite,
                ..AssembleInput::default()
            }),
        ),
        (
            "hmr-webpack",
            with_both(AssembleInput {
                hmr_strategy: HmrStrategy::Webpack,
                ..AssembleInput::default()
            }),
        ),
        (
            // SSR suppresses HMR even in dev, and appends the SSR wrapper.
            "ssr-suppresses-hmr",
            with_both(AssembleInput {
                ssr: true,
                hmr_strategy: HmrStrategy::Vite,
                ssr_module_id: Some("src/Comp.vue".to_string()),
                ..AssembleInput::default()
            }),
        ),
        (
            // The SSR registered id falls back to the canonical id.
            "ssr-module-id-falls-back-to-canonical",
            with_both(AssembleInput {
                ssr: true,
                ..AssembleInput::default()
            }),
        ),
        (
            // A canonical id needing Debug escaping at BOTH `{:?}` sites.
            "canonical-id-needing-debug-escaping",
            with_both(AssembleInput {
                canonical_id: "src/a \"quoted\"\\path/Comp.vue".to_string(),
                ssr: true,
                ssr_module_id: Some("a \"quoted\"\\id.vue".to_string()),
                ..AssembleInput::default()
            }),
        ),
        (
            // No script: the fallback object and the scope-id write, which only
            // exist on that branch.
            "no-script-with-scope-id",
            AssembleInput {
                scope_id: "data-v-abcdef12".to_string(),
                ..AssembleInput::default()
            }
            .with_template("function render() {}\n", &template_map),
        ),
        (
            // A scope id is NOT written when a script is present.
            "scope-id-ignored-when-script-present",
            with_both(AssembleInput {
                scope_id: "data-v-abcdef12".to_string(),
                ..AssembleInput::default()
            }),
        ),
        (
            // The `ssr` axis (not the template's generated text) decides
            // the SSR render binding.
            "ssr-render-binding",
            AssembleInput {
                ssr: true,
                ..AssembleInput::default()
            }
            .with_script("const __sfc__ = {}\n", &script_map)
            .with_template("function ssrRender() {}\n", &map_of(&[seg(0, 9, 9, 2)])),
        ),
        (
            // A present template ALWAYS carries a render binding — the
            // `ssr` axis decides WHICH one, never whether one exists.
            "non-ssr-render-binding",
            AssembleInput::default()
                .with_script("const __sfc__ = {}\n", &script_map)
                .with_template("function render() {}\n", &map_of(&[seg(0, 6, 9, 2)])),
        ),
        (
            // Maps NOT requested: the result carries no map at all — not an
            // empty map, not a map with empty `mappings` (§7.7). The fragment
            // map strings are non-empty and must be ignored, not composed
            // unasked.
            "maps-not-requested",
            with_both(AssembleInput {
                source_map_requested: false,
                ..AssembleInput::default()
            }),
        ),
        (
            // Maps not requested AND a structurally uncomposable fragment map:
            // no validation runs, so this composes rather than failing.
            "maps-not-requested-ignores-an-invalid-map",
            AssembleInput {
                source_map_requested: false,
                ..AssembleInput::default()
            }
            .with_script("const x = 1\n", "{ not json"),
        ),
        (
            // Everything at once.
            "all-axes-together",
            with_both(AssembleInput {
                canonical_id: "src/nested/Comp.vue".to_string(),
                style_count: 2,
                style_langs: vec![Some("scss".to_string()), Some("css".to_string())],
                custom_block_count: 1,
                custom_types: vec!["i18n".to_string()],
                runtime_module_name: Some("vue".to_string()),
                ssr: true,
                ssr_module_id: Some("nested/Comp.vue".to_string()),
                ..AssembleInput::default()
            })
            .with_template_imports(&["_createVNode"], &["_ssrRenderAttrs"]),
        ),
    ]);
}

// Cases — the fail-closed taxonomy

/// A raw map document written out member by member, for the shapes a JSON
/// serializer cannot produce: a duplicate member, a number outside the
/// interoperable domain, an unpaired surrogate.
fn raw_map(members: &str) -> String {
    format!("{{{members}}}")
}

/// Every `UncomposableInputMap` sub-code, and both `MissingRequiredInputMap`
/// fragments.
///
/// Each case asserts that the two implementations report the SAME outcome kind,
/// the same family, the same sub-code and the same fragment — the full
/// fail-closed outcome, not merely that both refused. Both sides already have
/// per-sub-code tests of their own; what this adds is that their two
/// independently derived taxonomies land on the same answer for one input.
#[test]
fn every_fail_closed_outcome_agrees_across_implementations() {
    const CODE: &str = "const x = 1\n";
    let good_template_map = map_of(&[seg(0, 9, 9, 2)]);

    // A script fragment carrying a raw map, so the outcome is attributed to the
    // script unless a case says otherwise.
    let bad_script = |raw: &str| AssembleInput::default().with_script(CODE, raw);

    let agreed = assert_cross_implementation_equality(&[
        // U1 — malformed map JSON
        ("U1.1 map-bytes-not-json", bad_script("{ not json")),
        ("U1.2 map-root-not-object", bad_script("[]")),
        (
            "U1.3 mappings-member-absent",
            bad_script(&raw_map(r#""version":3,"sources":["a.vue"],"names":[]"#)),
        ),
        (
            "U1.4 mappings-member-not-a-string",
            bad_script(&raw_map(
                r#""version":3,"sources":["a.vue"],"names":[],"mappings":0"#,
            )),
        ),
        (
            "U1.5 sources-member-absent-or-not-an-array",
            bad_script(&raw_map(r#""version":3,"names":[],"mappings":"""#)),
        ),
        (
            "U1.6 names-member-absent-or-not-an-array",
            bad_script(&raw_map(r#""version":3,"sources":["a.vue"],"mappings":"""#)),
        ),
        (
            "U1.7 metadata-member-wrong-type",
            bad_script(&raw_map(
                r#""version":3,"sources":["a.vue"],"names":[],"mappings":"","sourceRoot":7"#,
            )),
        ),
        (
            // The two ignore-list spellings are ONE field: where both appear
            // they must be deep-equal.
            "U1.7 disagreeing-ignore-list-spellings",
            bad_script(&raw_map(
                r#""version":3,"sources":["a.vue"],"names":[],"mappings":"","ignoreList":[0],"x_google_ignoreList":[]"#,
            )),
        ),
        (
            // The agreement is over the CONVERTED binary64 values: 2^64 and
            // 2^65 are distinct, legally-typed entries that both saturate to
            // the same value under an unsigned-64-bit pre-narrow, so only a
            // comparison at full binary64 identity sees the disagreement.
            "U1.7 disagreeing-ignore-list-spellings-beyond-u64",
            bad_script(&raw_map(
                r#""version":3,"sources":["a.vue"],"names":[],"mappings":"","ignoreList":[18446744073709551616],"x_google_ignoreList":[36893488147419103232]"#,
            )),
        ),
        (
            "U1.8 duplicate-object-member",
            bad_script(&raw_map(
                r#""version":3,"version":3,"sources":["a.vue"],"names":[],"mappings":"""#,
            )),
        ),
        (
            "U1.9 number-outside-interoperable-domain",
            bad_script(&raw_map(
                r#""version":3,"sources":["a.vue"],"names":[],"mappings":"","x_extension":1e400"#,
            )),
        ),
        (
            "U1.10 string-not-well-formed-unicode",
            bad_script(&raw_map(
                r#""version":3,"sources":["\ud800"],"names":[],"mappings":"""#,
            )),
        ),
        // U2 — version
        (
            "U2.1 version-member-absent",
            bad_script(&raw_map(r#""sources":["a.vue"],"names":[],"mappings":"""#)),
        ),
        (
            "U2.2 version-not-an-integer",
            bad_script(&raw_map(
                r#""version":3.5,"sources":["a.vue"],"names":[],"mappings":"""#,
            )),
        ),
        (
            "U2.2 version-not-a-number-at-all",
            bad_script(&raw_map(
                r#""version":"3","sources":["a.vue"],"names":[],"mappings":"""#,
            )),
        ),
        (
            "U2.3 version-not-3",
            bad_script(&raw_map(
                r#""version":2,"sources":["a.vue"],"names":[],"mappings":"""#,
            )),
        ),
        // U3 — wire data
        (
            "U3.1 vlq-invalid-character",
            bad_script(
                &MapBuilder::new(&["a.vue"])
                    .build()
                    .replace(r#""mappings":"""#, r#""mappings":"$""#),
            ),
        ),
        ("U3.2 vlq-truncated-segment", bad_script(&mappings_map("g"))),
        ("U3.3 segment-field-count", bad_script(&mappings_map("AC"))),
        (
            "U3.4 vlq-field-out-of-range",
            bad_script(&mappings_map("ggggggE")),
        ),
        (
            // A running accumulator driven negative.
            "U3.5 accumulator-out-of-range",
            bad_script(&mappings_map("D")),
        ),
        (
            // Within one line, a column strictly less than the previous one.
            "U3.6 generated-column-accumulator-decreased",
            bad_script(&mappings_map("E,D")),
        ),
        // U4 — table rows
        (
            "U4.1 source-row-not-a-string",
            bad_script(&raw_map(
                r#""version":3,"sources":[0],"names":[],"mappings":"""#,
            )),
        ),
        (
            "U4.2 name-row-not-a-string",
            bad_script(&raw_map(
                r#""version":3,"sources":["a.vue"],"names":[0],"mappings":"""#,
            )),
        ),
        (
            "U4.3 sources-content-row-not-string-or-null",
            bad_script(&raw_map(
                r#""version":3,"sources":["a.vue"],"names":[],"sourcesContent":[0],"mappings":"""#,
            )),
        ),
        (
            "U4.4 sources-content-length-mismatch",
            bad_script(&raw_map(
                r#""version":3,"sources":["a.vue"],"names":[],"sourcesContent":["x","y"],"mappings":"""#,
            )),
        ),
        // U5 — indexed map
        (
            "U5.1 sections-member-present",
            bad_script(&raw_map(r#""version":3,"sections":[]"#)),
        ),
        // U6 — dangling index
        (
            "U6.1 source-index-out-of-table",
            bad_script(&mappings_map("ACAA")),
        ),
        (
            "U6.2 name-index-out-of-table",
            bad_script(&mappings_map("AAAAA")),
        ),
        (
            "U6.3 ignore-list-index-out-of-table",
            bad_script(&raw_map(
                r#""version":3,"sources":["a.vue"],"names":[],"ignoreList":[5],"mappings":"""#,
            )),
        ),
        (
            // §4.3 step 1.15 places no upper bound on a legally-typed entry —
            // only step 1.23 (`U6.3`) does, against the real table. A value
            // beyond i32::MAX is not a wrong-type rejection.
            "U6.3 ignore-list-index-beyond-i32-max",
            bad_script(&raw_map(
                r#""version":3,"sources":[],"names":[],"ignoreList":[2147483648],"mappings":"""#,
            )),
        ),
        // ── U7 — coordinates, against the fragment's own PRE-REWRITE code ──
        (
            // `"const x = 1\n"` has two lines (0 and 1); line 2 is out.
            "U7.1 generated-line-out-of-fragment",
            bad_script(&mappings_map(";;A")),
        ),
        (
            // Line 0 is `"ab"`, so column 3 is past its end-of-line column.
            "U7.2 generated-column-out-of-fragment",
            AssembleInput::default().with_script("ab\n", &mappings_map("G")),
        ),
        (
            // Line 0 is one astral character — two UTF-16 units — so column 1
            // addresses no character boundary.
            "U7.3 generated-column-splits-a-surrogate-pair",
            AssembleInput::default().with_script("\u{1d400}\n", &mappings_map("C")),
        ),
        // U8 — cross-fragment metadata
        (
            // Two declared roots cannot both be represented. The conflict is
            // attributed to the LATER contributing map in fixed
            // script-then-template order.
            "U8.1 source-root-conflict",
            AssembleInput::default()
                .with_script(
                    CODE,
                    &MapBuilder::new(&["a.vue"])
                        .source_root(json!("/one"))
                        .build(),
                )
                .with_template(
                    "function render() {}\n",
                    &MapBuilder::new(&["b.vue"])
                        .source_root(json!("/two"))
                        .build(),
                ),
        ),
        (
            // Present versus absent is a conflict too: absence is not a
            // wildcard that agrees with anything.
            "U8.1 source-root-present-versus-absent",
            AssembleInput::default()
                .with_script(
                    CODE,
                    &MapBuilder::new(&["a.vue"])
                        .source_root(json!("/one"))
                        .build(),
                )
                .with_template(
                    "function render() {}\n",
                    &MapBuilder::new(&["b.vue"]).build(),
                ),
        ),
        (
            // An EMPTY root is a declared value, so it conflicts with absence
            // rather than being read as the identity root.
            "U8.1 empty-source-root-versus-absent",
            AssembleInput::default()
                .with_script(
                    CODE,
                    &MapBuilder::new(&["a.vue"]).source_root(json!("")).build(),
                )
                .with_template(
                    "function render() {}\n",
                    &MapBuilder::new(&["b.vue"]).build(),
                ),
        ),
        (
            // The conflict is reported against the template even when it is the
            // template that declares NO root — attribution follows contribution
            // order, not which side looks unusual.
            "U8.1 absent-source-root-first",
            AssembleInput::default()
                .with_script(CODE, &MapBuilder::new(&["a.vue"]).build())
                .with_template(
                    "function render() {}\n",
                    &MapBuilder::new(&["b.vue"])
                        .source_root(json!("/two"))
                        .build(),
                ),
        ),
        // The other fail-closed kind
        (
            "missing-required-script-map",
            AssembleInput::default().with_script(CODE, ""),
        ),
        (
            "missing-required-template-map",
            AssembleInput::default().with_template("function render() {}\n", ""),
        ),
        (
            // Both fragments authored, present and mapless: the SCRIPT is
            // reported, because the inventory is walked script-then-template.
            "missing-required-both-reports-script",
            AssembleInput::default()
                .with_script(CODE, "")
                .with_template("function render() {}\n", ""),
        ),
        (
            // An authored-but-ABSENT fragment requires nothing: the inline
            // topology, where the render closure lives inside `setup()`.
            "authored-but-absent-template-requires-nothing",
            {
                let mut input =
                    AssembleInput::default().with_script(CODE, &map_of(&[seg(0, 0, 1, 0)]));
                input.authored_template = true;
                input
            },
        ),
        // Precedence between checks
        (
            // Version beats indexed-map: a `version: 2` map that ALSO carries
            // `sections` reports the version.
            "precedence version-beats-indexed-map",
            bad_script(&raw_map(r#""version":2,"sections":[]"#)),
        ),
        (
            // Indexed-map beats absent `mappings`: an indexed map legitimately
            // has none.
            "precedence indexed-map-beats-absent-mappings",
            bad_script(&raw_map(
                r#""version":3,"sources":[],"names":[],"sections":[]"#,
            )),
        ),
        (
            // The SCRIPT map's checks run to completion first: a malformed
            // script map and a dangling-index template map report the script's.
            "precedence script-map-beats-template-map",
            AssembleInput::default()
                .with_script(CODE, "{ not json")
                .with_template("function render() {}\n", &mappings_map("ACAA")),
        ),
        (
            // Index bounds beat coordinate bounds as a STAGE: an index
            // violation in a LATER segment still beats a coordinate violation
            // in an earlier one.
            "precedence index-bounds-beat-coordinate-bounds",
            AssembleInput::default().with_script("ab\n", &mappings_map("G,ACAA")),
        ),
        (
            // A missing required map is decided BEFORE any present map is
            // validated: the template's map is structurally uncomposable, but
            // the script's absent required map is the reported outcome.
            "precedence missing-required-beats-uncomposable",
            AssembleInput::default()
                .with_script(CODE, "")
                .with_template("function render() {}\n", "{ not json"),
        ),
        (
            // A good template beside a bad script still fails closed — no
            // partial result, not even the half that composed.
            "no-partial-result-when-one-fragment-fails",
            AssembleInput::default()
                .with_script(CODE, "{ not json")
                .with_template("function render() {}\n", &good_template_map),
        ),
    ]);

    // The suite above is only worth what it exercises. Assert positively that
    // the agreed outcomes cover EVERY sub-code of the taxonomy, that both
    // fail-closed kinds occurred, and that both fragments were attributed — a
    // case set that collapsed onto one popular rejection would otherwise report
    // perfect agreement while testing almost nothing.
    let mut observed_codes: Vec<String> = agreed
        .iter()
        .filter_map(|outcome| match outcome {
            ComposeOutcome::UncomposableInputMap { code, .. } => Some(code.clone()),
            _ => None,
        })
        .collect();
    observed_codes.sort();
    observed_codes.dedup();

    let expected_codes: Vec<String> = every_uncomposable_code()
        .into_iter()
        .map(sub_code_str)
        .collect();
    let missing: Vec<&String> = expected_codes
        .iter()
        .filter(|code| !observed_codes.contains(code))
        .collect();
    assert!(
        missing.is_empty(),
        "these sub-codes are in the taxonomy but no case reaches them: {missing:?}"
    );

    let missing_fragments: Vec<&str> = ["script", "template"]
        .into_iter()
        .filter(|fragment| {
            !agreed.iter().any(|outcome| match outcome {
                ComposeOutcome::UncomposableInputMap { fragment: at, .. }
                | ComposeOutcome::MissingRequiredInputMap { fragment: at } => at == fragment,
                ComposeOutcome::Composed { .. } | ComposeOutcome::AssemblyFailed { .. } => false,
            })
        })
        .collect();
    assert!(
        missing_fragments.is_empty(),
        "no case attributes an outcome to these fragments: {missing_fragments:?}"
    );

    assert!(
        agreed
            .iter()
            .any(|outcome| matches!(outcome, ComposeOutcome::MissingRequiredInputMap { .. })),
        "no case reaches the missing-required-input-map outcome kind"
    );
}

/// Every sub-code in the taxonomy.
///
/// The list is kept complete by the compiler, not by review: [`code_ordinal`]
/// matches exhaustively, so a new variant fails to compile until it is given an
/// ordinal, and the round-trip assertion below then fails until it is also
/// listed here — at which point the coverage assertion demands a case for it.
fn every_uncomposable_code() -> Vec<UncomposableCode> {
    use UncomposableCode as C;
    let all = vec![
        C::MapBytesNotJson,
        C::MapRootNotObject,
        C::MappingsMemberAbsent,
        C::MappingsMemberNotAString,
        C::SourcesMemberAbsentOrNotAnArray,
        C::NamesMemberAbsentOrNotAnArray,
        C::MetadataMemberWrongType,
        C::DuplicateObjectMember,
        C::NumberOutsideInteroperableDomain,
        C::StringNotWellFormedUnicode,
        C::VersionMemberAbsent,
        C::VersionNotAnInteger,
        C::VersionNot3,
        C::VlqInvalidCharacter,
        C::VlqTruncatedSegment,
        C::SegmentFieldCount,
        C::VlqFieldOutOfRange,
        C::AccumulatorOutOfRange,
        C::GeneratedColumnAccumulatorDecreased,
        C::SourceRowNotAString,
        C::NameRowNotAString,
        C::SourcesContentRowNotStringOrNull,
        C::SourcesContentLengthMismatch,
        C::SectionsMemberPresent,
        C::SourceIndexOutOfTable,
        C::NameIndexOutOfTable,
        C::IgnoreListIndexOutOfTable,
        C::GeneratedLineOutOfFragment,
        C::GeneratedColumnOutOfFragment,
        C::GeneratedColumnSplitsASurrogatePair,
        C::SourceRootConflict,
    ];
    for (ordinal, code) in all.iter().enumerate() {
        assert_eq!(
            code_ordinal(*code),
            ordinal,
            "`every_uncomposable_code` is missing a variant or lists one out of order"
        );
    }
    all
}

/// Each sub-code's position in [`every_uncomposable_code`]. Exhaustive by
/// construction: adding a variant to the taxonomy breaks this match.
fn code_ordinal(code: UncomposableCode) -> usize {
    use UncomposableCode as C;
    match code {
        C::MapBytesNotJson => 0,
        C::MapRootNotObject => 1,
        C::MappingsMemberAbsent => 2,
        C::MappingsMemberNotAString => 3,
        C::SourcesMemberAbsentOrNotAnArray => 4,
        C::NamesMemberAbsentOrNotAnArray => 5,
        C::MetadataMemberWrongType => 6,
        C::DuplicateObjectMember => 7,
        C::NumberOutsideInteroperableDomain => 8,
        C::StringNotWellFormedUnicode => 9,
        C::VersionMemberAbsent => 10,
        C::VersionNotAnInteger => 11,
        C::VersionNot3 => 12,
        C::VlqInvalidCharacter => 13,
        C::VlqTruncatedSegment => 14,
        C::SegmentFieldCount => 15,
        C::VlqFieldOutOfRange => 16,
        C::AccumulatorOutOfRange => 17,
        C::GeneratedColumnAccumulatorDecreased => 18,
        C::SourceRowNotAString => 19,
        C::NameRowNotAString => 20,
        C::SourcesContentRowNotStringOrNull => 21,
        C::SourcesContentLengthMismatch => 22,
        C::SectionsMemberPresent => 23,
        C::SourceIndexOutOfTable => 24,
        C::NameIndexOutOfTable => 25,
        C::IgnoreListIndexOutOfTable => 26,
        C::GeneratedLineOutOfFragment => 27,
        C::GeneratedColumnOutOfFragment => 28,
        C::GeneratedColumnSplitsASurrogatePair => 29,
        C::SourceRootConflict => 30,
    }
}

/// A flat v3 map over one source with a hand-written `mappings` string, for the
/// wire-level rejections whose whole point is a `mappings` value no encoder
/// would produce.
fn mappings_map(mappings: &str) -> String {
    json!({
        "version": 3,
        "sources": ["a.vue"],
        "names": [],
        "mappings": mappings,
    })
    .to_string()
}

// Cases — genuine compiler output

/// One real compile: the canonical id, the neutral bundle the Vue carrier
/// produced, the parse-derived file meta, and the assembly profile.
struct RealCompile {
    id: String,
    canonical_id: String,
    compiled: RuntimeCompileOutput,
    meta: FileMeta,
    profile: CompileProfile,
    /// The topology the carrier actually resolved, read off the bundle rather
    /// than off the requested axis. Not currently read by any assertion — the
    /// carrier's inline/filename-omission defect this once distinguished is
    /// fixed at the test-fidelity level (`compile_fixture` now supplies the
    /// filename the real host always does) — kept for diagnostics if a case
    /// needs to branch on resolved topology again.
    #[allow(dead_code)]
    inline: bool,
}

/// The axes a real compile is driven over.
#[derive(Debug, Clone, Copy)]
struct CompileAxes {
    ssr: bool,
    is_production: bool,
    source_map: bool,
    /// `None` resolves to `is_production`, matching the carrier's own default.
    inline: Option<bool>,
    /// Vapor-mode codegen. Exclusive with `ssr` in every case below: the three
    /// backends a Vue cell can be compiled for are vdom (neither), vapor (this)
    /// and ssr (`ssr`).
    force_vapor: bool,
}

/// Compile a fixture through the REAL Vue pipeline: the parse snapshot supplies
/// the authored-fragment inventory and the style/custom-block tables exactly as
/// production derives them, and the carrier's `compile_bundle` supplies the
/// fragments. Nothing here is hand-built.
fn compile_fixture(fixture: &str, axes: CompileAxes) -> RealCompile {
    use oxc_allocator::Allocator;
    use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
    use verter_compiler::framework_common::{CarrierCompiler, RuntimeCompileOptions};

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/framework-conformance-harness/fixtures/vue")
        .join(fixture);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    // Checked-out text may carry either line ending; the composition is
    // CRLF-faithful, but a fixture whose bytes differ per platform would make
    // the case non-reproducible rather than exercising that faithfulness.
    let source = source.replace("\r\n", "\n");

    let canonical_id = format!("fixtures/vue/{fixture}");
    let provenance = crate::types::MetaProvenance::default();
    let (snapshot, artifact) = crate::parse::parse_vue_snapshot(
        &canonical_id,
        &source,
        verter_semantic::analysis::AnalysisScope::LSP,
        &provenance,
    );

    // The real host ALWAYS supplies a filename to the carrier — both call
    // sites in `virtual_file_pipeline.rs` derive it as
    // `profile.filename.clone().or_else(|| Some(snapshot.canonical_id.clone()))`,
    // never `None`. A `None` filename here would exercise a carrier input
    // shape the real host never produces, which is not a faithful case for a
    // suite whose whole point is comparing against how production actually
    // invokes the compiler.
    let allocator = Allocator::new();
    let compiled = VueCarrierCompiler
        .compile_bundle(
            &source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some(canonical_id.clone()),
                source_map: axes.source_map,
                ssr: axes.ssr,
                is_production: axes.is_production,
                inline: axes.inline,
                force_js: true,
                force_vapor: axes.force_vapor,
                ..RuntimeCompileOptions::default()
            },
            &allocator,
        )
        .expect("the Vue carrier produces a runtime bundle for a valid fixture")
        .into_produced()
        .expect("the Vue carrier produces a runtime surface; it never refuses one");

    let profile = CompileProfile {
        filename: Some(canonical_id.clone()),
        is_production: axes.is_production,
        ssr: axes.ssr,
        source_map: axes.source_map,
        inline: axes.inline,
        force_vapor: axes.force_vapor,
        hmr_strategy: HmrStrategy::Vite,
        ..CompileProfile::default()
    };

    RealCompile {
        id: format!(
            "{fixture} ssr={} vapor={} prod={} map={} inline={:?}",
            axes.ssr, axes.force_vapor, axes.is_production, axes.source_map, axes.inline
        ),
        canonical_id,
        inline: compiled.inline,
        compiled,
        meta: snapshot.meta,
        profile,
    }
}

/// Run real compiles through BOTH implementations.
///
/// Production runs on the genuine triple — the same call the host makes — while
/// the reference runs on the DTO projected out of that triple. Only the inputs
/// cross the bridge; neither side sees the other's output.
#[track_caller]
fn assert_real_compile_equality(cases: &[RealCompile]) -> Vec<ComposeOutcome> {
    assert!(!cases.is_empty(), "an empty case set proves nothing");

    let inputs: Vec<AssembleInput> = cases
        .iter()
        .map(|case| {
            AssembleInput::from_production_inputs(
                &case.canonical_id,
                &case.compiled,
                &case.meta,
                &case.profile,
            )
        })
        .collect();
    let reference = reference_outcomes(&inputs);

    let mut divergences = Vec::new();
    let mut agreed = Vec::with_capacity(cases.len());
    for (case, expected) in cases.iter().zip(reference) {
        let actual = match assemble_vue_main_module(
            &case.canonical_id,
            &case.compiled,
            &case.meta,
            &case.profile,
        ) {
            Ok(assembled) => ComposeOutcome::Composed {
                map: assembled
                    .source_map
                    .as_ref()
                    .map(|raw| compared_artifact("production", raw, &assembled.code)),
                code: assembled.code,
            },
            Err(VueMainAssemblyFailure::InputMap(
                AssembleMapFailure::MissingRequiredInputMap { fragment },
            )) => ComposeOutcome::MissingRequiredInputMap {
                fragment: fragment.as_str().to_string(),
            },
            Err(VueMainAssemblyFailure::InputMap(AssembleMapFailure::UncomposableInputMap {
                fragment,
                code,
            })) => ComposeOutcome::UncomposableInputMap {
                fragment: fragment.as_str().to_string(),
                family: family_str(code.family()).to_string(),
                code: sub_code_str(code),
            },
            Err(VueMainAssemblyFailure::InputMap(
                AssembleMapFailure::InvalidSfcExportPlacement { reason },
            )) => {
                // `case.compiled` is a GENUINE real-compile output (see
                // `compile_fixture`) — every in-scope producer declares a
                // consistent fact for its own bytes, so this is a real
                // producer defect, not a normal outcome the reference-side
                // JS oracle (which has no equivalent concept) could ever be
                // compared against.
                panic!(
                    "real compile {} produced an inconsistent __sfc__ export-\
                     placement fact: {reason:?}",
                    case.id
                );
            }
            Err(other) => panic!(
                "real compile {} produced a fragment/composition/publication failure — every \
                 real compile's fragments are, by construction, independently valid: {other:?}",
                case.id
            ),
        };
        if actual == expected {
            agreed.push(actual);
        } else {
            divergences.push(format!(
                "── {} ──\n  production: {actual:#?}\n  reference:  {expected:#?}",
                case.id
            ));
        }
    }

    assert!(
        divergences.is_empty(),
        "on genuine compiler output the production assembler and the independent JavaScript \
         reference disagree on {} of {} cases:\n\n{}",
        divergences.len(),
        cases.len(),
        divergences.join("\n\n")
    );
    agreed
}

/// Genuine compiler output, over the fixtures and the axes that change which
/// fragments exist and which bytes surround them.
///
/// The synthetic cases above pin the algebra at coordinates a compiler rarely
/// emits; these pin it at the ones it actually does — real maps, real table
/// contents, real fragment sizes — and additionally prove the input DTO carries
/// everything the real assembler reads.
#[test]
fn genuine_compiler_output_agrees_across_implementations() {
    let fixtures = [
        "basic-interpolation.vue",
        "props-emit.vue",
        // Template-only: the compiler synthesises a script block, so this is
        // the authored-versus-present distinction on real output.
        "slots.vue",
    ];
    let axes = [
        CompileAxes {
            ssr: false,
            is_production: false,
            source_map: true,
            inline: None,
            force_vapor: false,
        },
        CompileAxes {
            ssr: true,
            is_production: false,
            source_map: true,
            inline: None,
            force_vapor: false,
        },
        // Production, at the carrier's own default topology — which resolves
        // `inline` to `is_production`.
        CompileAxes {
            ssr: false,
            is_production: true,
            source_map: true,
            inline: None,
            force_vapor: false,
        },
        // Production with the render function kept standalone, so the
        // production axis is covered at both topologies.
        CompileAxes {
            ssr: false,
            is_production: true,
            source_map: true,
            inline: Some(false),
            force_vapor: false,
        },
        // Maps off: the result must carry NO map on either side.
        CompileAxes {
            ssr: false,
            is_production: false,
            source_map: false,
            inline: None,
            force_vapor: false,
        },
    ];

    let cases: Vec<RealCompile> = fixtures
        .iter()
        .flat_map(|fixture| axes.iter().map(move |axes| compile_fixture(fixture, *axes)))
        .collect();

    // EVERY map-requesting compile must actually carry a fragment map, or the
    // suite would be agreeing on empty artifacts and reporting that as
    // coverage of real compiler output.
    let unmapped: Vec<&str> = cases
        .iter()
        .filter(|case| case.profile.source_map)
        .filter(|case| {
            !case
                .compiled
                .script
                .as_ref()
                .is_some_and(|script| !script.source_map.is_empty())
                && !case
                    .compiled
                    .template
                    .as_ref()
                    .is_some_and(|template| !template.source_map.is_empty())
        })
        .map(|case| case.id.as_str())
        .collect();
    assert!(
        unmapped.is_empty(),
        "these map-requesting compiles carry no fragment map at all, so they compare empty \
         artifacts: {unmapped:?}"
    );

    let map_disabled_cases = cases.iter().filter(|case| !case.profile.source_map).count();
    let agreed = assert_real_compile_equality(&cases);

    let breakdown: Vec<String> = cases
        .iter()
        .zip(&agreed)
        .map(|(case, outcome)| match outcome {
            ComposeOutcome::Composed { map: Some(map), .. } => {
                format!("{} -> {} segments", case.id, map.segments.len())
            }
            other => format!("{} -> {other:?}", case.id),
        })
        .collect();

    // What was agreed on: real ordered segment sequences, not just the boundary
    // segment every mapped fragment contributes. The bar is deliberately modest
    // — today's runtime fragment maps are genuinely sparse (single digits per
    // fragment) — but it is not zero, which is the failure this guards.
    let richest = agreed
        .iter()
        .filter_map(|outcome| match outcome {
            ComposeOutcome::Composed { map: Some(map), .. } => Some(map.segments.len()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    assert!(
        richest >= 5,
        "no real compile agreed on more than {richest} segments, so this suite is comparing \
         near-empty artifacts:\n  {}",
        breakdown.join("\n  ")
    );

    // Every real compile driven the way the real host actually drives one —
    // filename always supplied, per `virtual_file_pipeline.rs`'s
    // `profile.filename.clone().or_else(|| Some(snapshot.canonical_id.clone()))`
    // — must compose successfully on both sides. An earlier revision of this
    // suite omitted `filename` from `compile_fixture`'s `RuntimeCompileOptions`
    // and observed the inline topology fail closed with a `U7` (generated
    // coordinates out of bounds): `CodeTransform`'s `Original` chunk only
    // advances the generated cursor when `source_id` is `Some(...)`
    // (`code_transform/source_map.rs:204`), so a `None` filename silently
    // drops original bytes from the map's coordinate accounting while later
    // chunks still advance, producing genuinely out-of-bounds coordinates.
    // That defect is real (see `filename_none_is_not_a_real_host_shape` below)
    // but is not reachable through the real host, which never supplies
    // `filename: None`. This assertion is therefore unconditional — no
    // topology is exempted — proving the fix, not merely documenting it.
    let unexpected_failures: Vec<&str> = cases
        .iter()
        .zip(&agreed)
        .filter(|(_case, outcome)| !matches!(outcome, ComposeOutcome::Composed { .. }))
        .map(|(case, _)| case.id.as_str())
        .collect();
    assert!(
        unexpected_failures.is_empty(),
        "these real compiles, driven exactly the way the real host drives one, fail closed: \
         {unexpected_failures:?}\n  {}",
        breakdown.join("\n  ")
    );

    let mapless_results = agreed
        .iter()
        .filter(|outcome| matches!(outcome, ComposeOutcome::Composed { map: None, .. }))
        .count();
    assert_eq!(
        mapless_results, map_disabled_cases,
        "every map-disabled compile must agree on carrying NO map"
    );
}

/// A real, narrow carrier defect — NOT reachable through the real host, which
/// never supplies `filename: None` (`virtual_file_pipeline.rs`'s
/// `profile.filename.clone().or_else(|| Some(snapshot.canonical_id.clone()))`
/// always resolves to `Some`). This is a regression/documentation test for
/// the defect itself, not a claim it affects production: with the carrier
/// given no filename, `CodeTransform`'s `Original` chunk only advances the
/// generated cursor when `source_id` is `Some(...)`
/// (`code_transform/source_map.rs:204`), so original bytes silently drop out
/// of the map's coordinate accounting while later chunks still advance,
/// producing genuinely out-of-bounds (`U7`) coordinates on the inline
/// topology. Both implementations agree it fails closed — this is a
/// composition-correctness agreement, not a divergence — but the underlying
/// carrier map/code mismatch is a real defect the emitter's true owner
/// should fix. It is scoped here rather than filed loosely so a future fix
/// has a discriminating test to turn green.
#[test]
fn filename_none_is_not_a_real_host_shape_and_the_carrier_defect_it_exposes_is_tracked() {
    use oxc_allocator::Allocator;
    use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
    use verter_compiler::framework_common::{CarrierCompiler, RuntimeCompileOptions};

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/framework-conformance-harness/fixtures/vue")
        .join("basic-interpolation.vue");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .replace("\r\n", "\n");

    let canonical_id = "fixtures/vue/basic-interpolation.vue".to_string();
    let provenance = crate::types::MetaProvenance::default();
    let (snapshot, artifact) = crate::parse::parse_vue_snapshot(
        &canonical_id,
        &source,
        verter_semantic::analysis::AnalysisScope::LSP,
        &provenance,
    );

    let allocator = Allocator::new();
    // `filename: None` — the shape the real host never produces.
    let compiled = VueCarrierCompiler
        .compile_bundle(
            &source,
            &artifact,
            &RuntimeCompileOptions {
                filename: None,
                source_map: true,
                ssr: false,
                is_production: true,
                inline: None, // resolves to `is_production` — the inline topology
                force_js: true,
                ..RuntimeCompileOptions::default()
            },
            &allocator,
        )
        .expect("the Vue carrier produces a runtime bundle for a valid fixture")
        .into_produced()
        .expect("the Vue carrier produces a runtime surface; it never refuses one");

    let profile = CompileProfile {
        filename: None,
        is_production: true,
        ssr: false,
        source_map: true,
        inline: None,
        hmr_strategy: HmrStrategy::Vite,
        ..CompileProfile::default()
    };

    let case = RealCompile {
        id: "basic-interpolation.vue filename=None (known carrier defect)".to_string(),
        canonical_id,
        inline: compiled.inline,
        compiled,
        meta: snapshot.meta,
        profile,
    };

    // `assert_real_compile_equality` itself asserts the two implementations
    // AGREE — which they do, both failing closed identically — and returns
    // the agreed outcome. What this test additionally checks is that the
    // agreed outcome really is a fail-closed one, not a successful
    // composition (which would mean the carrier defect is fixed).
    let agreed = assert_real_compile_equality(std::slice::from_ref(&case));
    assert!(
        matches!(
            agreed[0],
            ComposeOutcome::UncomposableInputMap { .. }
                | ComposeOutcome::MissingRequiredInputMap { .. }
        ),
        "expected the known `filename: None` carrier defect to still fail closed (documenting it, \
         not asserting production behaviour — the real host never omits filename, see \
         `genuine_compiler_output_agrees_across_implementations`) — got {:?}. If this now \
         composes, the carrier defect is fixed: delete this test and its citation.",
        agreed[0],
    );
}

// The comparator's own discrimination
//
// Everything above compares two implementations and passes when they agree.
// That is silent about the comparator itself: a [`ComparedArtifact`] that
// stopped reading `sourceRoot`, or a comparison that dropped a member, would
// let both sides "agree" on a divergence neither of them has. That failure
// mode must be provably caught: a comparator that silently ignores one
// field must be provably caught.
//
// The cases below supply the missing direction. Each takes ONE real production
// artifact, perturbs exactly ONE member of the emitted document, runs BOTH
// through the real [`compared_artifact`] reader, and asserts the perturbation
// is visible in that member and in NO other. A reader that stopped populating
// the member decodes both copies identically and the case fails.

/// The raw emitted artifact and the code it describes, for an input that
/// composes. The map is taken from the genuine assembler, not hand-built, so
/// what the cases perturb is a document production really emits.
fn composed_artifact_json(input: &AssembleInput) -> (String, String) {
    let (compiled, meta, profile) = input.to_production_inputs();
    let assembled = assemble_vue_main_module(&input.canonical_id, &compiled, &meta, &profile)
        .expect("this input composes");
    let raw = assembled
        .source_map
        .expect("a map was requested, so one was produced");
    (raw, assembled.code)
}

/// Every member of [`ComparedArtifact`] whose value differs, in declaration
/// order.
///
/// The artifact is destructured WITHOUT a `..` rest pattern, so adding a member
/// to the compared shape stops this function compiling until the member is
/// listed here — and [`COMPARED_MEMBERS`] then demands a perturbation case for
/// it. The per-member coverage below is kept complete by the compiler rather
/// than by review.
fn differing_compared_members(
    left: &ComparedArtifact,
    right: &ComparedArtifact,
) -> Vec<&'static str> {
    let ComparedArtifact {
        source_root,
        names,
        sources,
        sources_content,
        ignore_list,
        mappings,
        segments,
    } = left;

    let mut differing = Vec::new();
    if *source_root != right.source_root {
        differing.push("source_root");
    }
    if *names != right.names {
        differing.push("names");
    }
    if *sources != right.sources {
        differing.push("sources");
    }
    if *sources_content != right.sources_content {
        differing.push("sources_content");
    }
    if *ignore_list != right.ignore_list {
        differing.push("ignore_list");
    }
    if *mappings != right.mappings {
        differing.push("mappings");
    }
    if *segments != right.segments {
        differing.push("segments");
    }
    differing
}

/// The compared members, in [`differing_compared_members`]' own order. Every
/// one needs a case in [`the_comparator_discriminates_every_compared_member`].
const COMPARED_MEMBERS: &[&str] = &[
    "source_root",
    "names",
    "sources",
    "sources_content",
    "ignore_list",
    "mappings",
    "segments",
];

/// A case exercising every compared member at once: two contributing
/// fragments, multi-row tables, declared content, an ignore list on both
/// fragments, an agreeing `sourceRoot`, and named segments.
fn every_member_case() -> AssembleInput {
    AssembleInput::default()
        .with_script(
            "const x = 1\n",
            &MapBuilder::new(&["a.vue", "b.vue"])
                .names(&["one", "two"])
                .sources_content(&[Some("A"), Some("B")])
                .ignore_list(&[1])
                .source_root(json!("/src"))
                .segments(&[named(0, 0, 1, 0, 1)])
                .build(),
        )
        .with_template(
            "function render() {}\n",
            &MapBuilder::new(&["c.vue"])
                .names(&["three"])
                .sources_content(&[Some("C")])
                .ignore_list(&[0])
                .source_root(json!("/src"))
                .segments(&[named(0, 9, 9, 2, 0)])
                .build(),
        )
}

/// The ignore list's spelling in an emitted document. One logical member, two
/// accepted keys (§7.3/§7.8) — a perturbation has to find the one this encoder
/// wrote rather than assume either.
fn emitted_ignore_list_member(object: &serde_json::Map<String, Value>) -> &'static str {
    if object.contains_key("ignoreList") {
        "ignoreList"
    } else {
        "x_google_ignoreList"
    }
}

fn swap_first_two_rows(object: &mut serde_json::Map<String, Value>, member: &str) {
    let rows = object
        .get_mut(member)
        .and_then(Value::as_array_mut)
        .unwrap_or_else(|| panic!("the emitted artifact carries `{member}`"));
    assert!(
        rows.len() >= 2,
        "`{member}` needs two rows for a swap to be observable, got {}",
        rows.len()
    );
    rows.swap(0, 1);
}

fn emitted_mappings(object: &serde_json::Map<String, Value>) -> String {
    object
        .get("mappings")
        .and_then(Value::as_str)
        .expect("the emitted artifact carries `mappings`")
        .to_string()
}

/// An edit to an emitted artifact, applied in place.
type Perturbation = fn(&mut serde_json::Map<String, Value>);

/// `(the member it targets, the edit, the members that edit must move)`.
type MemberCase = (&'static str, Perturbation, &'static [&'static str]);

/// Apply one perturbation to an emitted artifact and hand back the document.
fn perturbed(raw: &str, perturb: Perturbation) -> String {
    let mut document: Value = serde_json::from_str(raw).expect("the artifact is JSON");
    let object = document
        .as_object_mut()
        .expect("the artifact is a JSON object");
    perturb(object);
    Value::Object(object.clone()).to_string()
}

/// Every compared member is genuinely compared.
///
/// One production artifact, one perturbation per member, each asserted to move
/// THAT member and no other. The final coverage assertion is what keeps this
/// honest as the compared shape changes: a new member added to
/// [`ComparedArtifact`] breaks [`differing_compared_members`]' destructure,
/// and listing it in [`COMPARED_MEMBERS`] then fails here until it has a case.
#[test]
fn the_comparator_discriminates_every_compared_member() {
    let (raw, code) = composed_artifact_json(&every_member_case());
    let base = compared_artifact("base", &raw, &code);

    // A byte-identical re-read must compare EQUAL. Without this, a reader that
    // returned something different every call would pass every case below for
    // exactly the wrong reason.
    assert!(
        differing_compared_members(&base, &compared_artifact("copy", &raw, &code)).is_empty(),
        "re-reading one artifact must produce one value"
    );

    // `(member, perturbation, the members it must move)`. Every entry but the
    // last moves exactly one member; `segments` is derived from `mappings` and
    // so cannot be moved alone — see its note below.
    let cases: &[MemberCase] = &[
        (
            "source_root",
            |object| {
                object.insert("sourceRoot".to_string(), json!("/other"));
            },
            &["source_root"],
        ),
        (
            "names",
            |object| swap_first_two_rows(object, "names"),
            &["names"],
        ),
        (
            "sources",
            |object| swap_first_two_rows(object, "sources"),
            &["sources"],
        ),
        (
            "sources_content",
            |object| {
                let rows = object
                    .get_mut("sourcesContent")
                    .and_then(Value::as_array_mut)
                    .expect("the emitted artifact carries `sourcesContent`");
                rows[0] = json!("perturbed");
            },
            &["sources_content"],
        ),
        (
            "ignore_list",
            |object| {
                let member = emitted_ignore_list_member(object);
                let entries = object
                    .get_mut(member)
                    .and_then(Value::as_array_mut)
                    .expect("the emitted artifact carries an ignore list");
                // Another IN-BOUNDS row, so the document stays valid and the
                // only thing that moved is which row is ignored.
                entries[0] = json!(0);
            },
            &["ignore_list"],
        ),
        (
            "mappings",
            |object| {
                // A trailing group. §7.6: "encoding stops after the last
                // segment-bearing line: no trailing `;` group" — and it decodes
                // to no extra segment, which is the whole point of this row.
                let mappings = emitted_mappings(object);
                object.insert("mappings".to_string(), json!(format!("{mappings};")));
            },
            &["mappings"],
        ),
        (
            "segments",
            |object| {
                // Drop the last generated line's segments. `segments` is
                // DECODED from `mappings`, so no perturbation can move it
                // alone; the independence that matters is the converse — a
                // `mappings` change the segment sequence does not show — and
                // the row above proves that direction.
                let mappings = emitted_mappings(object);
                let (head, _) = mappings
                    .rsplit_once(';')
                    .expect("the artifact spans several generated lines");
                object.insert("mappings".to_string(), json!(head));
            },
            &["mappings", "segments"],
        ),
    ];

    let exercised: Vec<&str> = cases.iter().map(|(member, _, _)| *member).collect();
    assert_eq!(
        exercised, COMPARED_MEMBERS,
        "every compared member needs a case, in declaration order"
    );

    for (member, perturb, expected) in cases {
        let document = perturbed(&raw, *perturb);
        assert_ne!(
            document, raw,
            "the `{member}` perturbation did not change the document"
        );

        let other = compared_artifact(member, &document, &code);
        assert_eq!(
            differing_compared_members(&base, &other),
            *expected,
            "perturbing `{member}` must move exactly {expected:?}"
        );
        assert_ne!(
            base, other,
            "the comparator must reject a `{member}` divergence"
        );
    }
}

/// A `mappings` divergence is caught even when the decoded segments AGREE.
///
/// The compared artifact carries both the `mappings` string and the sequence it
/// decodes to, and it is easy to read that as redundant. It is not: a trailing
/// `;` group encodes a generated line beyond the last segment-bearing one, so
/// it decodes to no extra segment and the two sequences stay identical while
/// the wire strings differ. §7.6 forbids that encoding, and only the string
/// member can see it — drop `mappings` from the comparison and this divergence
/// becomes invisible.
#[test]
fn a_mappings_divergence_is_caught_even_when_the_decoded_segments_agree() {
    let (raw, code) = composed_artifact_json(&every_member_case());
    let base = compared_artifact("base", &raw, &code);

    let document = perturbed(&raw, |object| {
        let mappings = emitted_mappings(object);
        object.insert("mappings".to_string(), json!(format!("{mappings};")));
    });
    let trailing = compared_artifact("trailing-group", &document, &code);

    assert_eq!(
        base.segments, trailing.segments,
        "a trailing empty group decodes to no additional segment — if this ever \
         stops holding, the case below is no longer about `mappings` alone"
    );
    assert_ne!(
        base.mappings, trailing.mappings,
        "the wire strings differ by exactly the trailing group"
    );
    assert_ne!(
        base, trailing,
        "so the comparison must fail through the `mappings` member alone"
    );
}

/// Layer-2 vector inventory: every frozen entry against its `expected`.
/// Child of this harness (no second DTO). Ungated.
mod vector_inventory;

/// BF2 seed matrix. Child of this harness (no second DTO). Gated: needs
/// the provisioned oracle install.
#[cfg(feature = "bf2-authoritative")]
mod bf2_seed_matrix;

/// The full-axis gate over the same 36-cell seed matrix: unlike
/// [`bf2_seed_matrix`], which deliberately does NOT gate on the oracle's
/// non-wire mapping verdict, this module requires the FULL verdict (parse,
/// link, structural, diagnostics, mapping, runtime) to pass for every cell.
#[cfg(feature = "bf2-authoritative")]
mod bf2_full_axis_gate;

/// Nested `v-for`/`v-if` runtime mount through pinned with-vapor, not a
/// string match. Child of this harness. Gated.
#[cfg(feature = "bf2-authoritative")]
mod nested_v_for_runtime_proof;

/// Svelte golden inventory + shipped `.svelte` route. Same manifest and
/// digest helpers. Gated (oracle install).
#[cfg(feature = "bf2-authoritative")]
mod svelte_official_conformance_matrix;

/// The authoritative full-axis gate over the six committed Svelte CLIENT
/// cells, plus the recorded shipped-route behaviour of the six SERVER cells
/// and the mutation-discrimination proof that the oracle behind it genuinely
/// discriminates. Gated on the same feature for the same reason.
#[cfg(feature = "bf2-authoritative")]
mod svelte_official_conformance_gate;

/// Every in-scope PublicApi / TSC / declaration cell, observed by the REAL
/// TypeScript compiler inside the pinned framework closure the harness
/// realizes. Gated on the same feature: it drives that realized install.
#[cfg(feature = "bf2-authoritative")]
mod public_api_typescript_observation;

/// The IDE/TSX product family, observed by the REAL TypeScript compiler inside
/// the workspace declaration domain (`@verter/svelte-jsx`, `@verter/types`).
/// Gated on the same feature: it drives the same observation validator.
#[cfg(feature = "bf2-authoritative")]
mod ide_surface_typescript_observation;
