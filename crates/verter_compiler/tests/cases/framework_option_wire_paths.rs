//! The option path a request-construction refusal names is a field of the
//! REQUEST SCHEMA, resolved against the generated declaration a caller
//! writes their request to.
//!
//! `FrameworkOption`'s `Display` is what every transport's refusal message
//! embeds ("unsupported option '<path>'"), so the path it prints is public
//! surface: a caller reads it to find the field they wrote. Two wrong
//! sources for it have to be ruled out, and this file rules out both.
//!
//! The first is the Rust variant spelling. Variants are named
//! `Surface_option`, so a case-lowered `Debug` form reads
//! `vue:transformOptionsHoistStatic` while the request field is
//! `hoistStatic`.
//!
//! The second is the committed option INVENTORY (`VueOption::tsv_row` /
//! `SvelteOption::tsv_row`). That inventory is faithful to what it
//! describes — the OFFICIAL frameworks' option surfaces — but that is a
//! different namespace from Verter's request object, which is FLAT and
//! camelCase. `compiler-core:ParserOptions` + `compatConfig.MODE` is the
//! request's `compatConfigMode`, and `SvelteOptions.customElement.props` +
//! `*.type` is the request's `customElementDescriptor.props.*.propType`.
//! Worse, the inventory records `compatConfig` on TWO surfaces, which the
//! request carries as the two distinct fields `compatConfig` and
//! `transformCompatConfig`: read from the inventory, both refusals tell a
//! caller to remove `compatConfig`, and one of them never wrote it.
//!
//! So the path comes from `VueOption::request_field` /
//! `SvelteOption::request_field`, and the oracle here is
//! `packages/native/host-compile-request.generated.ts` — the declaration a
//! caller's request is typed against, itself byte-pinned to the Rust decode
//! schema by `verter_napi`'s own freshness guard. Reading it means this
//! file cannot restate the schema; it has to agree with it.
//!
//! `tsv_rows_match_the_committed_inventory` stays because the inventory
//! rows are still contract — they are the exhaustiveness proof that every
//! semantics-affecting framework option is classified exactly once. They
//! are simply not what a refusal names.
//!
//! Mutation recipes:
//! - Point `TransformOptionsCompatConfig::request_field` at
//!   `"compatConfig"` (the inventory's own spelling for the row):
//!   `distinct_refusable_options_never_collapse_onto_one_path` reports the
//!   collision with `ParserOptionsCompatConfig`, and
//!   `every_refusable_option_names_its_own_request_field` reports the wrong
//!   path. The schema-walk case does NOT catch it — `compatConfig` IS a
//!   real own key — which is exactly why the explicit table and the
//!   collision check carry the contract and the walk is their companion.
//! - Restore the inventory-derived `Display` (strip the surface prefix,
//!   re-attach any dotted tail):
//!   `every_request_field_resolves_against_the_generated_declaration`
//!   reports `compatConfig.MODE`, `customElement.tag` and
//!   `customElement.props.*.type` as paths the declaration has no key for,
//!   `refusal_paths_are_request_fields_not_inventory_rows` reports zero
//!   divergence where seventeen is required, and the refusable table goes
//!   red on those seventeen rows.
//! - Case-lower `format!("{option:?}")` in `Display`: every rendered path
//!   names a field the declaration does not have, and the walk case goes
//!   red on all of them.
//! - Change one `VueOption::tsv_row` arm's option column (e.g.
//!   `TransformOptionsHoistStatic` to `"hoisted"`):
//!   `tsv_rows_match_the_committed_inventory` reports the invented row and
//!   the unquoted one.
//! - Delete one entry from `PRESENCE_REFUSED_SVELTE_OPTIONS`: the count
//!   assertion in `every_refusable_option_names_its_own_request_field`
//!   fails, so the refusable set cannot silently shrink out from under
//!   this file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use verter_compiler::compile_request::svelte::{
    ALL_SVELTE_OPTIONS, PRESENCE_REFUSED_SVELTE_OPTIONS, VALUE_REFUSED_SVELTE_OPTIONS,
};
use verter_compiler::compile_request::vue::{
    ALL_VUE_OPTIONS, PRESENCE_REFUSED_VUE_OPTIONS, VALUE_REFUSED_VUE_OPTIONS,
};
use verter_compiler::compile_request::{FrameworkOption, SvelteOption, VueOption};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate is <workspace>/crates/verter_compiler")
        .to_path_buf()
}

