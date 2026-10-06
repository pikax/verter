//! Tests for generated-unit admission.
//!
//! Every snapshot is built through the PRODUCTION config parse chain
//! (`snapshot_builder::configured_project` over a `MemoryWorkspace`, whose walk
//! materializes each project's program file set), so admission is exercised
//! against real `files`/`include`/`exclude`/`extends`/`references` semantics AND
//! against populated `materialized_files` — the shape in which an off-disk
//! generated unit is never a walk-time program member.

use std::sync::Arc;

use crate::canonical_path::CanonicalPath;
use crate::memory::{MemoryOptions, MemoryWorkspace};
use crate::snapshot_builder::{build_workspace_snapshot_simple, configured_project};
use crate::workspace_snapshot::{SnapshotGeneration, WorkspaceSnapshot};
use verter_session_query::resolution::ProjectId;

use super::{
    decide_generated_unit_admission, decide_generated_unit_admission_with_basis,
    GeneratedUnitAdmission, GeneratedUnitNonAdmissionReason,
};

const ROOT: &str = "d:/ws";
const CARRIER: &str = "d:/ws/src/Foo.vue";
const IDE_TSX: &str = "d:/ws/src/Foo.vue.tsx";
const IDE_JSX: &str = "d:/ws/src/Foo.vue.jsx";
const API_TS: &str = "d:/ws/src/Foo.vue.verter.ts";
const SIDECAR_DTS: &str = "d:/ws/src/Foo.vue.tsx.__verter_types.d.ts";

