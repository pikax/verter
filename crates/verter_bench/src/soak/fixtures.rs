//! The soak project's source texts, generated from `(file, revision)` alone,
//! so a restored revision republishes exactly the bytes it first published.

use super::manifest::{
    big_file, canonical, churn_file, dep_file, surge_file, Action, ConfigVariant, LifecycleClass,
    Mode, COLD_ALIASES, LARGE_UNION_MEMBERS,
};
use super::run::publication_digest;

/// The persistent configuration sentinel module: its one alias reads a type
/// through the `@pick/*` path mapping the configuration selects.
pub const SENTINEL_FILE: &str = "cfg/use.ts";
/// The sentinel alias.
pub const SENTINEL_ALIAS: &str = "Sentinel";
/// The two path-mapping targets the sentinel reads through.
pub const PICK_FILES: [(&str, &str); 2] = [
    (
        "cfg/a/pick.ts",
        "export type Picked = { readonly via: \"a\" };\n",
    ),
    (
        "cfg/b/pick.ts",
        "export type Picked = { readonly via: \"b\" };\n",
    ),
];

/// The sentinel module's text.
#[must_use]
pub fn sentinel_source() -> String {
    "import type { Picked } from \"@pick/pick\";\nexport type Sentinel = Picked;\n".to_string()
}

/// The project's `tsconfig.json` for `variant`.
#[must_use]
pub fn tsconfig(variant: ConfigVariant) -> String {
    let target = match variant {
        ConfigVariant::Original => "cfg/a/*",
        ConfigVariant::Alternate => "cfg/b/*",
    };
    format!(
        "{{\"compilerOptions\":{{\"strict\":true,\"baseUrl\":\".\",\"paths\":{{\"@pick/*\":[\"{target}\"]}}}},\"include\":[\"**/*.ts\"]}}\n"
    )
}

/// A tranche module's text at `revision` (0 the original, 1 the edit, 100
/// and up the candidate-churn versions).
#[must_use]
pub fn module_source(mode: Mode, tranche: u32, revision: u32) -> String {
    use std::fmt::Write;
    let tag = format!("{}t{tranche:03}r{revision}", mode.as_str());
    let mut out = String::from("import type { Dep } from \"./dep\";\n");
    let mut alias = |name: String| {
        let _ = writeln!(
            out,
            "export type {name} = {{ readonly key: \"{tag}{name}\"; readonly dep: Dep }};"
        );
    };
    for i in 0..COLD_ALIASES {
        alias(format!("A{i}"));
    }
    for name in ["O0", "O1", "M0", "M1", "X0", "X1", "X2", "X3", "X4"] {
        alias(name.to_string());
    }
    for f in 0..2 {
        let _ = writeln!(
            out,
            "export function f{f}() {{\n  return {{ ok: \"{tag}f{f}\" }};\n}}"
        );
    }
    out
}

/// A pressure tranche's large-union module.
#[must_use]
pub fn big_source(tranche: u32) -> String {
    use std::fmt::Write;
    let mut out = String::from("export type Big0 =");
    for m in 0..LARGE_UNION_MEMBERS {
        let _ = write!(out, "\n  | \"t{tranche:03}b0m{m}\"");
    }
    out.push_str(";\n");
    out
}

/// The module every tranche module imports.
#[must_use]
pub fn dep_source(tranche: u32) -> String {
    format!("export type Dep = {{ readonly tranche: \"t{tranche:03}\" }};\n")
}

/// A uniquely owned churn module.
#[must_use]
pub fn churn_source(tranche: u32, ordinal: u32) -> String {
    format!("export type C = {{ readonly churn: \"t{tranche:03}c{ordinal}\" }};\n")
}

/// A surge component with `props` string props. `ordinal` makes each
/// component's source distinct.
#[must_use]
pub fn surge_source(tranche: u32, ordinal: u32, props: u32) -> String {
    use std::fmt::Write;
    let mut out =
        format!("<script setup lang=\"ts\">\n// t{tranche:03}s{ordinal}\ndefineProps<{{\n");
    for p in 0..props {
        let _ = writeln!(out, "  p{p}: string");
    }
    out.push_str("}>()\n</script>\n");
    out
}

/// A file of the tranche's isolation project.
#[must_use]
pub fn isolation_file(tranche: u32, name: &str) -> String {
    format!("/w/t{tranche:03}/{name}")
}

/// The isolation type module's text.
#[must_use]
pub fn isolation_types_source() -> String {
    "export type Leaf = { z: string };\nexport type Mid = { m: Leaf; n: Pick<Leaf, \"z\"> };\n"
        .to_string()
}

/// An isolation component whose `props` mapped-type props each spend
/// projection operations when its metadata is computed.
#[must_use]
pub fn isolation_component_source(tag: &str, props: u32) -> String {
    use std::fmt::Write;
    let mut decls = String::new();
    let mut members = String::new();
    for p in 0..props {
        let _ = writeln!(decls, "type M{p} = {{ [K in keyof Mid]: Mid[K] }}");
        let _ = writeln!(members, "  p{p}: M{p}");
    }
    format!(
        "<script setup lang=\"ts\">\n// {tag}\nimport type {{ Mid }} from \"./ref\"\n{decls}defineProps<{{\n{members}}}>()\n</script>\n"
    )
}