/// The `(surface, option)` column pairs of one inventory file, in the order
/// the file lists them.
fn inventory_rows(file_name: &str) -> Vec<(String, String)> {
    let path = workspace_root()
        .join("packages/framework-conformance-harness/evidence")
        .join(file_name);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{file_name} must be readable at {path:?}: {e}"));
    raw.lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut columns = line.split('\t');
            let surface = columns
                .next()
                .unwrap_or_else(|| panic!("{file_name}: row without a surface column"));
            let option = columns
                .next()
                .unwrap_or_else(|| panic!("{file_name}: row without an option column"));
            (surface.to_string(), option.to_string())
        })
        .collect()
}

#[test]
fn tsv_rows_match_the_committed_inventory() {
    for (file_name, quoted) in [
        (
            "vue-options.tsv",
            ALL_VUE_OPTIONS
                .iter()
                .map(|option| {
                    let (surface, name) = option.tsv_row();
                    (surface.to_string(), name.to_string())
                })
                .collect::<Vec<_>>(),
        ),
        (
            "svelte-options.tsv",
            ALL_SVELTE_OPTIONS
                .iter()
                .map(|option| {
                    let (surface, name) = option.tsv_row();
                    (surface.to_string(), name.to_string())
                })
                .collect::<Vec<_>>(),
        ),
    ] {
        let committed = inventory_rows(file_name);
        assert_eq!(
            quoted.len(),
            committed.len(),
            "{file_name}: {} rows quoted by variants, {} rows committed",
            quoted.len(),
            committed.len()
        );

        let quoted_set: BTreeSet<_> = quoted.iter().cloned().collect();
        let committed_set: BTreeSet<_> = committed.iter().cloned().collect();
        assert_eq!(
            quoted_set.len(),
            quoted.len(),
            "{file_name}: two variants quote the same inventory row"
        );

        let invented: Vec<_> = quoted_set.difference(&committed_set).collect();
        assert!(
            invented.is_empty(),
            "{file_name}: variants quote rows the inventory does not have: {invented:?}"
        );
        let unquoted: Vec<_> = committed_set.difference(&quoted_set).collect();
        assert!(
            unquoted.is_empty(),
            "{file_name}: inventory rows no variant quotes: {unquoted:?}"
        );
    }
}

// ── the generated request declaration, as an oracle ──────────────────────

/// The generated declaration, reduced to what resolving a field path needs:
/// each `export interface` as `name -> (field -> declared type text)`, and
/// each `export type` alias as `name -> body text`.
///
/// The file is generated, one field per line and one interface per block,
/// so this reads its shape rather than parsing TypeScript. A construct it
/// cannot read surfaces at the walk as an unresolved segment, named, rather
/// than as a silent pass.
struct RequestDeclaration {
    interfaces: BTreeMap<String, BTreeMap<String, String>>,
    aliases: BTreeMap<String, String>,
}

