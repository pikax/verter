//! The soak project's source texts, generated from `(file, revision)` alone,
//! so a restored revision republishes exactly the bytes it first published.

use super::manifest::{ConfigVariant, Mode, COLD_ALIASES, LARGE_UNION_MEMBERS};

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
    let tag = format!("t{tranche:03}r{revision}");
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
    for name in ["O0", "O1", "M0", "M1", "X0", "X1", "X2"] {
        alias(name.to_string());
    }
    for f in 0..2 {
        let _ = writeln!(
            out,
            "export function f{f}() {{\n  return {{ ok: \"{tag}f{f}\" }};\n}}"
        );
    }
    if mode == Mode::Pressure {
        out.push_str("export type Big0 =");
        for m in 0..LARGE_UNION_MEMBERS {
            let _ = write!(out, "\n  | \"{tag}b0m{m}\"");
        }
        out.push_str(";\n");
    }
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

/// SHA-256 of every source and configuration text the `(mode, tranches)`
/// workload can publish: the sentinel project, and each tranche module,
/// dependency, churn, surge, isolation and refusal fixture at the first,
/// second and last tranche and at every revision the template publishes.
#[must_use]
pub fn content_digest(mode: Mode, tranches: u32) -> String {
    use super::manifest::{
        sha256_hex, CHURN_EDITS, ISOLATION_REFUSE_PROPS, ISOLATION_SPEND_PROPS, OVERSIZED_PROPS,
        REFUSAL_PROPS, SURGE_PROPS,
    };
    let mut texts: Vec<String> = vec![
        tsconfig(ConfigVariant::Original),
        tsconfig(ConfigVariant::Alternate),
        sentinel_source(),
        isolation_types_source(),
    ];
    texts.extend(
        PICK_FILES
            .iter()
            .flat_map(|(f, t)| [f.to_string(), t.to_string()]),
    );
    let mut tranche_set = vec![0, 1, tranches.saturating_sub(1)];
    tranche_set.dedup();
    for tranche in tranche_set {
        let tag = format!("t{tranche:03}");
        texts.extend((0..=1).map(|rev| module_source(mode, tranche, rev)));
        texts.extend((100..100 + CHURN_EDITS).map(|rev| module_source(mode, tranche, rev)));
        texts.push(dep_source(tranche));
        texts.extend((0..2).map(|k| churn_source(tranche, k)));
        texts.push(surge_source(tranche, 0, SURGE_PROPS));
        texts.push(surge_source(tranche, 1, OVERSIZED_PROPS));
        texts.extend(
            [
                ISOLATION_SPEND_PROPS,
                ISOLATION_REFUSE_PROPS,
                REFUSAL_PROPS,
                REFUSAL_PROPS + 3,
            ]
            .map(|props| isolation_component_source(&tag, props)),
        );
    }
    sha256_hex(texts.join("\u{0}").as_bytes())
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
