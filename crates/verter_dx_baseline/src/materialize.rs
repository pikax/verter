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
#[path = "materialize_tests.rs"]
mod materialize_tests;