/// A snapshot over `configs` (`(tsconfig path, body)`), with the carrier and one
/// plain module on "disk". Extra `files` are non-project files (e.g. an
/// `extends` base).
fn snapshot(configs: &[(&str, &str)], files: &[(&str, &str)]) -> WorkspaceSnapshot {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![ROOT.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(CARRIER.to_string(), Arc::<str>::from("<template/>"));
    ws.inject_file(
        "d:/ws/src/main.ts".to_string(),
        Arc::<str>::from("export {}"),
    );
    for (path, body) in configs.iter().chain(files) {
        ws.inject_file((*path).to_string(), Arc::<str>::from(*body));
    }
    let root = CanonicalPath::new(ROOT);
    let projects = configs
        .iter()
        .enumerate()
        .map(|(i, (tsconfig, _))| {
            configured_project(&ws, tsconfig, ROOT, &root, ProjectId(i as u32))
        })
        .collect();
    build_workspace_snapshot_simple(projects, SnapshotGeneration(1))
}

fn units(paths: &[&str]) -> Vec<CanonicalPath> {
    paths.iter().map(|p| CanonicalPath::new(p)).collect()
}

fn decide(snapshot: &WorkspaceSnapshot, tsconfig: &str, paths: &[&str]) -> GeneratedUnitAdmission {
    decide_generated_unit_admission(snapshot, &CanonicalPath::new(tsconfig), &units(paths))
}

/// `Ok(())` when admitted, else the offending `(unit, reason)` rows.
fn verdict(
    snapshot: &WorkspaceSnapshot,
    tsconfig: &str,
    paths: &[&str],
) -> Result<(), Vec<(String, GeneratedUnitNonAdmissionReason)>> {
    match decide(snapshot, tsconfig, paths) {
        GeneratedUnitAdmission::Admitted(_) => Ok(()),
        GeneratedUnitAdmission::NotAdmitted(refusal) => Err(refusal
            .offending()
            .iter()
            .map(|(unit, reason)| (unit.as_str().to_string(), *reason))
            .collect()),
    }
}

use GeneratedUnitNonAdmissionReason::{
    Excluded, NoSuchConfiguredProject, NotMatchedByIncludeOrFiles, OwnedByDifferentProject,
};

const TSCONFIG: &str = "d:/ws/tsconfig.json";

/// One owning project, one membership shape per row: which units it admits.
#[test]
fn single_project_membership_shapes() {
    struct Case {
        name: &'static str,
        tsconfig: &'static str,
        extra_files: &'static [(&'static str, &'static str)],
        units: &'static [&'static str],
        expected: Result<(), &'static [(&'static str, GeneratedUnitNonAdmissionReason)]>,
    }
    const EXTENSION_SPECIFIC: &str =
        r#"{ "include": ["src/**/*.ts", "src/**/*.js", "src/**/*.vue", "src/**/*.d.ts"] }"#;
    let cases = [
        // The extension-specific include OWNS `Foo.vue` yet matches neither IDE
        // companion form.
        Case {
            name: "extension-specific include refuses the IDE companions",
            tsconfig: EXTENSION_SPECIFIC,
            extra_files: &[],
            units: &[IDE_TSX, IDE_JSX],
            expected: Err(&[
                (IDE_JSX, NotMatchedByIncludeOrFiles),
                (IDE_TSX, NotMatchedByIncludeOrFiles),
            ]),
        },
        // …while its `*.ts` / `*.d.ts` globs DO match the import surface and the
        // declaration sidecar.
        Case {
            name: "extension-specific include admits the .ts-suffixed units",
            tsconfig: EXTENSION_SPECIFIC,
            extra_files: &[],
            units: &[API_TS, SIDECAR_DTS],
            expected: Ok(()),
        },
        // One unadmitted unit refuses the WHOLE proposed set, naming only the
        // offender.
        Case {
            name: "one unadmitted unit refuses the whole set",
            tsconfig: EXTENSION_SPECIFIC,
            extra_files: &[],
            units: &[API_TS, IDE_TSX, SIDECAR_DTS],
            expected: Err(&[(IDE_TSX, NotMatchedByIncludeOrFiles)]),
        },
        Case {
            name: "directory include admits every TS-family unit",
            tsconfig: r#"{ "include": ["src"] }"#,
            extra_files: &[],
            units: &[IDE_TSX, API_TS, SIDECAR_DTS],
            expected: Ok(()),
        },
        Case {
            name: "files-only project naming just the carrier refuses the companion",
            tsconfig: r#"{ "files": ["src/Foo.vue"] }"#,
            extra_files: &[],
            units: &[IDE_TSX],
            expected: Err(&[(IDE_TSX, NotMatchedByIncludeOrFiles)]),
        },
        Case {
            name: "files-only project naming the companion admits it",
            tsconfig: r#"{ "files": ["src/Foo.vue", "src/Foo.vue.tsx"] }"#,
            extra_files: &[],
            units: &[IDE_TSX],
            expected: Ok(()),
        },
        Case {
            name: "exclude removing a companion the include matched",
            tsconfig: r#"{ "include": ["src"], "exclude": ["src/**/*.vue.tsx"] }"#,
            extra_files: &[],
            units: &[IDE_TSX, API_TS],
            expected: Err(&[(IDE_TSX, Excluded)]),
        },
        Case {
            name: "include inherited through extends is the one evaluated",
            tsconfig: r#"{ "extends": "./tsconfig.base.json" }"#,
            extra_files: &[(
                "d:/ws/tsconfig.base.json",
                r#"{ "include": ["src/**/*.ts", "src/**/*.vue"] }"#,
            )],
            units: &[API_TS, IDE_TSX],
            expected: Err(&[(IDE_TSX, NotMatchedByIncludeOrFiles)]),
        },
        // The JS family is a member only under allowJs / checkJs.
        Case {
            name: "JS companion without allowJs",
            tsconfig: r#"{ "include": ["src"] }"#,
            extra_files: &[],
            units: &[IDE_JSX],
            expected: Err(&[(IDE_JSX, NotMatchedByIncludeOrFiles)]),
        },
        Case {
            name: "JS companion with allowJs",
            tsconfig: r#"{ "compilerOptions": { "allowJs": true }, "include": ["src"] }"#,
            extra_files: &[],
            units: &[IDE_JSX],
            expected: Ok(()),
        },
        Case {
            name: "JS companion with checkJs",
            tsconfig: r#"{ "compilerOptions": { "checkJs": true }, "include": ["src"] }"#,
            extra_files: &[],
            units: &[IDE_JSX],
            expected: Ok(()),
        },
    ];

    for case in cases {
        let snap = snapshot(&[(TSCONFIG, case.tsconfig)], case.extra_files);
        let expected = case.expected.map_err(|rows| {
            rows.iter()
                .map(|(unit, reason)| ((*unit).to_string(), *reason))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            verdict(&snap, TSCONFIG, case.units),
            expected,
            "{}",
            case.name
        );
    }
}