/// The publications the runner makes before the first action: the original
/// configuration, the two path-mapping targets and the sentinel, each as the
/// digest of its path and bytes ([`publication_digest`]).
#[must_use]
pub fn setup_publications() -> Vec<String> {
    let mut out = vec![publication_digest(
        &canonical("tsconfig.json"),
        &tsconfig(ConfigVariant::Original),
    )];
    out.extend(
        PICK_FILES
            .iter()
            .map(|(file, text)| publication_digest(&canonical(file), text)),
    );
    out.push(publication_digest(
        &canonical(SENTINEL_FILE),
        &sentinel_source(),
    ));
    out
}

/// Every publication `action` of a `mode` workload makes, in order, as the
/// digest of the path and the exact bytes. The runner records what it
/// actually published and the verifier compares the two, so a fixture this
/// enumeration omits is a failed run, not an unhashed input.
#[must_use]
pub fn expected_publications(mode: Mode, action: &Action) -> Vec<String> {
    use super::manifest::Target;
    let tranche = action.tranche;
    let at = |file: &str, text: &str| publication_digest(&canonical(file), text);
    let source_of = |file: &str, revision: u32| {
        if file == dep_file(tranche) {
            dep_source(tranche)
        } else if file == big_file(tranche) {
            big_source(tranche)
        } else {
            module_source(mode, tranche, revision)
        }
    };
    match &action.target {
        Target::File { file, revision }
            if matches!(
                action.class,
                LifecycleClass::Open | LifecycleClass::Edit | LifecycleClass::RevertSource
            ) =>
        {
            vec![at(file, &source_of(file, *revision))]
        }
        Target::Release { file, revision, .. } => vec![at(file, &source_of(file, *revision))],
        Target::Config { variant } => vec![at("tsconfig.json", &tsconfig(*variant))],
        Target::Query { file, .. } if action.class == LifecycleClass::UniqueOwnerChurn => {
            let ordinal = (0..)
                .find(|k| churn_file(tranche, *k) == *file)
                .expect("a churn module of the tranche");
            vec![at(file, &churn_source(tranche, ordinal))]
        }
        Target::Churn { file, edits, .. } => (0..*edits)
            .map(|edit| 100 + edit)
            .chain([0])
            .map(|revision| at(file, &module_source(mode, tranche, revision)))
            .collect(),
        Target::Surge {
            modules,
            props,
            oversized_props,
        } => (0..*modules)
            .map(|ordinal| {
                let count = if ordinal == 0 {
                    *oversized_props
                } else {
                    *props
                };
                at(
                    &surge_file(tranche, ordinal),
                    &surge_source(tranche, ordinal, count),
                )
            })
            .collect(),
        Target::Isolation {
            spend_props,
            refuse_props,
            ..
        } => {
            let tag = format!("t{tranche:03}");
            let one_host = [
                ("ref.ts", isolation_types_source()),
                ("spend.vue", isolation_component_source(&tag, *spend_props)),
                (
                    "refuse.vue",
                    isolation_component_source(&tag, *refuse_props),
                ),
            ];
            // The subject host and its two cold oracle hosts.
            (0..3)
                .flat_map(|_| one_host.iter())
                .map(|(name, text)| publication_digest(&isolation_file(tranche, name), text))
                .collect()
        }
        Target::Refusal { variant, props, .. } => {
            super::refusal::planned_publications(*variant, tranche, *props)
                .into_iter()
                .map(|(name, text)| publication_digest(&isolation_file(tranche, name), &text))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// A component whose own `<script setup>` nests `levels` `Partial` aliases
/// over `{ x: string }` and declares its props over the outermost: past the
/// fixed connected query-depth bound its metadata is the depth refusal. With
/// `extra`, it also imports the `Extra` type from `./extra` and intersects it
/// into its props.
#[must_use]
pub fn deep_chain_source(tag: &str, levels: u32, extra: bool) -> String {
    use std::fmt::Write;
    let mut script = format!("// {tag}\n");
    if extra {
        script.push_str("import type { Extra } from \"./extra\";\n");
    }
    script.push_str("type P0 = { x: string };\n");
    for level in 1..=levels {
        let _ = writeln!(script, "type P{level} = Partial<P{}>;", level - 1);
    }
    let props = if extra {
        format!("P{levels} & Extra")
    } else {
        format!("P{levels}")
    };
    format!("<script setup lang=\"ts\">\n{script}defineProps<{props}>()\n</script>\n")
}

/// A shallow component beside the deep one: its exact answer must never
/// inherit the deep component's refusal.
#[must_use]
pub fn shallow_sibling_source(tag: &str) -> String {
    format!("<script setup lang=\"ts\">\n// {tag}\ndefineProps<{{ y: number }}>()\n</script>\n")
}

/// The input a component imports and a missing-input action supplies late.
#[must_use]
pub fn extra_source(tag: &str) -> String {
    format!("// {tag}\nexport type Extra = {{ z?: number }};\n")
}

/// A component whose props are only the imported `Extra`.
#[must_use]
pub fn extra_only_source(tag: &str) -> String {
    format!(
        "<script setup lang=\"ts\">\n// {tag}\nimport type {{ Extra }} from \"./extra\";\ndefineProps<Extra>()\n</script>\n"
    )
}
