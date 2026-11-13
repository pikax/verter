//! Renderer for the published TypeScript declarations of the host compile
//! request and its response, and the macros that declare the JS-facing
//! unions it renders from.
//!
//! The Rust declarations the bindings DECODE and SERIALISE are the single
//! authority for these shapes. Three generated, byte-pinned files project
//! them, and nothing in any of them is written by hand:
//!
//! - `packages/native/host-compile-request.generated.ts` — the request, from
//!   the framework-tagged [`NapiHostCompileRequest`] and product-tagged
//!   [`NapiRequestedProduct`] unions in [`crate::host_compile_request`] and
//!   the `verter_protocol` DTOs their arms carry;
//! - `packages/native/host-compile-response.generated.ts` — the native
//!   response, from the unions and objects the native binding serialises in
//!   [`crate::compile_request_response`];
//! - `packages/wasm/src/compile-response.generated.ts` — the browser
//!   response, from the `Browser*` declarations in
//!   `verter_protocol::compile_response` that the browser binding
//!   serialises.
//!
//! A field, arm, optionality or string-union member that changes on the
//! Rust side and is not regenerated fails the freshness guard instead of
//! shipping a declaration the binding does not keep.
//!
//! Two structural sources feed each render, and neither reads Rust source
//! text:
//!
//! - Plain objects project through their `ts_rs::TS` derives, and the SET
//!   of them is the root's transitive `ts_rs` dependency closure rather
//!   than a list kept here. Each carries `#[ts(rename = "Host…")]` so the
//!   published name is declared beside the schema.
//! - Unions project through the metadata their declaring macro emits from
//!   ONE declaration list — [`tagged_js_union`] for the decoded request,
//!   [`js_object_union`] for the native response, and
//!   `verter_protocol::serialized_js_union` for the browser response and the
//!   missing-input rows. An arm cannot exist without a row, and a row cannot
//!   exist without an arm. Arms that share a published name and a shape
//!   publish as one declaration whose tag is the union of their tags.
//!
//! A name a render references but does not declare is a shared host type
//! written elsewhere (`HostDiagnosticsSnapshot`, `HostIdeResponse`, …): the
//! render imports it from the module its file names, so the import list is
//! derived from what the declarations reach, never kept beside them.
//!
//! Generation is a COMMAND — `pnpm gen:host-request-ts`, or `cargo run -p
//! verter_napi --features generate-host-request-ts --bin
//! generate_host_compile_request_ts` — which writes all three files. The
//! freshness guard renders in memory and byte-compares; it never writes.

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use ts_rs::{Config, TypeVisitor, TS};
use verter_protocol::compile_response::{
    BrowserCompileRequestResponse, BrowserCompiledProduct, FfiMissingInput,
};

pub use verter_protocol::js_union::{ArmField, ArmShape, TaggedArm, TaggedUnion};

use crate::compile_request_response::{
    NapiCompileRequestFailure, NapiCompileRequestsEntry, NapiCompiledProduct,
};
use crate::host_compile_request::{NapiHostCompileRequest, NapiRequestedProduct};

/// The committed path of the generated request declaration, relative to the
/// workspace root.
pub const HOST_COMPILE_REQUEST_TS_PATH: &str = "packages/native/host-compile-request.generated.ts";

/// The committed path of the generated native response declaration.
pub const HOST_COMPILE_RESPONSE_TS_PATH: &str =
    "packages/native/host-compile-response.generated.ts";

/// The committed path of the generated browser response declaration.
pub const BROWSER_COMPILE_RESPONSE_TS_PATH: &str =
    "packages/wasm/src/compile-response.generated.ts";

/// Every generated declaration file: its committed path, relative to the
/// workspace root, and its render. The generator binary writes exactly
/// these, and the freshness guard pins exactly these.
#[must_use]
pub fn generated_declarations() -> Vec<(&'static str, String)> {
    vec![
        (
            HOST_COMPILE_REQUEST_TS_PATH,
            render_host_compile_request_ts(),
        ),
        (
            HOST_COMPILE_RESPONSE_TS_PATH,
            render_host_compile_response_ts(),
        ),
        (
            BROWSER_COMPILE_RESPONSE_TS_PATH,
            render_browser_compile_response_ts(),
        ),
    ]
}