/// The measured shape still OWNS the carrier source. Under the IDENTITY basis
/// (the unit derives from no carrier) ownership and generated-unit admission
/// stay separate facts that disagree here; the production companion basis —
/// which maps `Foo.vue.tsx` back to `Foo.vue` — is exercised by the
/// carrier-basis tests below.
#[test]
fn carrier_ownership_does_not_imply_generated_unit_admission() {
    let snap = snapshot(
        &[(
            TSCONFIG,
            r#"{ "include": ["src/**/*.ts", "src/**/*.vue"] }"#,
        )],
        &[],
    );
    assert!(
        snap.default_configured_owner_for_file(CARRIER).is_some(),
        "the project owns the carrier source"
    );
    assert_eq!(
        verdict(&snap, TSCONFIG, &[IDE_TSX]),
        Err(vec![(IDE_TSX.to_string(), NotMatchedByIncludeOrFiles)])
    );
}

/// The production basis the verter_lsp call sites hand in: every companion
/// form of `Foo.vue` reverse-maps to its carrier source (a stand-in for the
/// registry's `classify_carrier_companion`, which this layer cannot see).
fn carrier_basis(unit: &CanonicalPath) -> CanonicalPath {
    let name = unit.as_str();
    match name.find(".vue.") {
        // `Foo.vue.tsx` / `Foo.vue.jsx` / `Foo.vue.verter.ts` / the sidecar —
        // everything from the `.vue.` infix on is the generated form.
        Some(idx) => CanonicalPath::new(&name[..idx + ".vue".len()]),
        None => unit.clone(),
    }
}

fn verdict_with_basis(
    snapshot: &WorkspaceSnapshot,
    tsconfig: &str,
    paths: &[&str],
) -> Result<(), Vec<(String, GeneratedUnitNonAdmissionReason)>> {
    match decide_generated_unit_admission_with_basis(
        snapshot,
        &CanonicalPath::new(tsconfig),
        &units(paths),
        carrier_basis,
    ) {
        GeneratedUnitAdmission::Admitted(_) => Ok(()),
        GeneratedUnitAdmission::NotAdmitted(refusal) => Err(refusal
            .offending()
            .iter()
            .map(|(unit, reason)| (unit.as_str().to_string(), *reason))
            .collect()),
    }
}