impl RequestDeclaration {
    fn load() -> Self {
        let path = workspace_root().join("packages/native/host-compile-request.generated.ts");
        let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "the generated host compile request declaration must be readable at {path:?}: \
                 {e}. Generate it with `pnpm gen:host-request-ts`."
            )
        });

        let mut interfaces: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut aliases: BTreeMap<String, String> = BTreeMap::new();
        let mut open: Option<(String, BTreeMap<String, String>)> = None;
        let mut pending_alias: Option<(String, String)> = None;

        for line in raw.lines() {
            if let Some((name, fields)) = open.as_mut() {
                if line == "}" {
                    interfaces.insert(name.clone(), std::mem::take(fields));
                    open = None;
                } else if let Some((field, type_text)) = declared_field(line) {
                    fields.insert(field, type_text);
                }
                continue;
            }

            if let Some((name, body)) = pending_alias.as_mut() {
                body.push(' ');
                body.push_str(line.trim());
                if line.trim_end().ends_with(';') {
                    aliases.insert(name.clone(), body.trim().trim_end_matches(';').to_string());
                    pending_alias = None;
                }
                continue;
            }

            if let Some(rest) = line.strip_prefix("export interface ") {
                let name = rest
                    .split_whitespace()
                    .next()
                    .expect("an interface declaration names its type");
                open = Some((name.to_string(), BTreeMap::new()));
                continue;
            }

            if let Some(rest) = line.strip_prefix("export type ") {
                let (name, body) = rest
                    .split_once('=')
                    .unwrap_or_else(|| panic!("a type alias assigns a body: {line}"));
                let name = name.trim().to_string();
                let body = body.trim();
                if line.trim_end().ends_with(';') {
                    aliases.insert(name, body.trim_end_matches(';').to_string());
                } else {
                    pending_alias = Some((name, body.to_string()));
                }
            }
        }

        assert!(
            open.is_none() && pending_alias.is_none(),
            "the generated declaration ended mid-block"
        );
        assert!(
            interfaces.contains_key("HostVueCompileOptions")
                && interfaces.contains_key("HostSvelteCompileOptions"),
            "neither options interface was read out of the generated declaration"
        );
        Self {
            interfaces,
            aliases,
        }
    }

    /// Resolve `path` (dot-separated, `*` for a map index) starting at the
    /// named type, answering the declared type text of the final segment.
    ///
    /// `Err` names the segment that did not resolve, so a failure reads as
    /// "the declaration has no such key" rather than as a parse problem.
    fn resolve(&self, root: &str, path: &str) -> Result<String, String> {
        let mut current = root.to_string();
        for segment in path.split('.') {
            let ty = strip_nullable(&current);
            if segment == "*" {
                current = self
                    .map_value_type(&ty)
                    .ok_or_else(|| format!("`{ty}` is not an index-signature map"))?;
                continue;
            }
            if let Some(fields) = self.interfaces.get(&ty) {
                current = fields
                    .get(segment)
                    .ok_or_else(|| format!("`{ty}` has no own key `{segment}`"))?
                    .clone();
                continue;
            }
            if let Some(body) = self.aliases.get(&ty) {
                // A tagged union arm: `{ "enabled": Payload }`.
                current = union_arm_payload(body, segment).ok_or_else(|| {
                    format!("alias `{ty}` has no arm keyed `{segment}` (body: {body})")
                })?;
                continue;
            }
            return Err(format!("`{ty}` is not a declared interface or alias"));
        }
        Ok(current)
    }

    fn map_value_type(&self, ty: &str) -> Option<String> {
        let body = self
            .aliases
            .get(ty)
            .cloned()
            .unwrap_or_else(|| ty.to_string());
        let start = body.find("]:")? + 2;
        let end = body.rfind('}')?;
        Some(body[start..end].trim().to_string())
    }
}

/// `field: T;` / `field?: T | null;` — a declared member line, and nothing
/// else. Doc-comment lines carry no trailing `;` and blank lines carry no
/// `:`, so neither becomes a phantom own key the walk could resolve
/// through.
fn declared_field(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if !trimmed.ends_with(';') || trimmed.starts_with('*') || trimmed.starts_with('/') {
        return None;
    }
    let (head, type_text) = trimmed.split_once(':')?;
    let head = head.trim();
    let name = head.strip_suffix('?').unwrap_or(head);
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }
    Some((
        name.to_string(),
        type_text.trim().trim_end_matches(';').trim().to_string(),
    ))
}

fn strip_nullable(type_text: &str) -> String {
    type_text
        .split('|')
        .map(str::trim)
        .find(|part| !part.is_empty() && *part != "null" && *part != "undefined")
        .unwrap_or(type_text)
        .to_string()
}

/// The payload of the `{ "key": Payload }` arm of a union alias body.
fn union_arm_payload(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":");
    let start = body.find(&needle)? + needle.len();
    let rest = &body[start..];
    let end = rest.find('}')?;
    Some(rest[..end].trim().to_string())
}

/// Every option a `CompileRequestError` can name, read from the lists the
/// refusal sites themselves consult.
fn refusable_options() -> Vec<FrameworkOption> {
    PRESENCE_REFUSED_VUE_OPTIONS
        .into_iter()
        .chain(VALUE_REFUSED_VUE_OPTIONS)
        .map(FrameworkOption::Vue)
        .chain(
            PRESENCE_REFUSED_SVELTE_OPTIONS
                .into_iter()
                .map(|(option, _)| option)
                .chain(VALUE_REFUSED_SVELTE_OPTIONS)
                .map(FrameworkOption::Svelte),
        )
        .collect()
}

fn request_root(option: FrameworkOption) -> &'static str {
    match option {
        FrameworkOption::Vue(_) => "HostVueCompileOptions",
        FrameworkOption::Svelte(_) => "HostSvelteCompileOptions",
    }
}

fn variant_name(option: FrameworkOption) -> String {
    match option {
        FrameworkOption::Vue(inner) => format!("{inner:?}"),
        FrameworkOption::Svelte(inner) => format!("{inner:?}"),
    }
}