// ─── the union declaration macros ────────────────────────────────────────

/// Declares a JS-facing tagged union the request DECODES once: the serde
/// enum the boundary decodes AND the structural metadata the TypeScript
/// projection renders from come out of the same list, so an arm cannot
/// exist without a row and a row cannot name an arm that does not exist.
///
/// Three arm forms are supported, matching what the decoder needs: a
/// newtype arm over a payload DTO, a struct arm with named fields, and a
/// tag-only arm (declared `{}` rather than as a unit variant so serde's
/// `deny_unknown_fields` still applies to it).
///
/// The declared tag literal is serde's tag: it is emitted as a per-variant
/// `rename`, and the container carries no `rename_all` to derive a competing
/// spelling from the variant identifier, so the value the projection
/// publishes and the value the decoder answers to are the same token rather
/// than two spellings a probe has to catch drifting apart.
///
/// TypeScript has no direct equivalent of `deny_unknown_fields`: the
/// closedness the decoder enforces surfaces in the projection as
/// excess-property checking against a closed object type.
macro_rules! tagged_js_union {
    (
        $(#[doc = $union_doc:literal])*
        $vis:vis enum $Name:ident tagged $tag_key:literal as $union_ts:literal {
            $(
                $(#[doc = $arm_doc:literal])*
                $variant:ident $body:tt => $tag_value:literal as $arm_ts:literal
            ),* $(,)?
        }
    ) => {
        #[derive(serde::Deserialize, Debug, Clone, PartialEq, Eq)]
        #[serde(tag = $tag_key, deny_unknown_fields)]
        $(#[doc = $union_doc])*
        $vis enum $Name {
            $(
                $(#[doc = $arm_doc])*
                #[serde(rename = $tag_value)]
                $variant $body,
            )*
        }

        ::verter_protocol::js_union_ts! {
            $Name [$tag_key] as $union_ts docs [$($union_doc),*] {
                $( $body => [$tag_value] as $arm_ts docs [$($arm_doc),*] ),*
            }
        }
    };
}

pub(crate) use tagged_js_union;

/// Declares a JS-facing object union the native binding SERIALISES: the
/// enum, its conversion to a JavaScript object and the structural metadata
/// the TypeScript projection renders from all come out of one declaration
/// list.
///
/// Every arm is a struct arm whose fields are native values in their own
/// right. The conversion writes the tag first (for a tagged union) and then
/// every field in declaration order, each through its own native
/// conversion, so the object a caller receives is exactly the object a
/// `#[napi(object)]` struct with those fields would produce — key order
/// included. An untagged union's arms are told apart by which slot is
/// `null`, and its arms say so with a `JsNull` field.
///
/// A field's wire key is its identifier, so a camelCase key is written as
/// one.
macro_rules! js_object_union {
    (@emit
        [$($union_doc:literal),*] $vis:vis $Name:ident [$($tag_key:literal)?] as $union_ts:literal {
            $(
                [$($arm_doc:literal),*] $variant:ident { $($field:ident : $ty:ty),* }
                [$($arm_key:literal => $tag_value:literal)?] as $arm_ts:literal;
            )*
        }
    ) => {
        $(#[doc = $union_doc])*
        $vis enum $Name {
            $(
                $(#[doc = $arm_doc])*
                $variant { $($field: $ty),* },
            )*
        }

        impl $Name {
            /// Writes this value's properties onto `object`: the tag first,
            /// then every field in declaration order.
            pub(crate) fn write_properties(
                self,
                object: &mut ::napi::bindgen_prelude::Object<'_>,
            ) -> ::napi::Result<()> {
                use ::napi::bindgen_prelude::JsObjectValue as _;
                match self {
                    $(
                        Self::$variant { $($field),* } => {
                            $( object.set_named_property($arm_key, $tag_value)?; )?
                            $( object.set_named_property(::core::stringify!($field), $field)?; )*
                        }
                    )*
                }
                Ok(())
            }

            /// The tag this value is published under, or `None` for an arm
            /// of an untagged union.
            pub fn tag(&self) -> ::std::option::Option<&'static str> {
                match self {
                    $( Self::$variant { .. } => ::verter_protocol::__js_union_opt!($($tag_value)?), )*
                }
            }
        }

        impl ::napi::bindgen_prelude::ToNapiValue for $Name {
            unsafe fn to_napi_value(
                env: ::napi::bindgen_prelude::sys::napi_env,
                val: Self,
            ) -> ::napi::Result<::napi::bindgen_prelude::sys::napi_value> {
                let env = ::napi::Env::from_raw(env);
                let mut object = ::napi::bindgen_prelude::Object::new(&env)?;
                val.write_properties(&mut object)?;
                Ok(::napi::bindgen_prelude::JsValue::raw(&object))
            }
        }

        ::verter_protocol::js_union_ts! {
            $Name [$($tag_key)?] as $union_ts docs [$($union_doc),*] {
                $( { $($field : $ty),* } => [$($tag_value)?] as $arm_ts docs [$($arm_doc),*] ),*
            }
        }
    };
    (
        $(#[doc = $union_doc:literal])*
        $vis:vis enum $Name:ident tagged $tag_key:literal as $union_ts:literal {
            $(
                $(#[doc = $arm_doc:literal])*
                $variant:ident { $($field:ident : $ty:ty),* $(,)? }
                    => $tag_value:literal as $arm_ts:literal
            ),* $(,)?
        }
    ) => {
        $crate::host_compile_request_ts::js_object_union! { @emit
            [$($union_doc),*] $vis $Name [$tag_key] as $union_ts {
                $(
                    [$($arm_doc),*] $variant { $($field : $ty),* }
                    [$tag_key => $tag_value] as $arm_ts;
                )*
            }
        }
    };
    (
        $(#[doc = $union_doc:literal])*
        $vis:vis enum $Name:ident untagged as $union_ts:literal {
            $(
                $(#[doc = $arm_doc:literal])*
                $variant:ident { $($field:ident : $ty:ty),* $(,)? } as $arm_ts:literal
            ),* $(,)?
        }
    ) => {
        $crate::host_compile_request_ts::js_object_union! { @emit
            [$($union_doc),*] $vis $Name [] as $union_ts {
                $(
                    [$($arm_doc),*] $variant { $($field : $ty),* } [] as $arm_ts;
                )*
            }
        }
    };
}

pub(crate) use js_object_union;

// ─── the renders ─────────────────────────────────────────────────────────

/// Render the published TypeScript declaration of the host compile
/// request.
///
/// Deterministic: the `ts_rs` configuration is the library default, never
/// read from the environment, so the same tree renders the same bytes on
/// every machine.
#[must_use]
pub fn render_host_compile_request_ts() -> String {
    let cfg = Config::new();
    render_module(
        &cfg,
        REQUEST_HEADER,
        None,
        vec![
            union_of::<NapiRequestedProduct>(NapiRequestedProduct::ts_union(&cfg)),
            union_of::<NapiHostCompileRequest>(NapiHostCompileRequest::ts_union(&cfg)),
        ],
        |walk| walk.visit::<NapiHostCompileRequest>(),
    )
}

/// Render the published TypeScript declaration of the native binding's
/// compile response: what `compileRequest` returns and throws, and what
/// each `compileRequests` entry carries.
#[must_use]
pub fn render_host_compile_response_ts() -> String {
    let cfg = Config::new();
    render_module(
        &cfg,
        NATIVE_RESPONSE_HEADER,
        Some("./host-types"),
        vec![
            union_of::<FfiMissingInput>(FfiMissingInput::ts_union(&cfg)),
            union_of::<NapiCompiledProduct>(NapiCompiledProduct::ts_union(&cfg)),
            union_of::<NapiCompileRequestFailure>(NapiCompileRequestFailure::ts_union(&cfg)),
            union_of::<NapiCompileRequestsEntry>(NapiCompileRequestsEntry::ts_union(&cfg)),
        ],
        |walk| walk.visit::<NapiCompileRequestsEntry>(),
    )
}

/// Render the published TypeScript declaration of the browser binding's
/// compile response.
#[must_use]
pub fn render_browser_compile_response_ts() -> String {
    let cfg = Config::new();
    render_module(
        &cfg,
        BROWSER_RESPONSE_HEADER,
        Some("@verter/native/host-types"),
        vec![union_of::<BrowserCompiledProduct>(
            BrowserCompiledProduct::ts_union(&cfg),
        )],
        |walk| walk.visit::<BrowserCompileRequestResponse>(),
    )
}

/// A union the render publishes from its metadata, keyed by the type the
/// closure walk meets it as.
struct RenderedUnion {
    id: TypeId,
    union: TaggedUnion,
}

fn union_of<U: 'static>(union: TaggedUnion) -> RenderedUnion {
    RenderedUnion {
        id: TypeId::of::<U>(),
        union,
    }
}

/// Render one generated module: its header, the imports of the shared
/// names it references, every plain declaration the root reaches, then
/// each union in the order given.
fn render_module(
    cfg: &Config,
    header: &str,
    imports_from: Option<&str>,
    unions: Vec<RenderedUnion>,
    visit_root: impl FnOnce(&mut ClosureWalk<'_>),
) -> String {
    let mut walk = ClosureWalk {
        cfg,
        // The unions are rendered by `render_tagged_union` under their
        // arms' published names. They are walked for what they reach and
        // never emitted as plain declarations.
        tagged: unions.iter().map(|rendered| rendered.id).collect(),
        visited: HashSet::new(),
        open: Vec::new(),
        nodes: Vec::new(),
        referenced: BTreeSet::new(),
    };
    visit_root(&mut walk);

    let declared: BTreeSet<&str> = walk
        .nodes
        .iter()
        .map(|node| node.name.as_str())
        .chain(unions.iter().map(|rendered| rendered.union.ts_name))
        .collect();
    let imported: Vec<&str> = walk
        .referenced
        .iter()
        .map(String::as_str)
        .filter(|name| !declared.contains(name))
        .collect();

    let mut out = String::from(header);
    if !imported.is_empty() {
        let module = imports_from.unwrap_or_else(|| {
            panic!(
                "the render references {imported:?}, which it neither declares nor has a \
                 module to import from"
            )
        });
        out.push_str("import type {\n");
        for name in &imported {
            let _ = writeln!(out, "  {name},");
        }
        let _ = writeln!(out, "}} from \"{module}\";\n");
    }

    for declaration in order_declarations(walk.nodes) {
        out.push_str(&declaration);
        out.push('\n');
    }
    for rendered in &unions {
        render_tagged_union(&mut out, &rendered.union);
    }

    // Exactly one trailing newline: the arm renderer separates blocks
    // with a blank line, and the last block ends the file.
    format!("{}\n", out.trim_end())
}

/// One declarable type in a render's `ts_rs` closure.
struct ClosureNode {
    /// The published TypeScript name.
    name: String,
    /// The rendered declaration block.
    declaration: String,
    /// The published names this declaration references.
    references: BTreeSet<String>,
}

/// Collects every declarable type a root reaches, with the edges between
/// them, and every published name the reached types are known by.
///
/// The set of declarations is DERIVED, never listed: it is the transitive
/// `ts_rs` dependency closure of the root. A hand-written list is a second
/// place a declaration has to be added, and the byte pin cannot tell "the
/// list is complete" from "the list is what it was" — a field of a newly
/// reachable DTO reddens the pin once, and a regeneration makes it green
/// again with the published file referencing a name it never declares.
/// Reaching the type is what declares it.
struct ClosureWalk<'a> {
    cfg: &'a Config,
    /// Types the union renderer owns: traversed, never emitted.
    tagged: Vec<TypeId>,
    visited: HashSet<TypeId>,
    /// The reference sets of the declarations still being expanded, one
    /// frame per level of the walk.
    open: Vec<BTreeSet<String>>,
    nodes: Vec<ClosureNode>,
    /// Every type name the reached types are published under.
    referenced: BTreeSet<String>,
}

impl TypeVisitor for ClosureWalk<'_> {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if let Some(name) = published_type_name(&T::name(self.cfg)) {
            self.referenced.insert(name.to_string());
        }
        // A derived declaration has an output path; a primitive, container,
        // wrapper or shared type written elsewhere has none and is not
        // declared. Containers still reach their element types: `ts_rs`
        // visits both the field type and its generic arguments, so
        // `Option<T>` and `BTreeMap<K, V>` deliver `T`, `K` and `V` here in
        // their own right.
        if T::output_path().is_none() {
            return;
        }
        let id = TypeId::of::<T>();
        let tagged = self.tagged.contains(&id);
        if !tagged {
            if let Some(open) = self.open.last_mut() {
                open.insert(T::ident(self.cfg));
            }
        }
        if !self.visited.insert(id) {
            return;
        }

        self.open.push(BTreeSet::new());
        T::visit_dependencies(self);
        T::visit_generics(self);
        let references = self.open.pop().expect("the frame pushed just above");

        if !tagged {
            self.nodes.push(ClosureNode {
                name: T::ident(self.cfg),
                declaration: declaration::<T>(self.cfg),
                references,
            });
        }
    }
}

/// The type a projected name refers to, when it names one: a leading
/// identifier that starts with a capital and is followed by nothing or by
/// an indexed access (`HostRequestedProduct["kind"]`).
///
/// A primitive (`string`, `null`), a generic application (`Array<…>`,
/// `Record<…>`) and an inline object or union name no type of their own.
fn published_type_name(name: &str) -> Option<&str> {
    let end = name
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'))
        .unwrap_or(name.len());
    let (head, rest) = name.split_at(end);
    let names_a_type = head.starts_with(|ch: char| ch.is_ascii_uppercase())
        && (rest.is_empty() || rest.starts_with('['));
    names_a_type.then_some(head)
}

/// Order the closure so a declaration follows everything it references,
/// breaking ties by published name so the render does not inherit the
/// walk's order — which `ts_rs` derives from a hash set and does not
/// promise to repeat across compilations.
fn order_declarations(nodes: Vec<ClosureNode>) -> Vec<String> {
    let mut pending: BTreeMap<String, ClosureNode> = nodes
        .into_iter()
        .map(|node| (node.name.clone(), node))
        .collect();
    let mut out = Vec::with_capacity(pending.len());

    while !pending.is_empty() {
        let ready = pending
            .values()
            .find(|node| {
                node.references
                    .iter()
                    .all(|reference| reference == &node.name || !pending.contains_key(reference))
            })
            .map(|node| node.name.clone())
            .expect("a render's type closure is a directed acyclic graph");
        let node = pending
            .remove(&ready)
            .expect("just selected from `pending`");
        out.push(node.declaration);
    }
    out
}

/// Project one declaration through its `ts_rs` derive.
fn declaration<T: TS + ?Sized>(cfg: &Config) -> String {
    let decl = T::decl(cfg);
    let (name, body) = split_declaration(&decl);
    let mut out = String::new();
    if let Some(docs) = T::docs() {
        out.push_str(docs.trim_start_matches('\n'));
    }
    if is_object_literal(body) {
        let inner = &body[1..body.len() - 1];
        let _ = writeln!(out, "export interface {name} {}", format_object_body(inner));
    } else {
        let _ = writeln!(out, "export type {name} ={};", format_union(body));
    }
    out
}

/// Split `type NAME = BODY;` into its name and body.
///
/// The three assumptions below are `ts_rs`'s own declaration grammar,
/// which it emits verbatim for every type. They are asserted rather than
/// recovered from: if a `ts_rs` upgrade changes that grammar, generation
/// must stop at the upgrade with the shape it did not recognise, not
/// publish a mangled declaration that then byte-pins itself as correct.
fn split_declaration(decl: &str) -> (&str, &str) {
    let rest = decl
        .strip_prefix("type ")
        .expect("a ts-rs declaration starts with `type `");
    let (name, body) = rest
        .split_once(" = ")
        .expect("a ts-rs declaration separates name and body with ` = `");
    (
        name,
        body.trim()
            .strip_suffix(';')
            .expect("a ts-rs declaration ends with `;`")
            .trim(),
    )
}

/// Whether `body` is a single object literal spanning the whole body.
fn is_object_literal(body: &str) -> bool {
    body.starts_with('{') && split_top_level(body, '|').len() == 1 && body.ends_with('}')
}

/// Format an object literal's interior as an indented member list.
fn format_object_body(inner: &str) -> String {
    let mut out = String::from("{\n");
    for member in split_top_level(inner, ',') {
        let (docs, declaration) = split_leading_docs(&member);
        for line in docs.lines() {
            let _ = writeln!(out, "  {line}");
        }
        let _ = writeln!(out, "  {declaration};");
    }
    out.push('}');
    out
}

/// Format a union body onto the assignment that precedes it: inline while
/// it stays narrow, one arm per line otherwise.
fn format_union(body: &str) -> String {
    let arms = split_top_level(body, '|');
    if arms.len() < 2 || arms.iter().map(|arm| arm.len() + 3).sum::<usize>() <= MAX_INLINE_UNION {
        return format!(" {}", arms.join(" | "));
    }
    let mut out = String::new();
    for arm in arms {
        let _ = write!(out, "\n  | {arm}");
    }
    out
}

/// The width above which a union is broken onto one line per arm.
const MAX_INLINE_UNION: usize = 60;

/// Split a member into its leading JSDoc block, if any, and the rest.
fn split_leading_docs(member: &str) -> (&str, &str) {
    let Some(rest) = member.strip_prefix("/**") else {
        return ("", member);
    };
    let Some(end) = rest.find("*/") else {
        return ("", member);
    };
    let split = "/**".len() + end + "*/".len();
    (member[..split].trim_end(), member[split..].trim())
}

/// Split `src` on `separator` at nesting depth zero, ignoring separators
/// inside strings, block comments and any bracket pair.
///
/// `<` and `>` count as a pair so a separator inside `Array<A | B>` or
/// `Record<string, never>` stays nested. `=>` is therefore read as one
/// token: a projected function type would otherwise close a bracket that
/// was never opened and take every separator after it with it.
fn split_top_level(src: &str, separator: char) -> Vec<String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut in_comment = false;
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        index += 1;
        if in_comment {
            current.push(ch);
            if ch == '*' && chars.get(index) == Some(&'/') {
                current.push('/');
                index += 1;
                in_comment = false;
            }
            continue;
        }
        if in_string {
            current.push(ch);
            if ch == '\\' {
                if let Some(&escaped) = chars.get(index) {
                    current.push(escaped);
                    index += 1;
                }
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '/' if chars.get(index) == Some(&'*') => {
                current.push_str("/*");
                index += 1;
                in_comment = true;
            }
            '"' => {
                in_string = true;
                current.push(ch);
            }
            '>' if current.ends_with('=') => current.push(ch),
            '{' | '[' | '(' | '<' => {
                depth += 1;
                current.push(ch);
            }
            '}' | ']' | ')' | '>' => {
                depth -= 1;
                current.push(ch);
            }
            _ if ch == separator && depth == 0 => {
                push_trimmed(&mut out, &current);
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    push_trimmed(&mut out, &current);
    out
}

fn push_trimmed(out: &mut Vec<String>, current: &str) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
}

/// The arms of a union that publish as one declaration: the first arm
/// under that published name, and every tag value the name answers to.
struct PublishedArm<'a> {
    arm: &'a TaggedArm,
    tags: Vec<&'static str>,
}

/// Group a union's arms by published name, in first-appearance order.
///
/// Arms that share a name must share a shape, and only the first may carry
/// documentation unless the rest repeat it: one declaration cannot publish
/// two shapes or two descriptions.
fn published_arms(union: &TaggedUnion) -> Vec<PublishedArm<'_>> {
    let mut published: Vec<PublishedArm<'_>> = Vec::new();
    for arm in &union.arms {
        match published
            .iter_mut()
            .find(|group| group.arm.ts_name == arm.ts_name)
        {
            Some(group) => {
                assert!(
                    group.arm.shape == arm.shape,
                    "the arms published as `{}` in `{}` carry different shapes",
                    arm.ts_name,
                    union.ts_name,
                );
                assert!(
                    arm.docs.is_empty() || arm.docs == group.arm.docs,
                    "the arms published as `{}` in `{}` carry different documentation",
                    arm.ts_name,
                    union.ts_name,
                );
                group.tags.extend(arm.tag_value);
            }
            None => published.push(PublishedArm {
                arm,
                tags: arm.tag_value.into_iter().collect(),
            }),
        }
    }
    published
}

/// Render a union: each published arm under its own name, then the union
/// that discriminates them.
fn render_tagged_union(out: &mut String, union: &TaggedUnion) {
    let published = published_arms(union);
    for PublishedArm { arm, tags } in &published {
        out.push_str(&render_docs(arm.docs));
        let tag = union.tag_key.map(|key| {
            let values: Vec<String> = tags.iter().map(|tag| format!("\"{tag}\"")).collect();
            format!("{key}: {}", values.join(" | "))
        });
        match (&arm.shape, &tag) {
            (ArmShape::Payload(payload), Some(tag)) => {
                let _ = writeln!(
                    out,
                    "export type {} = {{ {tag} }} & {payload};",
                    arm.ts_name
                );
            }
            (ArmShape::Payload(payload), None) => {
                let _ = writeln!(out, "export type {} = {payload};", arm.ts_name);
            }
            (ArmShape::Fields(fields), Some(tag)) if fields.is_empty() => {
                let _ = writeln!(out, "export type {} = {{ {tag} }};", arm.ts_name);
            }
            (ArmShape::Fields(fields), tag) => {
                let _ = writeln!(out, "export interface {} {{", arm.ts_name);
                if let Some(tag) = tag {
                    let _ = writeln!(out, "  {tag};");
                }
                for field in fields {
                    let _ = writeln!(out, "  {}: {};", field.name, field.ts_type);
                }
                out.push_str("}\n");
            }
        }
        out.push('\n');
    }

    out.push_str(&render_docs(union.docs));
    let _ = writeln!(out, "export type {} =", union.ts_name);
    for (index, PublishedArm { arm, .. }) in published.iter().enumerate() {
        let terminator = if index + 1 == published.len() {
            ";"
        } else {
            ""
        };
        let _ = writeln!(out, "  | {}{terminator}", arm.ts_name);
    }
    out.push('\n');
}

/// Render Rust documentation lines as a JSDoc block.
fn render_docs(docs: &[&str]) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let mut out = String::from("/**\n");
    for line in docs {
        let line = line.trim_end();
        if line.is_empty() {
            out.push_str(" *\n");
        } else {
            let _ = writeln!(out, " *{line}");
        }
    }
    out.push_str(" */\n");
    out
}

/// The request file's header. States the decoder's rules, which is what
/// every declaration below projects.
const REQUEST_HEADER: &str = r#"// @generated by verter — DO NOT EDIT BY HAND.
//
// The tag-discriminated host compile request the native adapter decodes,
// projected from the Rust declarations that decode it. Regenerate with
// `pnpm gen:host-request-ts`; a hand edit, or a Rust change without a
// regen, fails the freshness guard.
//
// Every rule below is the decoder's, not a convention:
//
// - Every object is closed. A key outside its declared set is refused at
//   decode, including a key that belongs to the other framework's arm or
//   to another product.
// - Every non-optional field is required. An absent key is a refusal, not
//   a substituted value.
// - An optional field may be omitted, or set to `undefined` or `null`;
//   all three read as absent, and what absent MEANS is decided by the
//   compiler, not here.
// - Every string union is closed. A spelling outside it is refused at
//   decode. A slot typed `string` is forwarded verbatim: the wire owns no
//   vocabulary over it, and whether one exists elsewhere is that slot's
//   own business, not this shape's.
//
// Nothing in this shape decides whether a compile is legal: the product
// set, the backend/product pairing and option support are the compiler's
// own rules, reported as its own refusals after the request is accepted.

"#;

/// The native response file's header.
const NATIVE_RESPONSE_HEADER: &str = r#"// @generated by verter — DO NOT EDIT BY HAND.
//
// The typed host compile response the native adapter returns and throws,
// projected from the Rust declarations it serialises. Regenerate with
// `pnpm gen:host-request-ts`; a hand edit, or a Rust change without a
// regen, fails the freshness guard.
//
// - A product row and a failure are discriminated by `kind`; a batch entry
//   by which of `response` and `failure` is `null`.
// - An optional field is absent, never `null`, when it has no value.
// - `compileRequest` throws a failure as an `Error` whose `message` is the
//   failure's message and which carries every other field of its arm as an
//   own property.

"#;

/// The browser response file's header.
const BROWSER_RESPONSE_HEADER: &str = r#"// @generated by verter — DO NOT EDIT BY HAND.
//
// The typed host compile response the browser adapter returns, projected
// from the Rust declarations it serialises. Regenerate with
// `pnpm gen:host-request-ts`; a hand edit, or a Rust change without a
// regen, fails the freshness guard.
//
// - A product row is discriminated by `kind`.
// - An optional field is absent (`undefined`), never `null`, when it has no
//   value.
// - A refusal is thrown as a string; there is no partial response.

"#;