/// The engine maps a companion through its carrier source's project, so the
/// extension-specific include that OWNS `Foo.vue` ADMITS the companions
/// projected from it — the baseline-serving model — while every genuinely
/// refused shape stays refused under its own typed reason.
#[test]
fn carrier_basis_membership_shapes() {
    const EXTENSION_SPECIFIC: &str =
        r#"{ "include": ["src/**/*.ts", "src/**/*.js", "src/**/*.vue", "src/**/*.d.ts"] }"#;
    struct Case {
        name: &'static str,
        tsconfig: &'static str,
        extra_files: &'static [(&'static str, &'static str)],
        units: &'static [&'static str],
        expected: Result<(), &'static [(&'static str, GeneratedUnitNonAdmissionReason)]>,
    }
    let cases = [
        // The extension-specific include owns `Foo.vue`; through the carrier
        // basis its IDE companions are members of the same project.
        Case {
            name: "extension-specific include admits the IDE companions via the carrier basis",
            tsconfig: EXTENSION_SPECIFIC,
            extra_files: &[],
            units: &[IDE_TSX, IDE_JSX, API_TS],
            expected: Ok(()),
        },
        // An excluded basis (the js-lax shape) refuses the family: the exclude
        // removes both the companion's own form and its carrier source.
        Case {
            name: "exclude removing the carrier source refuses its companion",
            tsconfig: r#"{ "include": ["src"], "exclude": ["src/lax"] }"#,
            extra_files: &[("d:/ws/src/lax/Foo.vue", "<template/>")],
            units: &["d:/ws/src/lax/Foo.vue.tsx"],
            expected: Err(&[("d:/ws/src/lax/Foo.vue.tsx", Excluded)]),
        },
        // A config that excludes the companion's own form refuses it even
        // though its basis is admitted: the unit's own form decides first.
        Case {
            name: "exclude removing the companion form refuses it despite the admitted basis",
            tsconfig: r#"{ "include": ["src"], "exclude": ["src/**/*.vue.tsx"] }"#,
            extra_files: &[],
            units: &[IDE_TSX, API_TS],
            expected: Err(&[(IDE_TSX, Excluded)]),
        },
        // A basis the include never matches (the carrier is not a member)
        // refuses the companion: no membership anywhere.
        Case {
            name: "include matching neither the unit nor its basis refuses it",
            tsconfig: r#"{ "include": ["src/**/*.ts"] }"#,
            extra_files: &[],
            units: &[IDE_TSX],
            expected: Err(&[(IDE_TSX, NotMatchedByIncludeOrFiles)]),
        },
        // `files` are exact and exclude-immune: naming just the carrier admits
        // its companions through the basis.
        Case {
            name: "files-only project naming just the carrier admits the companion",
            tsconfig: r#"{ "files": ["src/Foo.vue"] }"#,
            extra_files: &[],
            units: &[IDE_TSX],
            expected: Ok(()),
        },
    ];

    for case in cases {
        let snap = snapshot(&[(TSCONFIG, case.tsconfig)], case.extra_files);
        let expected = case.expected.map_err(|rows| {
            rows.iter()
                .map(|(unit, reason)| ((*unit).to_string(), *reason))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            verdict_with_basis(&snap, TSCONFIG, case.units),
            expected,
            "{}",
            case.name
        );
    }
}

/// Two extension-specific projects both own the carrier source; the basis's
/// default-owner walk resolves to the name-least one, so the companion is
/// admitted THERE and refused for the other (it would serve under the winner's
/// options).
#[test]
fn second_project_winning_the_basis_walk_is_a_non_admission() {
    const A: &str = "d:/ws/tsconfig.a.json";
    const Z: &str = "d:/ws/tsconfig.z.json";
    let snap = snapshot(
        &[
            (
                A,
                r#"{ "compilerOptions": { "strict": false }, "include": ["src/**/*.vue"] }"#,
            ),
            (
                Z,
                r#"{ "compilerOptions": { "strict": true }, "include": ["src/**/*.vue"] }"#,
            ),
        ],
        &[],
    );
    assert_eq!(verdict_with_basis(&snap, A, &[IDE_TSX]), Ok(()));
    assert_eq!(
        verdict_with_basis(&snap, Z, &[IDE_TSX]),
        Err(vec![(IDE_TSX.to_string(), OwnedByDifferentProject)])
    );
}

