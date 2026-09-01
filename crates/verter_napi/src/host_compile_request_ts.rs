//! Renderer for the published TypeScript declaration of the host compile
//! request, and the owner of the structural metadata it renders from.
//!
//! The Rust declarations that DECODE the request are the single authority
//! for its shape: the framework-tagged [`NapiHostCompileRequest`] and
//! product-tagged [`NapiRequestedProduct`] unions in
//! [`crate::host_compile_request`], and the `verter_protocol` DTOs their
//! arms carry. `packages/native/host-compile-request.generated.ts` is a
//! GENERATED, BYTE-PINNED projection of those declarations — nothing in it
//! is written by hand, so a field, variant, optionality or string-union
//! member that changes on the Rust side and is not regenerated fails the
//! freshness guard instead of shipping a declaration the decoder refuses.
//!
//! Two structural sources feed the render, and neither reads Rust source
//! text:
//!
//! - The nested DTOs project through their `ts_rs::TS` derives, and the
//!   SET of them is the request union's transitive `ts_rs` dependency
//!   closure rather than a list kept here. Each carries
//!   `#[ts(rename = "Host…")]` so the published name is declared beside
//!   the schema, and `#[ts(optional_fields = nullable)]` so a serde
//!   `Option<T>` slot projects as `field?: T | null` — omitted,
//!   `undefined` and `null` all decode to `None`.
//! - The two JS-facing tagged unions project through
//!   [`tagged_js_union`], which emits the serde enum AND its arm table
//!   from ONE declaration list. An arm cannot exist without a row, and a
//!   row cannot exist without an arm.
//!
//! `ts-rs` prints an advisory while compiling the two unions, because it
//! does not model serde's enum-level `deny_unknown_fields`. That is
//! accurate: TypeScript has no direct equivalent, and the closedness the
//! decoder enforces surfaces in the projection as excess-property
//! checking against a closed object type.
//!
//! Generation is a COMMAND — `pnpm gen:host-request-ts`, or
//! `cargo run -p verter_napi --features generate-host-request-ts --bin
//! generate_host_compile_request_ts`. The freshness guard renders in
//! memory and byte-compares; it never writes.

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use ts_rs::{Config, TypeVisitor, TS};

use crate::host_compile_request::{NapiHostCompileRequest, NapiRequestedProduct};

/// The committed path of the generated declaration, relative to the
/// workspace root. The generator binary and the freshness guard both
/// resolve it from here.
pub const HOST_COMPILE_REQUEST_TS_PATH: &str = "packages/native/host-compile-request.generated.ts";

// ─── structural metadata for a JS-facing tagged union ────────────────────

/// The shape an arm of a tagged union carries beside its tag.
pub enum ArmShape {
    /// A newtype arm: the tag object intersected with a named payload type.
    Payload(String),
    /// A struct arm's own fields. Empty means the arm is the tag alone.
    Fields(Vec<ArmField>),
}

/// One named field of a struct arm, with its projected TypeScript type.
pub struct ArmField {
    /// The wire key, which is the Rust field identifier. The two are one
    /// value, not two that agree: the macro reads the field through
    /// `stringify!` — the same identifier serde reads — and its grammar
    /// admits no attribute on a variant field, so a rename that could
    /// separate them has nowhere to be written.
    pub name: &'static str,
    /// The projected TypeScript type, resolved through `ts_rs`.
    pub ts_type: String,
}

/// One arm of a tagged union: its published name, its tag value, its
/// shape and its documentation.
pub struct TaggedArm {
    /// The published TypeScript name of this arm.
    pub ts_name: &'static str,
    /// The tag value that selects this arm at decode.
    pub tag_value: &'static str,
    /// Documentation lines, in source order.
    pub docs: &'static [&'static str],
    /// What the arm carries beside its tag.
    pub shape: ArmShape,
}

/// A JS-facing tagged union: the arms plus the key that discriminates them.
pub struct TaggedUnion {
    /// The published TypeScript name of the union.
    pub ts_name: &'static str,
    /// The property every arm carries to discriminate itself.
    pub tag_key: &'static str,
    /// Documentation lines, in source order.
    pub docs: &'static [&'static str],
    /// The arms, in declaration order.
    pub arms: Vec<TaggedArm>,
}

/// Declares a JS-facing tagged union once: the serde enum the boundary
/// decodes AND the structural metadata the TypeScript projection renders
/// from come out of the same list, so an arm cannot exist without a row
/// and a row cannot name an arm that does not exist.
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
        #[derive(serde::Deserialize, Debug, Clone, PartialEq, Eq, ts_rs::TS)]
        #[serde(tag = $tag_key, deny_unknown_fields)]
        #[ts(rename = $union_ts)]
        $(#[doc = $union_doc])*
        $vis enum $Name {
            $(
                $(#[doc = $arm_doc])*
                #[serde(rename = $tag_value)]
                $variant $body,
            )*
        }

        impl $Name {
            /// The structural metadata the TypeScript projection renders
            /// from, in declaration order.
            pub fn ts_union(cfg: &::ts_rs::Config) -> $crate::host_compile_request_ts::TaggedUnion {
                $crate::host_compile_request_ts::TaggedUnion {
                    ts_name: $union_ts,
                    tag_key: $tag_key,
                    docs: &[$($union_doc),*],
                    arms: vec![$(
                        $crate::host_compile_request_ts::TaggedArm {
                            ts_name: $arm_ts,
                            tag_value: $tag_value,
                            docs: &[$($arm_doc),*],
                            shape: tagged_js_union!(@shape cfg, $body),
                        }
                    ),*],
                }
            }
        }
    };

    (@shape $cfg:ident, ( $payload:ty )) => {
        $crate::host_compile_request_ts::ArmShape::Payload(
            <$payload as ::ts_rs::TS>::name($cfg),
        )
    };
    (@shape $cfg:ident, { }) => {
        $crate::host_compile_request_ts::ArmShape::Fields(::std::vec::Vec::new())
    };
    (@shape $cfg:ident, { $($field:ident : $ty:ty),* $(,)? }) => {
        $crate::host_compile_request_ts::ArmShape::Fields(vec![$(
            $crate::host_compile_request_ts::ArmField {
                name: stringify!($field),
                ts_type: <$ty as ::ts_rs::TS>::name($cfg),
            }
        ),*])
    };
}

