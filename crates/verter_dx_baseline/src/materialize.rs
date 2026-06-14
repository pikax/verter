//! Host-driven baseline materialization.
//!
//! Loads every materialized `.vue` source under a workspace root into a fully
//! initialized [`verter_session::VerterHost`] through PUBLIC session APIs only
//! (`new` → `upsert` → `ensure_loaded` → `ensure_compiled` → `get_ide` /
//! `get_public_api`) and emits the provider-facing artifacts the differential
//! baseline compares against:
//!
//! - IDE artifact `Foo.vue.tsx` / `Foo.vue.jsx` (the artifact-parity target),
//! - public-API twin `Foo.vue.ts` (so a sibling `import './Foo.vue'` rewritten
//!   to `'./Foo.vue.ts'` resolves for the TS provider),
//! - the `@verter/types` standalone `.d.ts` (reused verbatim from the Rust
//!   constant — never hand-written here),
//! - copied vendored dependency shims, and a synthesized provider-equivalent
//!   `tsconfig.json` when the fixture lacks one.
//!
//! Over-materializing every `.vue` under the root is the hermetic strategy that
//! covers the transitive import closure (an imported child and a
//! barrel-reexported child both get a `.vue.ts` twin) without a separate closure
//! walker.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oxc_sourcemap::{SourceMap, Token};

use verter_session::{
    CompileProfile, CompileTarget, FileKind, HostConfig, UpsertRequest, VerterHost,
    VERTER_TYPES_STANDALONE_DTS,
};
use verter_span::path::canonicalize_path;
use verter_workspace::{FilesystemOptions, FilesystemWorkspace, ProjectGraph, WorkspaceAccess};

/// Provider-equivalent fallback `tsconfig.json`, matching the runtime provider
/// options (tsserver inferred-project options + the TSGO test project defaults).
const FALLBACK_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "module": "ESNext",
    "target": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "preserve",
    "jsxImportSource": "vue",
    "allowImportingTsExtensions": true,
    "allowJs": true,
    "checkJs": true,
    "strict": true,
    "allowArbitraryExtensions": true,
    "lib": ["ESNext", "DOM", "DOM.Iterable"],
    "skipLibCheck": true
  }
}
"#;

/// `package.json` for the injected `@verter/types` shim.
const VERTER_TYPES_PACKAGE_JSON: &str =
    r#"{ "name": "@verter/types", "version": "0.0.0", "types": "index.d.ts" }"#;

/// Inputs to [`materialize`].
#[derive(Debug, Clone)]
pub struct MaterializeRequest {
    /// Absolute workspace root (already holds the stripped `.vue` sources).
    pub workspace_root: PathBuf,
    /// Entry `.vue` files. Used to guarantee at least these are materialized;
    /// the over-materialization pass also covers every `.vue` under the root.
    pub entries: Vec<PathBuf>,
    /// Optional vendored `node_modules` directory copied into
    /// `<root>/node_modules` (committed shims — never a runtime install).
    pub vendor_node_modules: Option<PathBuf>,
    /// The materialized workspace's resolved Vue line. When set, every copied
    /// `vue/package.json` and `@vue/*/package.json` version must EXACTLY equal it,
    /// so the differential cannot silently run against Vue declarations different
    /// from the ones the provider resolves. `None` skips the check (local dev).
    pub expected_vue_version: Option<String>,
    /// Whether a vendored-Vue version mismatch is a hard error (strict CI) or a
    /// recorded structured warning (non-strict). Mirrors the strict tsserver/tsgo
    /// tool-root pinning: strict refuses drift, non-strict records it.
    pub strict_vue_version: bool,
}

/// One vendored-Vue declaration version mismatch: a copied `vue`/`@vue/*` package
/// whose exact version differs from the resolved/expected Vue line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueVersionMismatch {
    /// Package whose version drifted (e.g. `vue`, `@vue/compiler-core`).
    pub package: String,
    /// The resolved/expected Vue line (`expected_vue_version`).
    pub expected: String,
    /// The version found in the copied `package.json` (`<absent>` when the file
    /// is missing or carries no parseable `version`).
    pub found: String,
}

/// One emitted artifact.
#[derive(Debug, Clone)]
pub struct MaterializedArtifact {
    /// Canonical authored `.vue` id this artifact derives from.
    pub source_vue: String,
    /// Generated artifact path on disk.
    pub generated_path: PathBuf,
    /// Rewritten artifact content (`.vue` import specifiers → `.vue.ts`). Kept
    /// in-memory for in-process consumers (tests, the parity gate); the CLI DTO
    /// surfaces only the on-disk path, so the bin profile sees this as unread.
    #[allow(dead_code)]
    pub content: String,
    /// The artifact's V3 source map, ALREADY shifted to match the rewritten
    /// `content` (a `.vue`→`.vue.ts` rewrite that lengthens a line shifts every
    /// later generated position on that line). Position-resolving consumers must
    /// use this, never the host's pre-rewrite map. `None` when the host produced
    /// no map. Surfaced verbatim across the materialize CLI/DTO boundary
    /// (`ArtifactDto.sourceMap`) so the TS runner projects against the same map.
    pub source_map: Option<String>,
    pub source_map_present: bool,
}

/// Result of a materialization pass.
#[derive(Debug, Clone, Default)]
pub struct MaterializeReport {
    /// `.vue.tsx` / `.vue.jsx` IDE artifacts.
    pub ide_artifacts: Vec<MaterializedArtifact>,
    /// `.vue.ts` public-API twins.
    pub public_api_twins: Vec<MaterializedArtifact>,
    /// Path of the injected `@verter/types/index.d.ts`.
    pub verter_types_dts: Option<PathBuf>,
    /// Canonical `.vue` ids whose IDE source map was absent (recorded, not a
    /// crash — surfaced as `compiled_code_map_absent`).
    pub map_absent: Vec<String>,
    /// Canonical `.vue` id → stable `sourceMapIdentity`.
    pub source_map_identities: BTreeMap<String, String>,
    /// `(canonical, message)` for any `.vue` that failed `ensure_compiled`.
    pub compile_errors: Vec<(String, String)>,
    /// Resolved `tsconfig.json` path.
    pub tsconfig_path: Option<PathBuf>,
    /// Whether the tsconfig was synthesized (no fixture tsconfig found).
    pub synthesized_tsconfig: bool,
    /// TS support files (barrels, helpers) whose `.vue` specifiers were rewritten
    /// to `.vue.ts` so the provider resolves cross-file Vue imports/reexports
    /// through the public-API twins.
    pub support_rewrites: Vec<PathBuf>,
    /// Vendored-Vue declaration version mismatches recorded in NON-strict mode
    /// (strict mode hard-fails instead). Empty when versions matched or no
    /// `expected_vue_version` was supplied.
    pub vue_version_warnings: Vec<VueVersionMismatch>,
}

/// Materialization failures.
#[derive(Debug, thiserror::Error)]
pub enum MaterializeError {
    #[error("workspace root is not a directory: {0}")]
    BadRoot(String),
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("host upsert failed for {canonical}: {message}")]
    Upsert { canonical: String, message: String },
    #[error(
        "vendored Vue declaration version mismatch for {package}: expected {expected}, found {found}"
    )]
    VueVersionMismatch {
        package: String,
        expected: String,
        found: String,
    },
}

fn io_err(path: impl AsRef<Path>, source: io::Error) -> MaterializeError {
    MaterializeError::Io {
        path: path.as_ref().display().to_string(),
        source,
    }
}

/// The fixed IDE compile profile — TSX + template data + source maps, matching
/// the LSP document registry's profile.
fn ide_profile() -> CompileProfile {
    CompileProfile {
        source_map: true,
        target: CompileTarget::IDE | CompileTarget::TEMPLATE_DATA,
        ..CompileProfile::default()
    }
}

/// A stable descriptor of the compile profile, folded into `sourceMapIdentity`.
fn profile_descriptor(profile: &CompileProfile) -> String {
    format!(
        "target={:?};source_map={}",
        profile.target, profile.source_map
    )
}

/// FNV-1a 64-bit, hex — a small, release-stable content hash (no extra dep).
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// `sourceMapIdentity` = stable hash over the emitted map content and the
/// compile profile.
pub fn compute_source_map_identity(profile_desc: &str, map: &str) -> String {
    let mut buf = Vec::with_capacity(profile_desc.len() + 1 + map.len());
    buf.extend_from_slice(profile_desc.as_bytes());
    buf.push(0);
    buf.extend_from_slice(map.as_bytes());
    fnv1a_hex(&buf)
}

/// Outcome of classifying an IDE artifact's optional source map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapOutcome {
    /// A map was present — its stable identity.
    Identity(String),
    /// `$/getCompiledCode`-style map-absent: recorded, never a crash.
    Absent,
}

/// Classify an IDE artifact's optional source map (the map-`Option`-`None`
/// handling point).
pub fn classify_source_map(profile_desc: &str, map: Option<&str>) -> MapOutcome {
    match map {
        Some(m) => MapOutcome::Identity(compute_source_map_identity(profile_desc, m)),
        None => MapOutcome::Absent,
    }
}