#[test]
fn unknown_owning_tsconfig_is_no_such_configured_project() {
    let snap = snapshot(&[(TSCONFIG, r#"{ "include": ["src"] }"#)], &[]);
    assert_eq!(
        verdict(&snap, "d:/ws/tsconfig.missing.json", &[IDE_TSX]),
        Err(vec![(IDE_TSX.to_string(), NoSuchConfiguredProject)])
    );
}

/// An empty proposal refuses nothing, whichever project it names: a refusal
/// always carries the unit that caused it.
#[test]
fn empty_unit_set_is_admitted_even_for_an_unknown_tsconfig() {
    let snap = snapshot(&[(TSCONFIG, r#"{ "include": ["src"] }"#)], &[]);
    assert_eq!(verdict(&snap, "d:/ws/tsconfig.missing.json", &[]), Ok(()));
    assert_eq!(verdict(&snap, TSCONFIG, &[]), Ok(()));
}

/// Project B owns the carrier and matches its companion, but project A — with
/// different compiler options — also matches the companion and WINS the
/// default-owner walk (name-least fallback). An engine would serve the companion
/// under A, so it is not admitted to B.
#[test]
fn second_project_that_would_own_the_unit_is_a_non_admission() {
    const A: &str = "d:/ws/tsconfig.a.json";
    const B: &str = "d:/ws/tsconfig.b.json";
    let snap = snapshot(
        &[
            (
                A,
                r#"{ "compilerOptions": { "strict": false }, "include": ["src/**/*.tsx"] }"#,
            ),
            (
                B,
                r#"{ "compilerOptions": { "strict": true }, "include": ["src/**/*.vue", "src/**/*.tsx"] }"#,
            ),
        ],
        &[],
    );
    let carrier_owner = snap
        .default_configured_owner_for_file(CARRIER)
        .and_then(|id| snap.tsconfig_path(id).cloned());
    assert_eq!(carrier_owner, Some(CanonicalPath::new(B)));

    assert_eq!(
        verdict(&snap, B, &[IDE_TSX]),
        Err(vec![(IDE_TSX.to_string(), OwnedByDifferentProject)])
    );
    assert_eq!(verdict(&snap, A, &[IDE_TSX]), Ok(()));
}

/// Two claimants under a solution config: the declared reference order decides
/// the unit's owner, overriding the name-least fallback. `z` is referenced first,
/// so the unit is admitted to `z` and refused for `a`.
#[test]
fn reference_walk_decides_the_generated_unit_owner() {
    const SOLUTION: &str = "d:/ws/tsconfig.json";
    const A: &str = "d:/ws/tsconfig.a.json";
    const Z: &str = "d:/ws/tsconfig.z.json";
    let snap = snapshot(
        &[
            (
                SOLUTION,
                r#"{ "files": [], "references": [{ "path": "./tsconfig.z.json" }, { "path": "./tsconfig.a.json" }] }"#,
            ),
            (A, r#"{ "include": ["src"] }"#),
            (Z, r#"{ "include": ["src"] }"#),
        ],
        &[],
    );
    assert_eq!(verdict(&snap, Z, &[IDE_TSX, API_TS]), Ok(()));
    assert_eq!(
        verdict(&snap, A, &[IDE_TSX]),
        Err(vec![(IDE_TSX.to_string(), OwnedByDifferentProject)])
    );
}

#[test]
fn fingerprint_is_order_independent_and_tracks_units_and_membership() {
    let fingerprint = |snap: &WorkspaceSnapshot, paths: &[&str]| match decide(snap, TSCONFIG, paths)
    {
        GeneratedUnitAdmission::Admitted(admitted) => admitted.fingerprint(),
        GeneratedUnitAdmission::NotAdmitted(refusal) => panic!("expected admission: {refusal:?}"),
    };
    let snap = snapshot(&[(TSCONFIG, r#"{ "include": ["src"] }"#)], &[]);

    let forward = fingerprint(&snap, &[IDE_TSX, API_TS]);
    assert_eq!(
        forward,
        fingerprint(&snap, &[API_TS, IDE_TSX, API_TS]),
        "input order and duplicates do not change the identity"
    );
    assert_ne!(
        forward,
        fingerprint(&snap, &[IDE_TSX]),
        "a different unit set is a different admission"
    );

    let widened = snapshot(&[(TSCONFIG, r#"{ "include": ["src", "test"] }"#)], &[]);
    assert_ne!(
        forward,
        fingerprint(&widened, &[IDE_TSX, API_TS]),
        "a changed include is a different admission"
    );
}

#[test]
fn admitted_proof_covers_exactly_its_sorted_units() {
    let snap = snapshot(&[(TSCONFIG, r#"{ "include": ["src"] }"#)], &[]);
    let GeneratedUnitAdmission::Admitted(admitted) = decide(&snap, TSCONFIG, &[API_TS, IDE_TSX])
    else {
        panic!("expected admission");
    };
    assert_eq!(admitted.units(), units(&[IDE_TSX, API_TS]).as_slice());
    assert!(admitted.covers(&CanonicalPath::new(IDE_TSX)));
    assert!(!admitted.covers(&CanonicalPath::new(SIDECAR_DTS)));
    assert_eq!(admitted.tsconfig_path(), &CanonicalPath::new(TSCONFIG));
}