pub(crate) use tagged_js_union;

// ─── the render ──────────────────────────────────────────────────────────

/// Render the published TypeScript declaration of the host compile
/// request.
///
/// Deterministic: the `ts_rs` configuration is the library default, never
/// read from the environment, so the same tree renders the same bytes on
/// every machine.
#[must_use]
pub fn render_host_compile_request_ts() -> String {
    let cfg = Config::new();
    let mut out = String::from(HEADER);

    for declaration in nested_declarations(&cfg) {
        out.push_str(&declaration);
        out.push('\n');
    }

    render_tagged_union(&mut out, &NapiRequestedProduct::ts_union(&cfg));
    render_tagged_union(&mut out, &NapiHostCompileRequest::ts_union(&cfg));

    // Exactly one trailing newline: the arm renderer separates blocks
    // with a blank line, and the last block ends the file.
    format!("{}\n", out.trim_end())
}

/// Every nested declaration the request reaches, ordered so a type is
/// declared before it is referenced.
///
/// The set is DERIVED, never listed: it is the transitive `ts_rs`
/// dependency closure of the request union. A hand-written list is a
/// second place a declaration has to be added, and the byte pin cannot
/// tell "the list is complete" from "the list is what it was" — a field
/// of a newly reachable DTO reddens the pin once, and a regeneration
/// makes it green again with the published file referencing a name it
/// never declares. Reaching the type is what declares it.
fn nested_declarations(cfg: &Config) -> Vec<String> {
    let mut walk = ClosureWalk {
        cfg,
        // The two unions are rendered by `render_tagged_union` under their
        // arms' published names. They are walked for what they reach and
        // never emitted here.
        tagged: vec![
            TypeId::of::<NapiHostCompileRequest>(),
            TypeId::of::<NapiRequestedProduct>(),
        ],
        visited: HashSet::new(),
        open: Vec::new(),
        nodes: Vec::new(),
    };
    walk.visit::<NapiHostCompileRequest>();
    order_declarations(walk.nodes)
}

/// One declarable type in the request's `ts_rs` closure.
struct ClosureNode {
    /// The published TypeScript name.
    name: String,
    /// The rendered declaration block.
    declaration: String,
    /// The published names this declaration references.
    references: BTreeSet<String>,
}

/// Collects every declarable type the request reaches, with the edges
/// between them.
struct ClosureWalk<'a> {
    cfg: &'a Config,
    /// Types the tagged-union renderer owns: traversed, never emitted.
    tagged: Vec<TypeId>,
    visited: HashSet<TypeId>,
    /// The reference sets of the declarations still being expanded, one
    /// frame per level of the walk.
    open: Vec<BTreeSet<String>>,
    nodes: Vec<ClosureNode>,
}

impl TypeVisitor for ClosureWalk<'_> {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        // A derived declaration has an output path; a primitive, container
        // or wrapper has none and is not declared. Containers still reach
        // their element types: `ts_rs` visits both the field type and its
        // generic arguments, so `Option<T>` and `BTreeMap<K, V>` deliver
        // `T`, `K` and `V` here in their own right.
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
            .expect("the request's type closure is a directed acyclic graph");
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

/// Render a tagged union: each arm under its own published name, then the
/// union that discriminates them.
fn render_tagged_union(out: &mut String, union: &TaggedUnion) {
    for arm in &union.arms {
        out.push_str(&render_docs(arm.docs));
        let tag = format!("{}: \"{}\"", union.tag_key, arm.tag_value);
        match &arm.shape {
            ArmShape::Payload(payload) => {
                let _ = writeln!(
                    out,
                    "export type {} = {{ {tag} }} & {payload};",
                    arm.ts_name
                );
            }
            ArmShape::Fields(fields) if fields.is_empty() => {
                let _ = writeln!(out, "export type {} = {{ {tag} }};", arm.ts_name);
            }
            ArmShape::Fields(fields) => {
                let _ = writeln!(out, "export interface {} {{", arm.ts_name);
                let _ = writeln!(out, "  {tag};");
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
    for (index, arm) in union.arms.iter().enumerate() {
        let terminator = if index + 1 == union.arms.len() {
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

/// The file header. States the decoder's rules, which is what every
/// declaration below projects.
const HEADER: &str = r#"// @generated by verter — DO NOT EDIT BY HAND.
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