/// The bytes appended to a rewritten `.vue` specifier (`.vue` → `.vue.ts`). Its
/// UTF-16 length equals its byte length (all ASCII), so the same constant is the
/// generated-column shift applied to a V3 source map.
const VUE_TWIN_SUFFIX: &str = ".ts";

/// Result of [`rewrite_vue_imports_tracked`]: the rewritten text plus the byte
/// offsets, in the ORIGINAL text, where [`VUE_TWIN_SUFFIX`] was inserted
/// (immediately after each rewritten `.vue`). Ascending. Used to keep a recorded
/// source map consistent with the rewritten generated code.
struct VueImportRewrite {
    output: String,
    insertions: Vec<usize>,
}

/// A lexed token, carrying only what specifier-position detection needs:
/// `Word` (an identifier, keyword, or numeric literal), single-char punctuators,
/// and `'`/`"` string literals (interior value + the byte offset of the closing
/// quote). Whitespace and comments are dropped; a template literal and a regex
/// literal are each consumed whole and reduced to one opaque placeholder
/// punctuator (`` ` `` / `/`) so their interiors — including any quotes — can
/// never be mistaken for code or for an import specifier.
enum Tok<'a> {
    Word(&'a str),
    Punct(char),
    Str { value: &'a str, close: usize },
}

#[inline]
fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

#[inline]
fn is_ident_part(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// Keywords after which a `/` begins a regular-expression literal (an expression
/// follows them) rather than a division operator. A value-ending word — any
/// identifier, `this`/`super`/`true`/`false`/`null`, or a numeric literal — is NOT
/// in this set, so a `/` after it lexes as division.
fn is_regex_prefix_keyword(w: &str) -> bool {
    matches!(
        w,
        "return"
            | "throw"
            | "case"
            | "do"
            | "else"
            | "in"
            | "of"
            | "instanceof"
            | "new"
            | "typeof"
            | "void"
            | "delete"
            | "yield"
            | "await"
    )
}

/// Whether a `/` at the current position begins a regex literal rather than a
/// division operator, decided from the previous significant token. A `/` is
/// division only after a token that ENDS a value: an identifier or numeric literal
/// ([`Tok::Word`] that is not an expression-introducing keyword), a string
/// literal, or a `)`/`]` closing a call/group/index/array. After everything else —
/// an operator, `,`, `;`, `{`, `}`, `(`, `[`, `:`, `<`, `>`, an expression keyword,
/// or the start of input — an expression may follow, so a `/` begins a regex.
///
/// The two mis-classifications are NOT symmetric. Reading a division `/` AS a regex
/// (e.g. a `}` that closed an object literal, or a `<` that was a comparison) only
/// ever consumes MORE input as a regex body, which removes string tokens — at worst
/// a missed rewrite, never an invented one (safe), so those ambiguous leads err
/// toward regex deliberately. The reverse — reading a real regex `/` as division —
/// re-lexes the regex body as ordinary tokens, so a `.vue` string inside it can be
/// WRONGLY rewritten. The division branches are `prev ∈ {Str, ')', ']'}`; of these,
/// `Str` and `]` end a value in practice (`arr[i] / n`), but `)` does not always:
/// the one practically reachable residual is a regex literal led by a control-flow
/// `)` (`if (cond) /…\.vue…/`, `for (…) /…/`, `while (…) /…/`), where the `)` ends a
/// control-flow head rather than a value, yet this function reads it as a value end
/// and returns division. That residual is judged acceptable for this test-infra
/// rewriter: it fails loudly (a malformed twin that does not compile) rather than
/// silently, and does not occur in the materialized SFC corpus. It is NOT "fixed" by
/// treating `)` as a regex lead — that would mis-rewrite the overwhelmingly common
/// `(a + b) / c` division case.
fn slash_starts_regex(prev: Option<&Tok>) -> bool {
    match prev {
        None => true,
        Some(Tok::Word(w)) => is_regex_prefix_keyword(w),
        Some(Tok::Str { .. }) => false,
        Some(Tok::Punct(p)) => !matches!(p, ')' | ']'),
    }
}

/// Lex `content` into the minimal token stream specifier detection needs.
///
/// `'`/`"` string literals, line comments (`// …`), block comments (`/* … */`),
/// template literals (with `${ … }` brace tracking), and regex literals (with
/// `[...]` char-class and escape tracking) are all recognized, so a `.vue`
/// appearing inside any of them is never mistaken for a module specifier. A `/` is
/// classified as a regex or a division by the previous significant token (see
/// [`slash_starts_regex`]); ambiguous leads err toward regex, which can only miss
/// a rewrite, never invent one. The quote, slash, backtick, and brace anchors are
/// all ASCII and cannot occur inside a multi-byte UTF-8 sequence, so byte scanning
/// stays correct over non-ASCII content.
fn lex_tokens(content: &str) -> Vec<Tok<'_>> {
    let b = content.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut toks = Vec::new();
    while i < n {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'/' && i + 1 < n && b[i + 1] == b'/' {
            i += 2;
            while i < n && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && i + 1 < n && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < n && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(n);
        } else if c == b'/' && slash_starts_regex(toks.last()) {
            // Regex literal: consume to the closing unescaped `/` that is not inside
            // a `[...]` character class, then its flag chars. Interior bytes —
            // INCLUDING quotes — never become string tokens, so a `.vue` quote run
            // inside a regex is never mistaken for an import specifier. A newline
            // ends an unterminated regex (JS forbids a literal newline in a regex
            // body), bounding the scan to one line so a mis-detected `/` can never
            // swallow a later import.
            i += 1;
            let mut in_class = false;
            while i < n {
                match b[i] {
                    b'\\' => {
                        i += 2;
                        continue;
                    }
                    b'\n' => break,
                    b'[' => in_class = true,
                    b']' => in_class = false,
                    b'/' if !in_class => {
                        i += 1;
                        break;
                    }
                    _ => {}
                }
                i += 1;
            }
            // Trailing regex flags (`d g i m s u v y`).
            while i < n && matches!(b[i], b'd' | b'g' | b'i' | b'm' | b's' | b'u' | b'v' | b'y') {
                i += 1;
            }
            toks.push(Tok::Punct('/'));
        } else if c == b'\'' || c == b'"' {
            let vs = i + 1;
            i += 1;
            while i < n {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if b[i] == c {
                    break;
                }
                i += 1;
            }
            // `close` is the closing-quote byte (or EOF for an unterminated
            // literal); both `vs` and `close` land on ASCII boundaries.
            let close = i.min(n);
            toks.push(Tok::Str {
                value: &content[vs..close],
                close,
            });
            i = close + 1;
        } else if c == b'`' {
            // Template literal: consume to the matching backtick, tracking `${ }`
            // brace depth so interior expression code does not leak out.
            i += 1;
            let mut depth = 0usize;
            while i < n {
                let d = b[i];
                if d == b'\\' {
                    i += 2;
                    continue;
                }
                if depth == 0 {
                    if d == b'`' {
                        i += 1;
                        break;
                    }
                    if d == b'$' && i + 1 < n && b[i + 1] == b'{' {
                        depth += 1;
                        i += 2;
                        continue;
                    }
                    i += 1;
                } else if d == b'{' {
                    depth += 1;
                    i += 1;
                } else if d == b'}' {
                    depth -= 1;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            toks.push(Tok::Punct('`'));
        } else if c.is_ascii_digit() {
            // Numeric literal — consumed as one value token so a following `/` lexes
            // as division, not a spurious regex. Digits, `.`, `_` separators, radix
            // prefixes, exponent letters, and the bigint `n` suffix are all value
            // chars; a lone `e+`/`e-` exponent sign merely splits the literal, which
            // is harmless (the tail is still a value token → division).
            let s = i;
            i += 1;
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
            toks.push(Tok::Word(&content[s..i]));
        } else if is_ident_start(c) {
            let s = i;
            i += 1;
            while i < n && is_ident_part(b[i]) {
                i += 1;
            }
            toks.push(Tok::Word(&content[s..i]));
        } else {
            // Any other byte is an opaque punctuator. Only `(` and `.` are ever
            // inspected; a multi-byte lead byte yields a non-`(`/`.` placeholder,
            // harmless to adjacency analysis.
            toks.push(Tok::Punct(char::from(c)));
            i += 1;
        }
    }
    toks
}

/// Whether the string token at `idx` sits in module-specifier position — the
/// specifier of `… from "…"`, a bare side-effect `import "…"`, or a dynamic
/// `import("…")` / `require("…")` call (rejecting member calls such as
/// `obj.require("…")`). This is the syntactic anchor that distinguishes a real
/// import/reexport specifier from an ordinary string literal.
fn is_specifier_position(toks: &[Tok], idx: usize) -> bool {
    if idx == 0 {
        return false;
    }
    match &toks[idx - 1] {
        // `import X from "…"`, `export … from "…"`, `export * from "…"`, or a bare
        // side-effect `import "…"`.
        Tok::Word(w) => *w == "from" || *w == "import",
        // `import("…")` / `require("…")` — the token before `(` is the call name,
        // and it must not be a member access (`a.require(…)` / `a.import(…)`).
        Tok::Punct('(') => {
            if idx < 2 {
                return false;
            }
            let is_call =
                matches!(&toks[idx - 2], Tok::Word(w) if *w == "import" || *w == "require");
            let is_member = idx >= 3 && matches!(&toks[idx - 3], Tok::Punct('.'));
            is_call && !is_member
        }
        _ => false,
    }
}

/// Whether `value` (a string-literal interior) is a concrete `.vue` specifier: it
/// ends in `.vue` and is not a wildcard glob (`*.vue`). An already-rewritten
/// `.vue.ts` ends in `.ts`, so it is excluded automatically (idempotent).
fn is_concrete_vue_specifier(value: &str) -> bool {
    value.ends_with(".vue") && !value.ends_with("*.vue")
}

/// Rewrite concrete `.vue` import/reexport specifiers to `.vue.ts`, tracking each
/// insertion point in the ORIGINAL text.
///
/// Specifier-aware: a `.vue` token is rewritten ONLY when it is the module
/// specifier of an `import`/`export … from`, a bare side-effect `import`, or a
/// dynamic `import(…)` / `require(…)` — never an ordinary string literal, a
/// comment, a `declare module '*.vue'` glob (its `module` anchor is not an import
/// keyword), or a `'./theme.vuetify'` non-`.vue` path. Idempotent: an
/// already-rewritten `.vue.ts` specifier ends in `.ts`, so it is left untouched.
fn rewrite_vue_imports_tracked(content: &str) -> VueImportRewrite {
    let toks = lex_tokens(content);
    let mut insertions = Vec::new();
    for (idx, t) in toks.iter().enumerate() {
        if let Tok::Str { value, close } = t {
            if is_concrete_vue_specifier(value) && is_specifier_position(&toks, idx) {
                insertions.push(*close);
            }
        }
    }
    insertions.sort_unstable();
    insertions.dedup();

    // Splice VUE_TWIN_SUFFIX in at each (ascending) insertion offset — the byte
    // just after each rewritten `.vue`, immediately before its closing quote.
    let mut out = String::with_capacity(content.len() + insertions.len() * VUE_TWIN_SUFFIX.len());
    let mut last = 0usize;
    for &ins in &insertions {
        out.push_str(&content[last..ins]);
        out.push_str(VUE_TWIN_SUFFIX);
        last = ins;
    }
    out.push_str(&content[last..]);

    VueImportRewrite {
        output: out,
        insertions,
    }
}

/// Rewrite concrete `.vue` import/reexport specifiers to `.vue.ts` so the TS
/// provider resolves cross-file Vue imports through the public-API twin (mirrors
/// TSGO behavior). A deliberate idempotent specifier transform, not type
/// resolution. See [`rewrite_vue_imports_tracked`] for the rules.
pub fn rewrite_vue_imports(content: &str) -> String {
    rewrite_vue_imports_tracked(content).output
}

/// Byte offset in `text` → (0-based line, 0-based UTF-16 column).
///
/// V3 source maps address generated positions by line and UTF-16 column, so an
/// insertion's byte offset must be reduced to that coordinate before a column
/// shift can be applied. `byte_offset` must fall on a char boundary (the rewrite
/// inserts only after the ASCII `.vue`, so it always does).
fn byte_offset_to_line_utf16col(text: &str, byte_offset: usize) -> (u32, u32) {
    let off = byte_offset.min(text.len());
    let mut line: u32 = 0;
    let mut line_start = 0usize;
    for (i, &b) in text.as_bytes().iter().enumerate() {
        if i >= off {
            break;
        }
        if b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let col = text[line_start..off].encode_utf16().count() as u32;
    (line, col)
}

/// Recompute a V3 source map so its generated positions match the text AFTER the
/// `.vue`→`.vue.ts` specifier rewrites.
///
/// Each insertion adds `VUE_TWIN_SUFFIX` (3 UTF-16 columns) on its generated
/// line. Every map token at or after the insertion column on that SAME line
/// shifts right by that amount (cumulatively across multiple insertions on the
/// line); tokens before it, and tokens on other lines, are unchanged — the
/// insertion adds no newline. A post-rewrite generated position therefore maps
/// back to the original source position the un-rewritten code mapped to.
///
/// Best-effort: returns the input unchanged when there are no insertions or when
/// the map cannot be parsed — a malformed map is never dropped.
fn shift_source_map_for_insertions(
    map_json: &str,
    original_code: &str,
    insertions: &[usize],
) -> String {
    if insertions.is_empty() {
        return map_json.to_string();
    }
    let map = match SourceMap::from_json_string(map_json) {
        Ok(m) => m,
        Err(_) => return map_json.to_string(),
    };
    // Insertion points as (generated line, generated UTF-16 column) in the
    // ORIGINAL generated text, against which each token's original column is
    // compared.
    let points: Vec<(u32, u32)> = insertions
        .iter()
        .map(|&off| byte_offset_to_line_utf16col(original_code, off))
        .collect();
    let suffix_cols = VUE_TWIN_SUFFIX.encode_utf16().count() as u32;

    let mut parts = map.into_parts();
    let shifted: Vec<Token> = parts
        .tokens
        .iter()
        .map(|t| {
            let line = t.get_dst_line();
            let col = t.get_dst_col();
            let inserts_before = points
                .iter()
                .filter(|(l, c)| *l == line && col >= *c)
                .count() as u32;
            Token::new(
                line,
                col + inserts_before * suffix_cols,
                t.get_src_line(),
                t.get_src_col(),
                t.get_source_id(),
                t.get_name_id(),
            )
        })
        .collect();
    parts.tokens = shifted.into_boxed_slice();
    // The cached per-chunk delta baselines no longer match the shifted columns;
    // drop them so the encoder re-derives every delta from the modified tokens.
    parts.token_chunks = None;
    SourceMap::from_parts(parts).to_json_string()
}

/// Append `suffix` to a file's full name (`Foo.vue` + `.tsx` → `Foo.vue.tsx`).
fn artifact_path(vue: &Path, suffix: &str) -> PathBuf {
    let name = vue
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    vue.with_file_name(format!("{name}{suffix}"))
}

/// Inject `@verter/types` from the Rust constant into `<root>/node_modules`.
fn inject_verter_types(root: &Path) -> Result<PathBuf, MaterializeError> {
    let dir = root.join("node_modules").join("@verter").join("types");
    fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    let dts = dir.join("index.d.ts");
    fs::write(&dts, VERTER_TYPES_STANDALONE_DTS).map_err(|e| io_err(&dts, e))?;
    let pkg = dir.join("package.json");
    fs::write(&pkg, VERTER_TYPES_PACKAGE_JSON).map_err(|e| io_err(&pkg, e))?;
    Ok(dts)
}

/// Copy `src` into `dst` recursively (committed vendored shims only).
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), MaterializeError> {
    fs::create_dir_all(dst).map_err(|e| io_err(dst, e))?;
    for entry in fs::read_dir(src).map_err(|e| io_err(src, e))? {
        let entry = entry.map_err(|e| io_err(src, e))?;
        let ty = entry.file_type().map_err(|e| io_err(entry.path(), e))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ty.is_file() {
            fs::copy(&from, &to).map_err(|e| io_err(&to, e))?;
        }
    }
    Ok(())
}

/// Read the `version` field from a `package.json`, or `None` when the file is
/// absent, unparseable, or carries no string `version`.
fn read_pkg_version(pkg_json: &Path) -> Option<String> {
    let txt = fs::read_to_string(pkg_json).ok()?;
    let val: serde_json::Value = serde_json::from_str(&txt).ok()?;
    val.get("version")?.as_str().map(str::to_string)
}

/// Collect `(package_name, package_json_path)` for the vendored Vue packages the
/// version-sync contract validates. The `vue` core is REQUIRED whenever an
/// expected line is pinned: its `package.json` path is returned UNCONDITIONALLY —
/// even when the file is absent — so a missing core declaration is compared (and
/// surfaces as a `<absent>` mismatch) instead of being silently skipped. Every
/// copied `@vue/*` scope package is validated when present. Deterministically
/// ordered (`vue` first, then `@vue/*` lexicographically) so a mismatch is
/// reported against the first drifting — or missing-required — package.
fn collect_vendored_vue_packages(node_modules: &Path) -> Vec<(String, PathBuf)> {
    // `vue` core is mandatory under an expected-version contract; include its path
    // unconditionally so an absent/unreadable `package.json` is still compared.
    let mut out = vec![(
        "vue".to_string(),
        node_modules.join("vue").join("package.json"),
    )];
    let scope = node_modules.join("@vue");
    if let Ok(entries) = fs::read_dir(&scope) {
        let mut subs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        subs.sort();
        for sub in subs {
            let pkg_json = sub.join("package.json");
            if pkg_json.is_file() {
                let name = format!(
                    "@vue/{}",
                    sub.file_name().unwrap_or_default().to_string_lossy()
                );
                out.push((name, pkg_json));
            }
        }
    }
    out
}

/// Enforce vendored-Vue declaration version-sync: the REQUIRED `vue` core and
/// every copied `@vue/*` package's exact version must equal `expected`. A required
/// package whose `package.json` is absent or unreadable is itself a mismatch
/// (`found = "<absent>"`), never a silent pass. In strict mode a mismatch is a
/// hard error (the strict-CI gate); in non-strict it is appended to
/// `report.vue_version_warnings`. Silent declaration drift (or a missing core
/// declaration) would invalidate the baseline — the differential would compare
/// against a Vue line the provider never resolves — so this mirrors the strict
/// tsserver/tsgo tool-root pinning.
fn enforce_vue_version_sync(
    node_modules: &Path,
    expected: &str,
    strict: bool,
    report: &mut MaterializeReport,
) -> Result<(), MaterializeError> {
    for (package, pkg_json) in collect_vendored_vue_packages(node_modules) {
        let found = read_pkg_version(&pkg_json).unwrap_or_else(|| "<absent>".to_string());
        if found != expected {
            if strict {
                return Err(MaterializeError::VueVersionMismatch {
                    package,
                    expected: expected.to_string(),
                    found,
                });
            }
            report.vue_version_warnings.push(VueVersionMismatch {
                package,
                expected: expected.to_string(),
                found,
            });
        }
    }
    Ok(())
}

/// Copy or synthesize the provider-equivalent `tsconfig.json`.
fn ensure_tsconfig(root: &Path) -> Result<(PathBuf, bool), MaterializeError> {
    let p = root.join("tsconfig.json");
    if p.exists() {
        Ok((p, false))
    } else {
        fs::write(&p, FALLBACK_TSCONFIG).map_err(|e| io_err(&p, e))?;
        Ok((p, true))
    }
}

/// Recursively collect every `.vue` file under `dir`, skipping `node_modules`
/// and dot-directories.
fn collect_vue_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), MaterializeError> {
    for entry in fs::read_dir(dir).map_err(|e| io_err(dir, e))? {
        let entry = entry.map_err(|e| io_err(dir, e))?;
        let path = entry.path();
        let ty = entry.file_type().map_err(|e| io_err(&path, e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if ty.is_dir() {
            if name == "node_modules" || name.starts_with('.') {
                continue;
            }
            collect_vue_files(&path, out)?;
        } else if ty.is_file() && name.ends_with(".vue") {
            out.push(path);
        }
    }
    Ok(())
}

/// Whether `name` is an ordinary TS support file (not a generated Vue artifact).
/// Generated `.vue.tsx`/`.vue.ts`/`.vue.jsx` are excluded — their specifiers are
/// already rewritten on emission.
fn is_ts_support_name(name: &str) -> bool {
    if name.ends_with(".vue.tsx") || name.ends_with(".vue.ts") || name.ends_with(".vue.jsx") {
        return false;
    }
    [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|ext| name.ends_with(ext))
}

/// Recursively collect TS support files under `dir`, skipping `node_modules`,
/// dot-directories, and generated Vue artifacts.
fn collect_ts_support_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), MaterializeError> {
    for entry in fs::read_dir(dir).map_err(|e| io_err(dir, e))? {
        let entry = entry.map_err(|e| io_err(dir, e))?;
        let path = entry.path();
        let ty = entry.file_type().map_err(|e| io_err(&path, e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if ty.is_dir() {
            if name == "node_modules" || name.starts_with('.') {
                continue;
            }
            collect_ts_support_files(&path, out)?;
        } else if ty.is_file() && is_ts_support_name(&name) {
            out.push(path);
        }
    }
    Ok(())
}

/// Rewrite `.vue` import/reexport specifiers to `.vue.ts` in every materialized
/// TS support file (barrels, helpers) so the provider resolves cross-file Vue
/// imports/reexports through the public-API twins. Returns the files actually
/// changed.
fn rewrite_ts_support_files(root: &Path) -> Result<Vec<PathBuf>, MaterializeError> {
    let mut files = Vec::new();
    collect_ts_support_files(root, &mut files)?;
    files.sort();
    files.dedup();
    let mut changed = Vec::new();
    for file in files {
        let content = fs::read_to_string(&file).map_err(|e| io_err(&file, e))?;
        let rewritten = rewrite_vue_imports(&content);
        if rewritten != content {
            fs::write(&file, &rewritten).map_err(|e| io_err(&file, e))?;
            changed.push(file);
        }
    }
    Ok(changed)
}

/// Build a fully-initialized host backed by a filesystem workspace rooted at
/// `root`. Single CPU thread to avoid oversubscription across parallel tests.
fn build_host(root_str: &str) -> VerterHost {
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![root_str.to_string()],
        eager_preload: false,
    }));
    ws.set_project_graph(ProjectGraph::new());
    let ws_access: Arc<dyn WorkspaceAccess> = ws;
    VerterHost::new_with_scheduler_config(
        HostConfig::default(),
        ws_access,
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    )
}

/// Run the host-driven materialization pass over a workspace root.
pub fn materialize(req: &MaterializeRequest) -> Result<MaterializeReport, MaterializeError> {
    let root = &req.workspace_root;
    if !root.is_dir() {
        return Err(MaterializeError::BadRoot(root.display().to_string()));
    }

    // Scaffold: vendored shims first, then @verter/types, then tsconfig. The
    // vendored `node_modules` overlay is copied BEFORE `inject_verter_types` so
    // the Rust-constant `@verter/types` shim is authoritative: it
    // deterministically overwrites any vendored `@verter/types` declaration. A
    // vendored shim winning here would violate the contract that `@verter/types`
    // is generated from the exported Rust constant (never a vendored/TS
    // declaration) — the baseline would otherwise type-check the generated TSX
    // against the wrong helper declarations.
    if let Some(vendor) = &req.vendor_node_modules {
        copy_dir_recursive(vendor, &root.join("node_modules"))?;
    }
    let verter_types_dts = Some(inject_verter_types(root)?);
    let (tsconfig_path, synthesized_tsconfig) = ensure_tsconfig(root)?;

    let mut report = MaterializeReport {
        verter_types_dts,
        tsconfig_path: Some(tsconfig_path),
        synthesized_tsconfig,
        ..MaterializeReport::default()
    };

    // Vendored Vue declaration version-sync: the copied `vue`/`@vue/*`
    // declarations must match the workspace's resolved Vue line. Strict CI
    // hard-fails on drift; non-strict records a structured warning.
    if let Some(expected) = &req.expected_vue_version {
        enforce_vue_version_sync(
            &root.join("node_modules"),
            expected,
            req.strict_vue_version,
            &mut report,
        )?;
    }

    // Host.
    let root_str = canonicalize_path(&root.to_string_lossy());
    let host = build_host(&root_str);

    // Over-materialize every .vue under the root (covers the transitive
    // closure); the explicit entries are folded in for determinism.
    let mut vue_files = Vec::new();
    collect_vue_files(root, &mut vue_files)?;
    for entry in &req.entries {
        if entry.is_file() && !vue_files.iter().any(|p| p == entry) {
            vue_files.push(entry.clone());
        }
    }
    vue_files.sort();
    vue_files.dedup();

    let profile = ide_profile();
    let profile_desc = profile_descriptor(&profile);

    for vue in &vue_files {
        let content = fs::read_to_string(vue).map_err(|e| io_err(vue, e))?;
        let canonical = canonicalize_path(&vue.to_string_lossy());

        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.clone()),
                input_id: canonical.clone(),
                source: Arc::from(content.as_str()),
                file_kind: FileKind::VueSfc,
                aliases: vec![],
            })
            .map_err(|e| MaterializeError::Upsert {
                canonical: canonical.clone(),
                message: format!("{e:?}"),
            })?;

        host.ensure_loaded(&canonical);

        if let Err(e) = host.ensure_compiled(&canonical, &profile) {
            // A bad file must not abort the whole pass — record and move on.
            report
                .compile_errors
                .push((canonical.clone(), format!("{e:?}")));
        }

        // IDE artifact (.vue.tsx / .vue.jsx).
        if let Some(ide) = host.get_ide(&canonical, &profile) {
            let suffix = if ide.is_jsx { ".jsx" } else { ".tsx" };
            let gen_path = artifact_path(vue, suffix);
            let rewrite = rewrite_vue_imports_tracked(&ide.code);
            fs::write(&gen_path, &rewrite.output).map_err(|e| io_err(&gen_path, e))?;
            // Keep the recorded map consistent with the rewritten code: a
            // `.vue`→`.vue.ts` rewrite that lengthens a line shifts every later
            // generated position on that line, so the host's pre-rewrite map
            // would resolve post-rewrite offsets to the wrong source position.
            let shifted_map = ide
                .source_map
                .as_deref()
                .map(|m| shift_source_map_for_insertions(m, &ide.code, &rewrite.insertions));
            match classify_source_map(&profile_desc, shifted_map.as_deref()) {
                MapOutcome::Identity(id) => {
                    report.source_map_identities.insert(canonical.clone(), id);
                }
                MapOutcome::Absent => report.map_absent.push(canonical.clone()),
            }
            report.ide_artifacts.push(MaterializedArtifact {
                source_vue: canonical.clone(),
                generated_path: gen_path,
                content: rewrite.output,
                source_map: shifted_map,
                source_map_present: ide.source_map.is_some(),
            });
        }

        // Public-API twin (.vue.ts).
        if let Some(api) = host.get_public_api(&canonical) {
            let gen_path = artifact_path(vue, ".ts");
            let rewrite = rewrite_vue_imports_tracked(&api.code);
            fs::write(&gen_path, &rewrite.output).map_err(|e| io_err(&gen_path, e))?;
            let shifted_map = api
                .source_map
                .as_deref()
                .map(|m| shift_source_map_for_insertions(m, &api.code, &rewrite.insertions));
            report.public_api_twins.push(MaterializedArtifact {
                source_vue: canonical.clone(),
                generated_path: gen_path,
                content: rewrite.output,
                source_map: shifted_map,
                source_map_present: api.source_map.is_some(),
            });
        }
    }

    // Rewrite `.vue` specifiers in ordinary TS support files (barrels included)
    // so the provider resolves cross-file reexports through the public-API twins,
    // not raw `.vue` paths it cannot resolve.
    report.support_rewrites = rewrite_ts_support_files(root)?;

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    // ── pure helpers ──────────────────────────────────────────────────────

    #[test]
    fn source_map_identity_is_stable_and_profile_sensitive() {
        let a = compute_source_map_identity("p1", "MAP");
        let b = compute_source_map_identity("p1", "MAP");
        let c = compute_source_map_identity("p2", "MAP");
        let d = compute_source_map_identity("p1", "MAP2");
        assert_eq!(a, b, "same inputs => same identity");
        assert_ne!(a, c, "different profile => different identity");
        assert_ne!(a, d, "different map content => different identity");
    }

    #[test]
    fn rewrite_vue_imports_is_idempotent_and_covers_both_quotes() {
        assert_eq!(
            rewrite_vue_imports("import X from './X.vue'"),
            "import X from './X.vue.ts'"
        );
        assert_eq!(
            rewrite_vue_imports("import X from \"./X.vue\""),
            "import X from \"./X.vue.ts\""
        );
        // Idempotent — an already-rewritten specifier is untouched.
        let once = rewrite_vue_imports("import X from './X.vue'");
        assert_eq!(rewrite_vue_imports(&once), once);
    }

    #[test]
    fn map_absent_is_recorded_not_crashed() {
        // A present map yields a stable identity.
        match classify_source_map("p", Some("MAP")) {
            MapOutcome::Identity(id) => assert_eq!(id, compute_source_map_identity("p", "MAP")),
            MapOutcome::Absent => panic!("present map must yield an identity"),
        }
        // An absent map is recorded as Absent — the map-Option-None path never
        // panics ($/getCompiledCode-style map-absent handling).
        assert_eq!(classify_source_map("p", None), MapOutcome::Absent);
    }

    // ── the recorded map tracks the rewritten generated code ─────────────────

    #[test]
    fn source_map_shift_resolves_post_rewrite_offset_to_correct_source() {
        use oxc_sourcemap::{SourceMap, Token};
        use std::borrow::Cow;

        // Generated TSX: a `.vue` reexport, then a probe-target token AFTER it on
        // the SAME line (so the rewrite's byte-length change shifts the target).
        let original_code = "export { default as X } from './X.vue';export const greeting = 1";
        let x_col = original_code.find("as X").expect("X") as u32 + "as ".len() as u32;
        let spec_col = original_code.find("'./X.vue'").expect("specifier") as u32;
        let greeting_col = original_code.find("greeting").expect("greeting") as u32;

        // A V3 map: the import name and specifier (both BEFORE the insertion) and
        // the post-rewrite target `greeting` (AFTER the insertion).
        let tokens = vec![
            Token::new(0, x_col, 5, 0, Some(0), None),
            Token::new(0, spec_col, 6, 0, Some(0), None),
            Token::new(0, greeting_col, 10, 5, Some(0), None),
        ]
        .into_boxed_slice();
        let map = SourceMap::new(
            None,
            vec![],
            None,
            vec![Cow::Borrowed("X.vue")],
            vec![None],
            tokens,
            None,
        );
        let map_json = map.to_json_string();

        // Apply the tracked rewrite and shift the map to match it.
        let rewrite = rewrite_vue_imports_tracked(original_code);
        assert!(rewrite.output.contains("./X.vue.ts"), "rewrite must run");
        assert_eq!(rewrite.insertions.len(), 1, "exactly one .vue specifier");
        let rewritten_greeting_col = rewrite.output.find("greeting").expect("greeting") as u32;
        assert_eq!(
            rewritten_greeting_col,
            greeting_col + VUE_TWIN_SUFFIX.len() as u32,
            "the target shifted right by the inserted suffix length"
        );

        let shifted_json =
            shift_source_map_for_insertions(&map_json, original_code, &rewrite.insertions);

        // The SHIFTED map resolves the post-rewrite target offset EXACTLY to its
        // original source position.
        let shifted = SourceMap::from_json_string(&shifted_json).expect("parse shifted");
        let lt = shifted.generate_lookup_table();
        let tok = shifted
            .lookup_token(&lt, 0, rewritten_greeting_col)
            .expect("token at post-rewrite target");
        assert_eq!(
            tok.get_dst_col(),
            rewritten_greeting_col,
            "shifted map has a token EXACTLY at the post-rewrite target column"
        );
        assert_eq!(
            (tok.get_src_line(), tok.get_src_col()),
            (10, 5),
            "post-rewrite target maps to the correct original source position"
        );
        // A token BEFORE the insertion is unchanged.
        let x_tok = shifted.lookup_token(&lt, 0, x_col).expect("x token");
        assert_eq!(
            x_tok.get_dst_col(),
            x_col,
            "a pre-insertion token must not shift"
        );
        assert_eq!((x_tok.get_src_line(), x_tok.get_src_col()), (5, 0));

        // Discrimination: the UN-shifted (host) map cannot exactly locate the
        // post-rewrite target — its `greeting` token is still at the OLD column,
        // so a probe at the post-rewrite offset lands off-by-suffix-length.
        let orig = SourceMap::from_json_string(&map_json).expect("parse orig");
        let olt = orig.generate_lookup_table();
        let off_tok = orig
            .lookup_token(&olt, 0, rewritten_greeting_col)
            .expect("floor token");
        assert_ne!(
            off_tok.get_dst_col(),
            rewritten_greeting_col,
            "the unshifted map has NO token at the post-rewrite target (the bug)"
        );
    }

    #[test]
    fn source_map_shift_is_cumulative_across_multiple_insertions_on_one_line() {
        use oxc_sourcemap::{SourceMap, Token};
        use std::borrow::Cow;

        // TWO `.vue` specifiers on ONE generated line, with a token between them
        // and a token after both — the cumulative (2×) shift must apply past the
        // second insertion, and a token sitting EXACTLY at an insertion column must
        // shift too (the `col >= c` boundary).
        let original_code = "export {a} from './A.vue';export {b} from './B.vue';const z=1";
        let rewrite = rewrite_vue_imports_tracked(original_code);
        assert_eq!(
            rewrite.insertions.len(),
            2,
            "two specifiers → two insertions"
        );
        let ins1 = rewrite.insertions[0] as u32; // closing-quote col of './A.vue'
        let ins2 = rewrite.insertions[1] as u32; // closing-quote col of './B.vue'
        assert!(ins1 < ins2, "ins1 {ins1} < ins2 {ins2}");
        let suffix = VUE_TWIN_SUFFIX.len() as u32; // 3

        // Tokens in the ORIGINAL generated coordinate system, each tagged by a
        // unique source row so it can be identified after the shift.
        let before = 0u32; // before both insertions
        let at_ins1 = ins1; // EXACTLY at insertion 1 (boundary: col == c)
        let between = ins1 + 5; // strictly between the two insertions
        let at_ins2 = ins2; // EXACTLY at insertion 2 (both insertions at/before)
        let after = ins2 + 4; // after both insertions
        assert!(between < ins2, "the between-token must fall before ins2");
        let tokens = vec![
            Token::new(0, before, 1, 0, Some(0), None),
            Token::new(0, at_ins1, 2, 0, Some(0), None),
            Token::new(0, between, 3, 0, Some(0), None),
            Token::new(0, at_ins2, 4, 0, Some(0), None),
            Token::new(0, after, 5, 0, Some(0), None),
        ]
        .into_boxed_slice();
        let map = SourceMap::new(
            None,
            vec![],
            None,
            vec![Cow::Borrowed("x")],
            vec![None],
            tokens,
            None,
        );
        let shifted_json = shift_source_map_for_insertions(
            &map.to_json_string(),
            original_code,
            &rewrite.insertions,
        );
        let shifted = SourceMap::from_json_string(&shifted_json).expect("parse shifted");

        // Expected post-shift generated column, keyed by source row.
        let expect = |src_row: u32| -> u32 {
            match src_row {
                1 => before,               // before everything → unchanged
                2 => at_ins1 + suffix,     // boundary at ins1 (>=) → one shift
                3 => between + suffix,     // one insertion before it
                4 => at_ins2 + 2 * suffix, // boundary at ins2 → both insertions count
                5 => after + 2 * suffix,   // cumulative 2× past both
                _ => unreachable!(),
            }
        };
        let mut seen = 0;
        for tok in shifted.get_tokens() {
            let row = tok.get_src_line();
            assert_eq!(
                tok.get_dst_col(),
                expect(row),
                "src row {row}: shifted dst col mismatch"
            );
            seen += 1;
        }
        assert_eq!(seen, 5, "all five tokens survived the shift");

        // Discrimination: a single (non-cumulative) shift would move the trailing
        // token by only +3, never the +6 the cumulative math produces.
        assert_ne!(
            expect(5),
            after + suffix,
            "the cumulative shift must exceed a single-insertion shift"
        );
    }

    #[test]
    fn shift_source_map_is_noop_without_insertions_and_safe_on_garbage() {
        use oxc_sourcemap::{SourceMap, Token};
        use std::borrow::Cow;
        let tokens = vec![Token::new(0, 5, 1, 0, Some(0), None)].into_boxed_slice();
        let map = SourceMap::new(
            None,
            vec![],
            None,
            vec![Cow::Borrowed("a.ts")],
            vec![None],
            tokens,
            None,
        );
        let json = map.to_json_string();
        // No insertions → identical map back.
        assert_eq!(
            shift_source_map_for_insertions(&json, "const x = 1", &[]),
            json
        );
        // A malformed map is returned unchanged (best-effort, never dropped).
        assert_eq!(
            shift_source_map_for_insertions("not a source map", "x", &[0]),
            "not a source map"
        );
    }

    #[test]
    fn byte_offset_to_line_utf16col_counts_utf16_units() {
        // `é` is 2 UTF-8 bytes but 1 UTF-16 code unit.
        let text = "café\nxy";
        // 'f' (byte 2) on line 0 → col 2.
        assert_eq!(byte_offset_to_line_utf16col(text, 2), (0, 2));
        // End of `café` (byte 5, the newline) → line 0, col 4 (c,a,f,é).
        assert_eq!(byte_offset_to_line_utf16col(text, 5), (0, 4));
        // 'y' on line 1 (byte 7) → line 1, col 1.
        assert_eq!(byte_offset_to_line_utf16col(text, 7), (1, 1));
    }

    #[test]
    fn artifact_path_appends_to_full_name() {
        let p = artifact_path(Path::new("/ws/Foo.vue"), ".tsx");
        assert!(p.ends_with("Foo.vue.tsx"), "{p:?}");
        let t = artifact_path(Path::new("/ws/Foo.vue"), ".ts");
        assert!(t.ends_with("Foo.vue.ts"), "{t:?}");
    }

    // ── @verter/types + vendored shims (no runtime install) ───────────────

    #[test]
    fn injects_verter_types_and_copies_vendored_vue_without_install() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // A vendored vue shim that exports `ref` — proves resolution is possible
        // off the committed shim, with no `npm install`.
        let vendor = root.join("vendor").join("node_modules");
        write(
            &vendor.join("vue").join("index.d.ts"),
            "export declare function ref<T>(v: T): { value: T };\n",
        );
        write(
            &vendor.join("vue").join("package.json"),
            r#"{ "name": "vue", "version": "3.5.0", "types": "index.d.ts" }"#,
        );

        let entry = root.join("Entry.vue");
        write(
            &entry,
            "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst n = ref(0)\n</script>\n<template><div>{{ n }}</div></template>\n",
        );

        let report = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![entry.clone()],
            vendor_node_modules: Some(vendor.clone()),
            expected_vue_version: None,
            strict_vue_version: false,
        })
        .unwrap();

        // @verter/types injected verbatim from the Rust constant.
        let dts = root
            .join("node_modules")
            .join("@verter")
            .join("types")
            .join("index.d.ts");
        assert!(dts.exists(), "@verter/types/index.d.ts must be present");
        assert_eq!(
            fs::read_to_string(&dts).unwrap(),
            VERTER_TYPES_STANDALONE_DTS,
            "must reuse the Rust constant, not a hand-written d.ts"
        );

        // Vendored vue copied into node_modules (no install).
        let vue_dts = root.join("node_modules").join("vue").join("index.d.ts");
        assert!(vue_dts.exists(), "vendored vue shim must be copied");
        assert!(fs::read_to_string(&vue_dts)
            .unwrap()
            .contains("function ref"));

        // Entry IDE artifact emitted and references @verter/types.
        assert_eq!(report.ide_artifacts.len(), 1);
        let tsx = &report.ide_artifacts[0];
        assert!(tsx.generated_path.ends_with("Entry.vue.tsx"));
        assert!(
            tsx.content.contains("@verter/types"),
            "generated TSX must import from @verter/types"
        );
        // Public-API twin emitted.
        assert_eq!(report.public_api_twins.len(), 1);
        assert!(report.public_api_twins[0]
            .generated_path
            .ends_with("Entry.vue.ts"));
        // Negative: nothing failed to compile.
        assert!(
            report.compile_errors.is_empty(),
            "{:?}",
            report.compile_errors
        );
    }

    #[test]
    fn vendored_verter_types_loses_to_rust_constant_shim() {
        // A vendor overlay that ships a STALE `@verter/types` declaration. The
        // Rust-constant shim must win: `@verter/types` is generated from the
        // exported constant, never a vendored/TS declaration — otherwise the
        // baseline would type-check the generated TSX against the wrong helper
        // declarations.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        const STALE_DTS: &str = "export type StaleVerterHelpers = never; // must not win\n";
        const STALE_PKG: &str = r#"{ "name": "@verter/types", "version": "9.9.9-stale" }"#;
        // Guard the discriminator itself: the sentinels must differ from the
        // authoritative Rust constants, or the assertions would pass vacuously.
        assert_ne!(STALE_DTS, VERTER_TYPES_STANDALONE_DTS);
        assert_ne!(STALE_PKG, VERTER_TYPES_PACKAGE_JSON);

        let vendor = root.join("vendor").join("node_modules");
        write(
            &vendor.join("@verter").join("types").join("index.d.ts"),
            STALE_DTS,
        );
        write(
            &vendor.join("@verter").join("types").join("package.json"),
            STALE_PKG,
        );

        write(&root.join("A.vue"), "<template><div/></template>\n");

        materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![],
            vendor_node_modules: Some(vendor),
            expected_vue_version: None,
            strict_vue_version: false,
        })
        .unwrap();

        // The Rust-constant shim is authoritative — the vendored overlay's
        // stale `@verter/types` declaration is overwritten, not preserved.
        let dts = root
            .join("node_modules")
            .join("@verter")
            .join("types")
            .join("index.d.ts");
        assert_eq!(
            fs::read_to_string(&dts).unwrap(),
            VERTER_TYPES_STANDALONE_DTS,
            "vendored @verter/types/index.d.ts must lose to the Rust constant"
        );
        let pkg = root
            .join("node_modules")
            .join("@verter")
            .join("types")
            .join("package.json");
        assert_eq!(
            fs::read_to_string(&pkg).unwrap(),
            VERTER_TYPES_PACKAGE_JSON,
            "vendored @verter/types/package.json must lose to the Rust constant"
        );
    }

    // ── vendored Vue declaration version-sync ─────────────────────────────

    #[test]
    fn vendored_vue_version_sync_matches_passes_and_strict_mismatch_hard_fails() {
        // Build a vendor node_modules with `vue` + `@vue/compiler-core` pinned at
        // explicit versions, then a trivial fixture to materialize.
        fn vendor_at(root: &Path, vue_ver: &str, compiler_ver: &str) -> PathBuf {
            let vendor = root.join("vendor").join("node_modules");
            write(
                &vendor.join("vue").join("package.json"),
                &format!(r#"{{ "name": "vue", "version": "{vue_ver}" }}"#),
            );
            write(
                &vendor.join("vue").join("index.d.ts"),
                "export declare const x: number;\n",
            );
            write(
                &vendor
                    .join("@vue")
                    .join("compiler-core")
                    .join("package.json"),
                &format!(r#"{{ "name": "@vue/compiler-core", "version": "{compiler_ver}" }}"#),
            );
            vendor
        }

        // Matching versions → materialize ok, no warnings.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let vendor = vendor_at(root, "3.5.13", "3.5.13");
        write(&root.join("A.vue"), "<template><div/></template>\n");
        let ok = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![],
            vendor_node_modules: Some(vendor),
            expected_vue_version: Some("3.5.13".to_string()),
            strict_vue_version: true,
        })
        .expect("matching vendored Vue versions must materialize");
        assert!(
            ok.vue_version_warnings.is_empty(),
            "no warnings on an exact version match: {:?}",
            ok.vue_version_warnings
        );

        // A drifting `@vue/*` under strict → hard error naming the package + versions.
        let tmp2 = tempfile::tempdir().unwrap();
        let root2 = tmp2.path();
        let vendor2 = vendor_at(root2, "3.5.13", "3.4.0"); // compiler-core drifts
        write(&root2.join("A.vue"), "<template><div/></template>\n");
        let err = materialize(&MaterializeRequest {
            workspace_root: root2.to_path_buf(),
            entries: vec![],
            vendor_node_modules: Some(vendor2),
            expected_vue_version: Some("3.5.13".to_string()),
            strict_vue_version: true,
        })
        .unwrap_err();
        match err {
            MaterializeError::VueVersionMismatch {
                package,
                expected,
                found,
            } => {
                assert_eq!(package, "@vue/compiler-core");
                assert_eq!(expected, "3.5.13");
                assert_eq!(found, "3.4.0");
            }
            other => panic!("expected a hard VueVersionMismatch under strict, got {other:?}"),
        }

        // The SAME drift in non-strict → recorded structured warning, never an error.
        let tmp3 = tempfile::tempdir().unwrap();
        let root3 = tmp3.path();
        let vendor3 = vendor_at(root3, "3.5.13", "3.4.0");
        write(&root3.join("A.vue"), "<template><div/></template>\n");
        let report = materialize(&MaterializeRequest {
            workspace_root: root3.to_path_buf(),
            entries: vec![],
            vendor_node_modules: Some(vendor3),
            expected_vue_version: Some("3.5.13".to_string()),
            strict_vue_version: false,
        })
        .expect("non-strict records a warning, never errors");
        assert!(
            report
                .vue_version_warnings
                .iter()
                .any(|w| w.package == "@vue/compiler-core"
                    && w.expected == "3.5.13"
                    && w.found == "3.4.0"),
            "non-strict mismatch must be recorded: {:?}",
            report.vue_version_warnings
        );
        // Negative: the matching `vue` core is NOT warned about.
        assert!(
            !report
                .vue_version_warnings
                .iter()
                .any(|w| w.package == "vue"),
            "a matching vue core must not be warned: {:?}",
            report.vue_version_warnings
        );
    }

    #[test]
    fn strict_vue_version_sync_hard_fails_when_required_vue_core_is_absent() {
        // The strict contract REQUIRES the vendored `vue/package.json` version be
        // read and compared. A vendor that copies a matching `@vue/*` line but NO
        // `vue` core must NOT silently pass (an empty/short iteration returning Ok):
        // the missing required core declaration is itself a strict mismatch.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let vendor = root.join("vendor").join("node_modules");
        // A matching `@vue/compiler-core`, but deliberately NO `vue/package.json`.
        write(
            &vendor
                .join("@vue")
                .join("compiler-core")
                .join("package.json"),
            r#"{ "name": "@vue/compiler-core", "version": "3.5.13" }"#,
        );
        write(&root.join("A.vue"), "<template><div/></template>\n");

        let err = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![],
            vendor_node_modules: Some(vendor),
            expected_vue_version: Some("3.5.13".to_string()),
            strict_vue_version: true,
        })
        .expect_err("a missing required `vue/package.json` must hard-fail under strict");
        match err {
            MaterializeError::VueVersionMismatch {
                package,
                expected,
                found,
            } => {
                assert_eq!(
                    package, "vue",
                    "the missing required package is the vue core"
                );
                assert_eq!(expected, "3.5.13");
                assert_eq!(
                    found, "<absent>",
                    "a missing/unreadable package.json surfaces as <absent>"
                );
            }
            other => {
                panic!("expected a hard VueVersionMismatch for the absent vue core, got {other:?}")
            }
        }
    }

    #[test]
    fn nonstrict_vue_version_sync_records_warning_when_required_vue_core_is_absent() {
        // The SAME absent-core case in non-strict mode → a recorded structured
        // warning naming `vue` with `found = "<absent>"`, never an error.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let vendor = root.join("vendor").join("node_modules");
        write(
            &vendor
                .join("@vue")
                .join("compiler-core")
                .join("package.json"),
            r#"{ "name": "@vue/compiler-core", "version": "3.5.13" }"#,
        );
        write(&root.join("A.vue"), "<template><div/></template>\n");

        let report = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![],
            vendor_node_modules: Some(vendor),
            expected_vue_version: Some("3.5.13".to_string()),
            strict_vue_version: false,
        })
        .expect("non-strict records a warning, never errors");
        assert!(
            report
                .vue_version_warnings
                .iter()
                .any(|w| w.package == "vue" && w.expected == "3.5.13" && w.found == "<absent>"),
            "a missing required vue core must be recorded as a <absent> warning: {:?}",
            report.vue_version_warnings
        );
        // Negative: the matching `@vue/compiler-core` is NOT warned about.
        assert!(
            !report
                .vue_version_warnings
                .iter()
                .any(|w| w.package == "@vue/compiler-core"),
            "a matching @vue/* package must not be warned: {:?}",
            report.vue_version_warnings
        );
    }

    // ── transitive closure: direct child + barrel-reexported child ─────────

    #[test]
    fn transitive_closure_produces_twins_for_child_and_barrel_reexport() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Entry imports a direct child AND a barrel that re-exports a panel.
        write(
            &root.join("Entry.vue"),
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\nimport { Panel } from './components'\n</script>\n<template><Child /><Panel /></template>\n",
        );
        write(
            &root.join("Child.vue"),
            "<script setup lang=\"ts\">\nconst label = 'child'\n</script>\n<template><span>{{ label }}</span></template>\n",
        );
        write(
            &root.join("components").join("Panel.vue"),
            "<script setup lang=\"ts\">\nconst title = 'panel'\n</script>\n<template><h1>{{ title }}</h1></template>\n",
        );
        // Barrel re-exporting the panel.
        write(
            &root.join("components").join("index.ts"),
            "export { default as Panel } from './Panel.vue'\n",
        );

        let report = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![root.join("Entry.vue")],
            vendor_node_modules: None,
            expected_vue_version: None,
            strict_vue_version: false,
        })
        .unwrap();

        let twin_names: Vec<String> = report
            .public_api_twins
            .iter()
            .map(|a| {
                a.generated_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        // The imported child AND the barrel-reexported child both got .vue.ts twins.
        assert!(
            twin_names.contains(&"Entry.vue.ts".to_string()),
            "{twin_names:?}"
        );
        assert!(
            twin_names.contains(&"Child.vue.ts".to_string()),
            "imported child twin missing: {twin_names:?}"
        );
        assert!(
            twin_names.contains(&"Panel.vue.ts".to_string()),
            "barrel-reexported child twin missing: {twin_names:?}"
        );

        // Entry's TSX rewrites the .vue import to .vue.ts on disk.
        let entry_tsx = report
            .ide_artifacts
            .iter()
            .find(|a| a.generated_path.ends_with("Entry.vue.tsx"))
            .expect("entry tsx");
        assert!(
            entry_tsx.content.contains("Child.vue.ts"),
            "entry TSX must import the rewritten child twin"
        );
        // Negative: no raw `.vue'` specifier survives the rewrite.
        assert!(
            !entry_tsx.content.contains("./Child.vue'"),
            "raw .vue specifier must be rewritten"
        );

        // Twins are real declarations, not empty.
        for twin in &report.public_api_twins {
            assert!(
                !twin.content.trim().is_empty(),
                "empty twin: {:?}",
                twin.generated_path
            );
        }

        // The on-disk barrel's `./Panel.vue` reexport is rewritten to the twin,
        // so the provider resolves the reexport THROUGH `Panel.vue.ts` rather
        // than a raw `.vue` path it cannot resolve.
        let barrel = fs::read_to_string(root.join("components").join("index.ts")).unwrap();
        assert!(
            barrel.contains("./Panel.vue.ts"),
            "barrel reexport must be rewritten to the twin: {barrel:?}"
        );
        // Negative: no raw `./Panel.vue'` specifier survives in the barrel.
        assert!(
            !barrel.contains("./Panel.vue'"),
            "raw .vue reexport must not survive the rewrite: {barrel:?}"
        );
        // The rewrite is recorded for the runner.
        assert!(
            report
                .support_rewrites
                .iter()
                .any(|p| p.ends_with("index.ts")),
            "barrel rewrite must be recorded: {:?}",
            report.support_rewrites
        );
    }

    #[test]
    fn support_file_rewrite_skips_string_literals_and_comments_end_to_end() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // A child .vue so its twin exists for the rewritten specifier to resolve.
        write(
            &root.join("Child.vue"),
            "<script setup lang=\"ts\">\nconst label = 'child'\n</script>\n<template><span/></template>\n",
        );
        // A support .ts barrel carrying a real reexport specifier AND a
        // non-specifier `.vue` string literal + a comment that both mention `.vue`
        // immediately before a closing quote — the shape a bare before-quote scan
        // would wrongly rewrite, but a specifier-aware rewrite must leave intact.
        write(
            &root.join("barrel.ts"),
            concat!(
                "export { default as Child } from './Child.vue'\n",
                "export const note = './Child.vue'\n",
                "// fallback path: './Child.vue'\n",
            ),
        );

        let report = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![],
            vendor_node_modules: None,
            expected_vue_version: None,
            strict_vue_version: false,
        })
        .unwrap();

        let barrel = fs::read_to_string(root.join("barrel.ts")).unwrap();
        // The reexport specifier IS rewritten to the twin.
        assert!(
            barrel.contains("export { default as Child } from './Child.vue.ts'"),
            "reexport specifier must be rewritten: {barrel}"
        );
        // The plain string assignment (not a specifier) is UNCHANGED.
        assert!(
            barrel.contains("export const note = './Child.vue'\n"),
            "non-specifier string literal must not be rewritten: {barrel}"
        );
        // The comment is UNCHANGED.
        assert!(
            barrel.contains("// fallback path: './Child.vue'"),
            "comment must not be rewritten: {barrel}"
        );
        // Exactly ONE `.vue.ts` exists — only the specifier gained it; the literal
        // and the comment did not.
        assert_eq!(
            barrel.matches(".vue.ts").count(),
            1,
            "only the import specifier may gain `.vue.ts`: {barrel}"
        );
        // The rewrite was recorded for the runner.
        assert!(
            report
                .support_rewrites
                .iter()
                .any(|p| p.ends_with("barrel.ts")),
            "barrel rewrite must be recorded: {:?}",
            report.support_rewrites
        );
    }

    #[test]
    fn ide_artifact_records_a_shifted_source_map_consistent_with_rewritten_code() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Entry imports a child `.vue` → the generated TSX carries a `.vue`
        // specifier that materialization rewrites to `.vue.ts`.
        write(
            &root.join("Entry.vue"),
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\nconst greeting: string = 'hi'\n</script>\n<template><Child />{{ greeting }}</template>\n",
        );
        write(
            &root.join("Child.vue"),
            "<script setup lang=\"ts\">\nconst label = 'child'\n</script>\n<template><span>{{ label }}</span></template>\n",
        );
        let report = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![root.join("Entry.vue")],
            vendor_node_modules: None,
            expected_vue_version: None,
            strict_vue_version: false,
        })
        .unwrap();

        let entry = report
            .ide_artifacts
            .iter()
            .find(|a| a.generated_path.ends_with("Entry.vue.tsx"))
            .expect("entry tsx");
        // The rewrite happened in the recorded content.
        assert!(entry.content.contains("Child.vue.ts"));
        assert!(!entry.content.contains("./Child.vue'"));
        // When the host produced a map, the recorded map is present AND well-formed
        // V3 JSON (the shift round-tripped it through oxc_sourcemap), so a
        // position-resolving consumer reads a map consistent with the rewrite.
        if entry.source_map_present {
            let map_json = entry
                .source_map
                .as_deref()
                .expect("source_map_present implies a recorded map");
            let parsed = oxc_sourcemap::SourceMap::from_json_string(map_json)
                .expect("recorded map must be valid V3 JSON");
            assert!(
                parsed.get_tokens().count() > 0,
                "the shifted map must retain its generated tokens"
            );
        }
    }

    #[test]
    fn rewrite_vue_imports_leaves_wildcard_module_glob_intact() {
        // A glob module declaration must NOT be corrupted into `*.vue.ts`.
        let shim = "declare module '*.vue' { const c: unknown; export default c }";
        assert_eq!(rewrite_vue_imports(shim), shim);
        // A concrete reexport in the same family IS rewritten (both quote forms).
        assert_eq!(
            rewrite_vue_imports("export { default as Panel } from './Panel.vue'"),
            "export { default as Panel } from './Panel.vue.ts'"
        );
        assert_eq!(
            rewrite_vue_imports("from \"./Panel.vue\""),
            "from \"./Panel.vue.ts\""
        );
        // A `.vue`-prefixed but non-specifier-ending name is left untouched.
        assert_eq!(
            rewrite_vue_imports("import x from './theme.vuetify'"),
            "import x from './theme.vuetify'"
        );
    }

    #[test]
    fn rewrite_only_touches_import_specifiers_not_string_literals_or_comments() {
        // A support `.ts` carrying a real import specifier AND a non-specifier
        // `.vue` string literal / comments. Only the specifier may be rewritten —
        // an ordinary string literal or a comment that mentions `.vue` must stay
        // byte-for-byte intact (rewriting it would change TS semantics).
        let src = concat!(
            "import Child from \"./Child.vue\"\n",
            "const label = \"see ./Child.vue\"\n",
            "// import Other from \"./Other.vue\"\n",
            "/* block ./Block.vue mention */\n",
            "export { default as P } from './P.vue'\n",
            "const dyn = () => import('./Lazy.vue')\n",
        );
        let out = rewrite_vue_imports(src);

        // Real static import specifier → rewritten.
        assert!(
            out.contains("import Child from \"./Child.vue.ts\""),
            "static import specifier must be rewritten: {out}"
        );
        // Reexport specifier → rewritten.
        assert!(
            out.contains("export { default as P } from './P.vue.ts'"),
            "reexport specifier must be rewritten: {out}"
        );
        // Dynamic import() specifier → rewritten.
        assert!(
            out.contains("import('./Lazy.vue.ts')"),
            "dynamic import specifier must be rewritten: {out}"
        );
        // Ordinary string literal → UNCHANGED.
        assert!(
            out.contains("const label = \"see ./Child.vue\""),
            "string literal must not be rewritten: {out}"
        );
        assert!(
            !out.contains("see ./Child.vue.ts"),
            "the string-literal `.vue` must not gain a `.ts`: {out}"
        );
        // Line comment → UNCHANGED.
        assert!(
            out.contains("// import Other from \"./Other.vue\""),
            "line comment must not be rewritten: {out}"
        );
        assert!(
            !out.contains("./Other.vue.ts"),
            "the commented `.vue` must not gain a `.ts`: {out}"
        );
        // Block comment → UNCHANGED.
        assert!(
            out.contains("/* block ./Block.vue mention */"),
            "block comment must not be rewritten: {out}"
        );
        assert!(
            !out.contains("./Block.vue.ts"),
            "the block-commented `.vue` must not gain a `.ts`: {out}"
        );
    }

    #[test]
    fn rewrite_leaves_regex_and_division_untouched_but_rewrites_real_imports() {
        // A regex literal whose body contains a balanced-quote `.vue` path right
        // after the word `from`. A regex-blind lexer mis-reads the `'./Child.vue'`
        // quote run as an import specifier and wrongly appends `.ts`; a regex-aware
        // lexer consumes the whole literal so its interior never becomes a string
        // token. The `/` here is in regex position (it follows `=`).
        let regex_line = "const r = /from './Child.vue'/";
        assert_eq!(
            rewrite_vue_imports(regex_line),
            regex_line,
            "a `.vue` quote run inside a regex literal must never be rewritten"
        );

        // A regex (with a `from '…vue'` shape inside) on one line, then a REAL
        // import on the next. The regex stays verbatim and the import is rewritten;
        // the regex scan is newline-bounded, so it cannot swallow the import line.
        let mixed = concat!("const re = /from 'X.vue'/\n", "import B from './B.vue'\n");
        let out = rewrite_vue_imports(mixed);
        assert!(
            out.contains("/from 'X.vue'/\n"),
            "the regex literal must be left byte-for-byte: {out}"
        );
        assert!(
            !out.contains("X.vue.ts"),
            "the regex-interior specifier-shaped quote must not gain `.ts`: {out}"
        );
        assert!(
            out.contains("import B from './B.vue.ts'"),
            "a real import after a regex must still be rewritten: {out}"
        );

        // Division (`a / b`) is NOT a regex: the operands stay intact (the `/`
        // follows an identifier) and a following real import is still rewritten.
        let div = concat!("const n = a / b\n", "import C from \"./C.vue\"\n");
        let outd = rewrite_vue_imports(div);
        assert!(outd.contains("a / b"), "division must be untouched: {outd}");
        assert!(
            outd.contains("import C from \"./C.vue.ts\""),
            "a real import after a division must be rewritten: {outd}"
        );
    }

    #[test]
    fn synthesizes_tsconfig_when_absent_and_keeps_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("A.vue"), "<template><div/></template>\n");

        let report = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![],
            vendor_node_modules: None,
            expected_vue_version: None,
            strict_vue_version: false,
        })
        .unwrap();
        assert!(report.synthesized_tsconfig);
        let cfg = fs::read_to_string(root.join("tsconfig.json")).unwrap();
        assert!(cfg.contains("\"jsxImportSource\": \"vue\""));
        assert!(cfg.contains("\"allowArbitraryExtensions\": true"));

        // Second run keeps the existing tsconfig (does not re-synthesize).
        let report2 = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![],
            vendor_node_modules: None,
            expected_vue_version: None,
            strict_vue_version: false,
        })
        .unwrap();
        assert!(!report2.synthesized_tsconfig);
    }

    #[test]
    fn bad_root_is_rejected() {
        let err = materialize(&MaterializeRequest {
            workspace_root: PathBuf::from("/no/such/dir/verter-dx-xyz"),
            entries: vec![],
            vendor_node_modules: None,
            expected_vue_version: None,
            strict_vue_version: false,
        })
        .unwrap_err();
        assert!(matches!(err, MaterializeError::BadRoot(_)));
    }
}