/// The rendered path minus its framework tag, asserting the tag is the
/// option's own.
fn rendered_path(option: FrameworkOption) -> String {
    let path = option.to_string();
    let (framework, tail) = path
        .split_once(':')
        .unwrap_or_else(|| panic!("{path} is not framework-tagged"));
    assert_eq!(framework, option.framework(), "{path}");
    assert!(!tail.is_empty(), "{path} has an empty option path");
    tail.to_string()
}

#[test]
fn every_request_field_resolves_against_the_generated_declaration() {
    let declaration = RequestDeclaration::load();
    let mut resolved = 0usize;

    for option in ALL_VUE_OPTIONS
        .iter()
        .map(|o| FrameworkOption::Vue(*o))
        .chain(
            ALL_SVELTE_OPTIONS
                .iter()
                .map(|o| FrameworkOption::Svelte(*o)),
        )
    {
        let Some(field) = option.request_field() else {
            // No slot: the rendered path is the option's full inventory
            // identity, which no caller can mistake for a request field.
            let (surface, name) = option.tsv_row();
            assert_eq!(
                rendered_path(option),
                format!("{surface}.{name}"),
                "{}",
                variant_name(option)
            );
            continue;
        };
        assert_eq!(rendered_path(option), field, "{}", variant_name(option));
        declaration
            .resolve(request_root(option), field)
            .unwrap_or_else(|why| {
                panic!(
                    "{} names request field `{field}`, which the generated declaration does not \
                     have: {why}",
                    variant_name(option)
                )
            });
        resolved += 1;
    }

    // Non-vacuity: a `request_field` answering `None` everywhere would
    // resolve nothing and pass in silence.
    assert!(
        resolved >= 60,
        "only {resolved} inventory rows map onto a request field; the request carries far more"
    );
}

#[test]
fn every_refusable_option_names_its_own_request_field() {
    // The contract, stated: for each option a refusal can name, the exact
    // path a caller is shown. Written out rather than derived, because
    // "the field the caller wrote" is a claim about the request object,
    // not something the option table can be asked to confirm about itself.
    let expected: BTreeMap<&str, &str> = [
        // Vue — presence-refused, in `into_request` order.
        ("ParserOptionsCompatConfig", "vue:compatConfig"),
        ("ParserOptionsCompatConfigMode", "vue:compatConfigMode"),
        (
            "ParserOptionsCompatConfigCompilerIsOnElement",
            "vue:compatConfigCompilerIsOnElement",
        ),
        (
            "ParserOptionsCompatConfigCompilerVBindSync",
            "vue:compatConfigCompilerVBindSync",
        ),
        (
            "ParserOptionsCompatConfigCompilerVIfVForPrecedence",
            "vue:compatConfigCompilerVIfVForPrecedence",
        ),
        (
            "ParserOptionsCompatConfigCompilerVBindObjectOrder",
            "vue:compatConfigCompilerVBindObjectOrder",
        ),
        (
            "ParserOptionsCompatConfigCompilerVOnNative",
            "vue:compatConfigCompilerVOnNative",
        ),
        (
            "ParserOptionsCompatConfigCompilerNativeTemplate",
            "vue:compatConfigCompilerNativeTemplate",
        ),
        (
            "ParserOptionsCompatConfigCompilerInlineTemplate",
            "vue:compatConfigCompilerInlineTemplate",
        ),
        (
            "ParserOptionsCompatConfigCompilerFilters",
            "vue:compatConfigCompilerFilters",
        ),
        // The inventory records this row on a second surface under the
        // same `compatConfig` name; the request carries it as its own
        // field, and the refusal has to say so.
        ("TransformOptionsCompatConfig", "vue:transformCompatConfig"),
        ("CodegenOptionsMode", "vue:codegenMode"),
        // Vue — value-refused.
        ("ParserOptionsDelimiters", "vue:delimiters"),
        // Svelte — presence-refused, in `into_request` order.
        ("ParseLoose", "svelte:loose"),
        ("CompileOptionsAccessors", "svelte:accessors"),
        ("CompileOptionsImmutable", "svelte:immutable"),
        (
            "CompileOptionsCompatibilityComponentApi",
            "svelte:compatibilityComponentApi",
        ),
        ("CompileOptionsHmr", "svelte:hmr"),
        ("CustomElementExtend", "svelte:customElementExtend"),
        ("ModuleGenerate", "svelte:generateModule"),
        ("ModuleExperimentalAsync", "svelte:experimentalAsync"),
        // Svelte — value-refused.
        (
            "CustomElementPropsType",
            "svelte:customElementDescriptor.props.*.propType",
        ),
        ("CustomElementTag", "svelte:customElementDescriptor.tag"),
        ("CompileOptionsNamespace", "svelte:namespace"),
        ("CompileOptionsFragments", "svelte:fragments"),
        ("CompileOptionsCss", "svelte:css"),
    ]
    .into_iter()
    .collect();

    let refusable = refusable_options();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for option in &refusable {
        let variant = variant_name(*option);
        assert!(
            seen.insert(variant.clone()),
            "{variant} appears twice in the refusable set"
        );
        let want = expected.get(variant.as_str()).unwrap_or_else(|| {
            panic!("{variant} can be refused but no caller-facing path is stated for it")
        });
        assert_eq!(&option.to_string(), want, "{variant}");
    }
    assert_eq!(
        refusable.len(),
        expected.len(),
        "the refusable set has {} options, {} are stated here",
        refusable.len(),
        expected.len()
    );
}

#[test]
fn distinct_refusable_options_never_collapse_onto_one_path() {
    // Keyed on the option VARIANT, not on its inventory option NAME. Two
    // rows sharing a name across surfaces is exactly the case that hides a
    // collision — `ParserOptions.compatConfig` and
    // `TransformOptions.compatConfig` are one name and two request fields
    // — so a name-keyed check would find nothing to report.
    let mut by_path: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for option in refusable_options() {
        by_path
            .entry(option.to_string())
            .or_default()
            .insert(variant_name(option));
    }

    let collapsed: Vec<_> = by_path
        .iter()
        .filter(|(_, variants)| variants.len() > 1)
        .collect();
    assert!(
        collapsed.is_empty(),
        "distinct refusable options share one refusal path: {collapsed:?}"
    );
    assert_eq!(
        by_path.len(),
        refusable_options().len(),
        "every refusable option must name a distinct request field"
    );
}

/// The inventory-derived path this rendering must NOT be: the row's
/// `option` column, prefixed by whatever option-path segments its
/// `surface` column carries beyond the surface type itself.
///
/// A local copy of the algorithm being ruled out, so the ruling-out is a
/// comparison against it rather than a description of it.
fn inventory_derived_path(option: FrameworkOption) -> String {
    let (surface, name) = option.tsv_row();
    let local_surface = surface.rsplit_once(':').map_or(surface, |(_, tail)| tail);
    match local_surface.split_once('.') {
        Some((_, nested)) => format!("{nested}.{name}"),
        None => name.to_string(),
    }
}

/// The two names a refusal must never leak: the Rust variant spelling, and
/// the official framework's own option surface.
///
/// The surface half is checked by comparison against the inventory-derived
/// algorithm itself, not by banning segment spellings: `props` is both an
/// inventory surface segment AND a genuine own key of the Svelte
/// custom-element descriptor, so a spelling ban would reject the correct
/// path. The count is exact — a rendering that agreed with the inventory
/// everywhere would make the table above satisfiable from the wrong
/// source, so this states how far the two genuinely diverge.
#[test]
fn refusal_paths_are_request_fields_not_inventory_rows() {
    let mut diverging = 0usize;
    for option in refusable_options() {
        let tail = rendered_path(option);
        assert!(
            !tail.chars().next().is_some_and(char::is_uppercase),
            "{tail} leads with an upper-case segment, which is a leaked Rust variant spelling"
        );
        if tail != inventory_derived_path(option) {
            diverging += 1;
        }
    }
    assert_eq!(
        diverging,
        17,
        "17 of the {} refusable options name a request field the inventory spells differently",
        refusable_options().len()
    );
}

/// Two inventory rows folding onto one request field is CORRECT, and must
/// not be "repaired" by inventing per-surface names: `hoistStatic` is
/// inventoried on two Vue surfaces and is one field, and a Svelte
/// descriptor member keeps its nesting rather than being flattened.
#[test]
fn two_inventory_rows_may_share_one_request_field() {
    assert_eq!(
        FrameworkOption::Vue(VueOption::TransformOptionsHoistStatic).to_string(),
        "vue:hoistStatic"
    );
    assert_eq!(
        FrameworkOption::Vue(VueOption::CompileScriptHoistStatic).to_string(),
        "vue:hoistStatic"
    );
    assert_eq!(
        FrameworkOption::Svelte(SvelteOption::CustomElementShadow).to_string(),
        "svelte:customElementDescriptor.shadow"
    );
}
