//! Canonical filesystem-path normalization — the single owner for the whole
//! workspace.
//!
//! Every consumer crate (LSP, type-runtime/tsserver IPC, scheduler source
//! loader, workspace project graph) routes path-id normalization through this
//! module so the canonical-ID format is identical everywhere.
//!
//! Canonical-ID format:
//! - Forward slashes only (no backslashes).
//! - Windows drive prefixes: lowercase drive letter, colon kept (`c:/Users/Dev`)
//!   — never `C:/`, never the drive-as-segment `/c/` form.
//! - `\\?\` / `\\?\UNC\` extended-length prefixes stripped.
//! - No trailing slash, except the roots `/` and `x:/`.
//! - All other casing is preserved (case-sensitive Linux paths must round-trip
//!   unchanged).

use std::borrow::Cow;
use std::path::{Path, PathBuf};

// ── Windows extended-length ("verbatim") path simplification ────────────────
//
// `Path::canonicalize()` on Windows returns an extended-length path:
// `\\?\D:\dir\file.js`, or `\\?\UNC\server\share\file.js` for a network path.
// That form is correct for the Win32 file APIs and is load-bearing wherever a
// path may exceed `MAX_PATH`, but it is NOT a portable process argument: a
// child that parses the argument with its own path logic (node's
// `resolveMainPath`, `cmd.exe`, tsc) does not understand the `\\?\` prefix. Node
// degenerates `\\?\D:\...` to `lstat('D:')` and dies with `EISDIR` before the
// script ever runs.
//
// So the canonical path stays canonical for filesystem work and identity, and
// the verbatim prefix is stripped at the EXEC boundary — the one place the value
// stops being a path we open and becomes a string another program parses.

/// The Windows extended-length prefix, disk form: `\\?\D:\…`.
const VERBATIM_PREFIX: &str = r"\\?\";

/// The Windows extended-length prefix, UNC form: `\\?\UNC\server\share\…`.
const VERBATIM_UNC_PREFIX: &str = r"\\?\UNC\";

/// Windows `MAX_PATH`, measured in **UTF-16 code units** — Win32's own unit, and
/// the limit counts the terminating NUL, so a usable path must be strictly
/// shorter. NOT a UTF-8 byte count: `é` is one code unit but two bytes, so a
/// byte-length test would refuse ~130 accented characters as "too long" and hand
/// the child the untouched verbatim path — reproducing the very failure this
/// module prevents. (Per-component length is volume-specific and a separate
/// concern; it is deliberately not checked here.)
const WIN32_MAX_PATH: usize = 260;

/// Length of `s` in UTF-16 code units — what Win32 measures a path in.
fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Whether a path component names a reserved Windows device, comparing the
/// component's STEM (everything before the FIRST `.`) case-insensitively:
/// `NUL`, `nul.txt`, and `Nul.tar.gz` are all reserved. Covers `CON`, `PRN`,
/// `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, the superscript-digit forms
/// `COM¹`/`COM²`/`COM³` and `LPT¹`/`LPT²`/`LPT³` that Windows reserves
/// alongside them, and the console devices `CONIN$`/`CONOUT$` (reserved WITH the
/// `$` only — bare `CONIN`/`CONOUT` are ordinary names).
///
/// Takes bytes so the exec-boundary simplifier here and the tracked-path
/// portability guard (which enumerates raw `git ls-files -z` output) share ONE
/// classification. They previously kept two hand-written lists, and the drift
/// between them is exactly how the superscript forms went missing.
#[must_use]
pub fn is_reserved_device_name(component: &[u8]) -> bool {
    let stem = component.split(|&b| b == b'.').next().unwrap_or(component);
    let upper: Vec<u8> = stem.iter().map(|b| b.to_ascii_uppercase()).collect();
    match upper.as_slice() {
        b"CON" | b"PRN" | b"AUX" | b"NUL" | b"CONIN$" | b"CONOUT$" => true,
        [b'C', b'O', b'M', d] | [b'L', b'P', b'T', d] => (b'1'..=b'9').contains(d),
        // `¹` U+00B9, `²` U+00B2, `³` U+00B3 — two UTF-8 bytes each, both
        // outside ASCII so the uppercase fold above leaves them untouched.
        [b'C', b'O', b'M', 0xC2, d] | [b'L', b'P', b'T', 0xC2, d] => {
            matches!(d, 0xB9 | 0xB2 | 0xB3)
        }
        _ => false,
    }
}

/// Why a verbatim path has no equivalent normal Win32 spelling, and so must be
/// left verbatim rather than rewritten to a different target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerbatimRefusal {
    /// `\\?\Volume{…}` and other device-namespace names: no drive/UNC spelling.
    DeviceNamespace,
    /// A bare `\\?\X:` with no separator. `X:` alone is DRIVE-RELATIVE under
    /// Win32 — it resolves against that drive's current directory.
    DriveRelative,
    /// `\\?\UNC\server` with no share component: `\\server` names no share.
    IncompleteUnc,
    /// Longer than [`WIN32_MAX_PATH`]; only the verbatim form can name it.
    TooLong {
        /// Length of the would-be simplified path in UTF-16 code units.
        utf16_units: usize,
    },
    /// Under the verbatim prefix `/` is an ordinary filename character; under a
    /// normal Win32 path it is a separator, so simplifying changes the meaning.
    LiteralForwardSlash,
    /// One component would not survive Win32 normalization intact.
    Component {
        /// The offending component, as authored.
        component: String,
        /// What Win32 would do to it.
        reason: ComponentRefusal,
    },
}

/// What Win32 path normalization would do to a single component that makes it
/// unsafe to drop the verbatim prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentRefusal {
    /// An interior `\\`: Win32 collapses the empty component.
    Empty,
    /// `.` or `..`: Win32 resolves them, verbatim keeps them literal.
    DotSegment,
    /// A trailing `.` or space: Win32 trims it, naming a different file.
    TrailingDotOrSpace,
    /// A character Win32 forbids in a path component.
    ForbiddenCharacter,
    /// A reserved device name — the path would resolve to the device.
    ReservedDeviceName,
}

impl std::fmt::Display for ComponentRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "is empty (Windows collapses it)",
            Self::DotSegment => {
                "is a `.`/`..` segment (Windows resolves it, the verbatim form does not)"
            }
            Self::TrailingDotOrSpace => {
                "ends with a dot or space (Windows trims it, naming a different file)"
            }
            Self::ForbiddenCharacter => "contains a character Windows forbids in a path",
            Self::ReservedDeviceName => "is a reserved Windows device name",
        })
    }
}

impl std::fmt::Display for VerbatimRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceNamespace => f.write_str(
                "it names the Windows device namespace, which has no drive-letter or UNC spelling",
            ),
            Self::DriveRelative => f.write_str(
                "it is a bare drive with no separator, which Windows resolves relative to that \
                 drive's current directory rather than its root",
            ),
            Self::IncompleteUnc => {
                f.write_str("its UNC body names a server but no share, so it names no directory")
            }
            Self::TooLong { utf16_units } => write!(
                f,
                "it is {utf16_units} characters long, over the Windows MAX_PATH limit of \
                 {WIN32_MAX_PATH}"
            ),
            Self::LiteralForwardSlash => f.write_str(
                "it contains a `/`, which is an ordinary filename character in the extended-length \
                 form but a separator in a normal Windows path",
            ),
            Self::Component { component, reason } => {
                write!(f, "its component `{component}` {reason}")
            }
        }
    }
}

/// The one decision [`simplify_verbatim_path_str`] and [`verbatim_refusal`]
/// share, so the two can never disagree about a path.
enum VerbatimClass {
    /// Not an extended-length path — nothing to do.
    NotVerbatim,
    /// The equivalent normal Win32 path.
    Simplified(String),
    /// Verbatim, with no safe Win32 equivalent.
    Refused(VerbatimRefusal),
}

/// Strip the Windows extended-length (`\\?\`) prefix from a path that is about
/// to become a **child-process argument or environment value**.
///
/// - `\\?\D:\dir\file` → `D:\dir\file` (verbatim-disk)
/// - `\\?\UNC\server\share\file` → `\\server\share\file` (verbatim-UNC)
/// - anything else — an already-simple Win32 path, a POSIX path, a verbatim path
///   with no Win32 equivalent — is returned UNCHANGED.
///
/// A verbatim path whose Win32 form would denote a DIFFERENT file (or no file)
/// is deliberately left verbatim rather than corrupted; [`VerbatimRefusal`]
/// enumerates every such case, and [`verbatim_refusal`] reports which one
/// applied so a caller can fail with an actionable message instead of handing a
/// child a path it cannot open. Handing the child a *wrong* path is worse than
/// handing it the verbatim one — it fails loudly instead of reading someone
/// else's file.
///
/// **This transform is host-independent by construction** and is NOT built on
/// [`std::path::Prefix`]. `Prefix` is produced by the host's path parser, so on
/// macOS/Linux `Path::new(r"\\?\D:\x")` has no prefix at all and a `Prefix`-based
/// implementation would compile to an unobservable no-op that no non-Windows gate
/// run could discriminate. Deciding on the literal prefix — exactly as
/// [`canonicalize_path`] already lowercases a Windows drive letter on every host —
/// keeps the rule testable everywhere it must hold.
///
/// Identity/equality semantics are untouched: this is not a canonical-ID
/// normalizer ([`canonicalize_path`] is), it changes no separators and no casing.
pub fn simplify_verbatim_path_str(raw: &str) -> Cow<'_, str> {
    match classify_verbatim(raw) {
        VerbatimClass::Simplified(simplified) => Cow::Owned(simplified),
        VerbatimClass::NotVerbatim | VerbatimClass::Refused(_) => Cow::Borrowed(raw),
    }
}

/// `Some(reason)` exactly when `raw` is an extended-length path that
/// [`simplify_verbatim_path_str`] REFUSES to simplify; `None` when it is not
/// verbatim at all, or when simplification succeeds.
///
/// A caller that must hand the path to a program which cannot parse the `\\?\`
/// prefix should fail on `Some` with the rendered reason rather than launching a
/// command that is known to die.
#[must_use]
pub fn verbatim_refusal(raw: &str) -> Option<VerbatimRefusal> {
    match classify_verbatim(raw) {
        VerbatimClass::Refused(reason) => Some(reason),
        VerbatimClass::NotVerbatim | VerbatimClass::Simplified(_) => None,
    }
}

/// [`simplify_verbatim_path_str`] over a [`Path`]. Borrows when nothing changes,
/// which is every POSIX path and every already-simple Windows path.
///
/// A path whose bytes are not valid UTF-8 (a Windows unpaired surrogate) is
/// returned unchanged — the safe direction: unsimplified still opens the right
/// file, a mangled path does not.
pub fn simplify_verbatim_path(path: &Path) -> Cow<'_, Path> {
    match path.to_str() {
        Some(raw) => match simplify_verbatim_path_str(raw) {
            Cow::Borrowed(_) => Cow::Borrowed(path),
            Cow::Owned(simplified) => Cow::Owned(PathBuf::from(simplified)),
        },
        None => Cow::Borrowed(path),
    }
}

fn classify_verbatim(raw: &str) -> VerbatimClass {
    // UNC form FIRST — `\\?\UNC\` also matches the shorter `\\?\` prefix.
    if let Some(body) = raw.strip_prefix(VERBATIM_UNC_PREFIX) {
        // `\\server\share\…`. Server AND share must both be present: `\\server`
        // alone names no directory, so it is not a usable Win32 path.
        let mut parts = body.split('\\');
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        if server.is_empty() || share.is_empty() {
            return VerbatimClass::Refused(VerbatimRefusal::IncompleteUnc);
        }
        return finish(format!(r"\\{body}"), body);
    }

    let Some(body) = raw.strip_prefix(VERBATIM_PREFIX) else {
        return VerbatimClass::NotVerbatim;
    };

    // Only the DISK form has a Win32 equivalent. `\\?\Volume{…}` and other
    // device-namespace names do not.
    let bytes = body.as_bytes();
    if bytes.len() < 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return VerbatimClass::Refused(VerbatimRefusal::DeviceNamespace);
    }
    match bytes.get(2) {
        // `\\?\D:` — `D:` alone is drive-RELATIVE, not the drive root.
        None => VerbatimClass::Refused(VerbatimRefusal::DriveRelative),
        Some(b'\\') => finish(body.to_string(), &body[2..]),
        Some(_) => VerbatimClass::Refused(VerbatimRefusal::DeviceNamespace),
    }
}

/// Validate `tail` (the part below the root) and length-check the assembled
/// `simplified` path.
fn finish(simplified: String, tail: &str) -> VerbatimClass {
    if let Some(reason) = tail_refusal(tail) {
        return VerbatimClass::Refused(reason);
    }
    let units = utf16_len(&simplified);
    if units >= WIN32_MAX_PATH {
        return VerbatimClass::Refused(VerbatimRefusal::TooLong { utf16_units: units });
    }
    VerbatimClass::Simplified(simplified)
}

/// The first reason (if any) that a path tail would not survive Win32
/// normalization intact. `tail` is backslash-separated, with an optional leading
/// and/or trailing separator.
fn tail_refusal(tail: &str) -> Option<VerbatimRefusal> {
    if tail.contains('/') {
        return Some(VerbatimRefusal::LiteralForwardSlash);
    }
    let trimmed = tail.strip_prefix('\\').unwrap_or(tail);
    let trimmed = trimmed.strip_suffix('\\').unwrap_or(trimmed);
    if trimmed.is_empty() {
        // A bare root (`D:\`, or `\\server\share`) — nothing below it to check.
        return None;
    }
    trimmed.split('\\').find_map(|component| {
        component_refusal(component).map(|reason| VerbatimRefusal::Component {
            component: component.to_string(),
            reason,
        })
    })
}

/// Why ONE path component would not mean the same thing without the verbatim
/// prefix, or `None` when it survives Win32 normalization unchanged.
fn component_refusal(component: &str) -> Option<ComponentRefusal> {
    if component.is_empty() {
        return Some(ComponentRefusal::Empty);
    }
    if component == "." || component == ".." {
        return Some(ComponentRefusal::DotSegment);
    }
    if component.ends_with('.') || component.ends_with(' ') {
        return Some(ComponentRefusal::TrailingDotOrSpace);
    }
    if component
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || (c as u32) < 0x20)
    {
        return Some(ComponentRefusal::ForbiddenCharacter);
    }
    if is_reserved_device_name(component.as_bytes()) {
        return Some(ComponentRefusal::ReservedDeviceName);
    }
    None
}

/// Whether `s` ends with a trailing `/` that should be stripped — i.e. it is
/// not the filesystem root `/` and not a Windows drive-root `x:/`.
fn ends_with_strippable_slash(s: &str) -> bool {
    if !s.ends_with('/') || s == "/" {
        return false;
    }
    let b = s.as_bytes();
    // drive-root `x:/`
    !(b.len() == 3 && b[1] == b':' && b[2] == b'/')
}

/// The single canonical normalization, allocating only when a transform is
/// actually required.
///
/// Returns [`Cow::Borrowed`] for the common macOS/Linux case where the input is
/// already canonical (no backslash, no `//?/` prefix, drive already lowercase or
/// absent, no strippable trailing slash); otherwise [`Cow::Owned`].
pub fn canonicalize_path_cow(raw: &str) -> Cow<'_, str> {
    let bytes = raw.as_bytes();
    let has_backslash = raw.contains('\\');
    let has_ext_prefix = raw.starts_with("//?/");
    let drive_needs_lower = bytes.len() >= 2
        && bytes[1] == b':'
        && bytes[0].is_ascii_alphabetic()
        && bytes[0].is_ascii_uppercase();
    let trailing_strip = ends_with_strippable_slash(raw);

    if !has_backslash && !has_ext_prefix && !drive_needs_lower && !trailing_strip {
        return Cow::Borrowed(raw);
    }
    Cow::Owned(canonicalize_owned(raw))
}

fn canonicalize_owned(raw: &str) -> String {
    // Step 1: backslash → forward slash (only allocate if needed).
    let normalized = if raw.contains('\\') {
        raw.replace('\\', "/")
    } else {
        raw.to_string()
    };

    // Step 2: strip Windows extended-length prefix — `//?/UNC/` BEFORE `//?/`.
    let normalized = if let Some(rest) = normalized.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        rest.to_string()
    } else {
        normalized
    };

    // Step 3: lowercase the Windows drive letter only (keep the colon), even on
    // non-Windows hosts — canonical IDs may carry Windows paths. NEVER `/c/`.
    let normalized = {
        let b = normalized.as_bytes();
        if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() && b[0].is_ascii_uppercase() {
            let mut s = String::with_capacity(normalized.len());
            s.push((b[0] as char).to_ascii_lowercase());
            s.push_str(&normalized[1..]);
            s
        } else {
            normalized
        }
    };

    // Step 4: strip ALL trailing slashes except the roots `/` and `x:/`.
    // Looped so the result is idempotent — `/a//` and `/a///` both canonicalize
    // to `/a`, otherwise one call would leave a residual `/a/` and a second call
    // would change it again (two canonical IDs for the same directory).
    if ends_with_strippable_slash(&normalized) {
        let mut s = normalized;
        while ends_with_strippable_slash(&s) {
            s.pop();
        }
        s
    } else {
        normalized
    }
}

/// The single canonical normalization, owned. Equal to
/// `canonicalize_path_cow(raw).into_owned()`.
pub fn canonicalize_path(raw: &str) -> String {
    canonicalize_path_cow(raw).into_owned()
}

/// Whether the host's default filesystem folds path case — the SINGLE
/// filesystem-case-identity policy for the whole workspace.
///
/// `true` on Windows (NTFS) and macOS (default case-insensitive APFS), `false` on
/// Linux. Every consumer that decides whether two paths denote the SAME file —
/// configured-project / `root_files` membership in the tsgo `--api` adapter, the
/// carrier-publish store directory fold — routes through this one predicate, so the
/// case policy can never diverge per call site. (The rare case-sensitive APFS volume
/// only over-separates, never collides, which is the safe direction; a precise
/// runtime probe is out of scope.)
#[must_use]
pub const fn fs_is_case_insensitive() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}

/// Filesystem-identity equality of two paths under THIS host's case policy
/// ([`fs_is_case_insensitive`]). Slash-normalized, then compared case-insensitively
/// on a case-insensitive filesystem (Windows / default macOS) and exactly on a
/// case-sensitive one (Linux).
///
/// This is the membership-comparison primitive: two engine-reported paths that fold
/// to the same file must compare equal so a carrier is never dropped from its
/// configured project, while two genuinely distinct case-sensitive files never
/// conflate.
#[must_use]
pub fn fs_paths_equal(a: &str, b: &str) -> bool {
    fs_paths_equal_under(a, b, fs_is_case_insensitive())
}

/// The pure FS-identity comparison core, parameterized by the case-sensitivity bit
/// so it is host-independent (and unit-testable on every platform). Applies the ONE
/// shared normalization ([`canonicalize_path_cow`]), then folds ASCII case iff
/// `case_insensitive`.
///
/// Normalizing through [`canonicalize_path_cow`] rather than an ad-hoc slash replace
/// is what keeps this predicate in lockstep with [`InjectedPathKey`], which derives
/// its key the same way. A partial normalization here (slash-only) made the two
/// disagree on a case-SENSITIVE host for every input differing in drive case,
/// extended-length prefix, or trailing slash: the unconditional drive fold lives in
/// `canonicalize_path`, so `C:\ws\A.ts` and `c:/ws/A.ts` keyed equal while this
/// predicate called them distinct. The case-insensitive branch masked it, which is
/// why only Linux saw it.
fn fs_paths_equal_under(a: &str, b: &str, case_insensitive: bool) -> bool {
    let a = canonicalize_path_cow(a);
    let b = canonicalize_path_cow(b);
    if case_insensitive {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// A filesystem-identity key for one path under THIS host's case policy
/// ([`fs_is_case_insensitive`]) — the SAME identity notion [`fs_paths_equal`]
/// compares with, but reified as a `Hash`/`Eq` key so a set of paths can be
/// membership-tested in one lookup instead of an O(n) scan.
///
/// Construction ([`InjectedPathKey::new`]) canonicalizes the raw path
/// ([`canonicalize_path`] — slash-normalized, drive-lowercased, extended-prefix
/// stripped, trailing-slash stripped) and THEN, iff the host filesystem folds
/// case, ASCII-lowercases the whole result. The fold matches
/// [`fs_paths_equal`]'s case-insensitive branch exactly, so two paths that
/// [`fs_paths_equal`] would call the same file produce the same key (and two it
/// would call distinct produce distinct keys). [`canonicalize_path`] alone folds
/// only the drive letter, which is insufficient on a case-insensitive FS where
/// two paths differing in NON-drive case denote the same file — the extra fold
/// closes that gap.
///
/// This is the workspace path-EQUIVALENCE policy ([`fs_paths_equal`]) reified as a
/// key, NOT an OS file-id (no inode / device probe): a FILESYSTEM concern routed
/// through the shared path primitive, NOT a type-text heuristic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InjectedPathKey(String);

impl InjectedPathKey {
    /// Build the filesystem-identity key for `raw` under this host's case policy.
    pub fn new(raw: &str) -> Self {
        Self(injected_key_under(raw, fs_is_case_insensitive()))
    }

    /// The inner normalized key string (for diagnostics / debugging).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The pure key-derivation core, parameterized by the case-sensitivity bit so it
/// runs and discriminates on EVERY host (mirrors [`fs_paths_equal_under`]):
/// canonicalize, then ASCII-lowercase the whole path iff `case_insensitive`.
fn injected_key_under(raw: &str, case_insensitive: bool) -> String {
    let canon = canonicalize_path(raw);
    if case_insensitive {
        canon.to_ascii_lowercase()
    } else {
        canon
    }
}

/// A normalized filesystem path.
///
/// Invariants:
/// - Forward slashes only (no backslashes).
/// - Windows drive prefixes: lowercase drive letter, colon kept, `\\?\` prefix
///   stripped.
/// - Paths without drive prefixes: no case transformation.
/// - No trailing slash (except root `/` or `x:/`).
///
/// Distinct from the workspace `NormalizedGlob`: a `CanonicalPath` never
/// contains wildcard characters (`*`, `?`, `[`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CanonicalPath(String);

impl CanonicalPath {
    /// Create a new canonical path from a raw string.
    pub fn new(raw: &str) -> Self {
        Self(canonicalize_path(raw))
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume and return the inner String.
    pub fn into_string(self) -> String {
        self.0
    }

    /// Check if this path starts with the given prefix at a directory boundary.
    ///
    /// Returns `true` if `self` starts with `prefix` AND the character
    /// immediately after the prefix (if any) is `/`. This prevents
    /// `c:/project-extra` from matching prefix `c:/project`.
    pub fn starts_with_dir(&self, prefix: &CanonicalPath) -> bool {
        let s = self.as_str();
        let p = prefix.as_str();
        s.starts_with(p)
            && (s.len() == p.len() || p.ends_with('/') || s.as_bytes().get(p.len()) == Some(&b'/'))
    }
}

impl std::fmt::Display for CanonicalPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for CanonicalPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CanonicalPath {
    fn from(raw: &str) -> Self {
        Self::new(raw)
    }
}

impl From<String> for CanonicalPath {
    fn from(raw: String) -> Self {
        Self::new(&raw)
    }
}

/// Directory-boundary containment test: is `path` equal to or under `root`?
///
/// Both sides are canonicalized first (cheap borrow when already canonical).
/// Case-PRESERVING — a case mismatch is not contained. Rejects sibling-prefix
/// matches (`/a/project-extra` is NOT under `/a/project`).
pub fn is_under_dir(path: &str, root: &str) -> bool {
    let p = canonicalize_path_cow(path);
    let r = canonicalize_path_cow(root);
    let (p, r) = (p.as_ref(), r.as_ref());
    p.starts_with(r)
        && (p.len() == r.len()
            // When the root itself ends in `/` (the canonical roots `/` and
            // `x:/`), the prefix boundary IS the slash — the byte after the
            // prefix is a real path char, not another `/`.
            || r.ends_with('/')
            || p.as_bytes()[r.len()] == b'/')
}

/// Return the LONGEST `root` in `roots` that contains `file`, in CANONICAL form
/// (directory-boundary containment); falls back to `workspace_root` when none
/// match.
///
/// Correctness does not depend on caller sort order — the longest match is
/// computed here. The result is always canonical: the winning root (or the
/// fallback) is returned through [`canonicalize_path_cow`], so a caller that
/// stored a raw extended-prefix root (`//?/C:/repo`) still receives the
/// canonical `c:/repo` and never leaks the raw form back out (e.g. as a
/// `projectRootPath`). The `Cow` borrows when the winner is already canonical
/// (the common case, since callers canonicalize at push time) and only
/// allocates when a transform is actually needed.
pub fn longest_project_root<'a>(
    file: &str,
    roots: &'a [String],
    workspace_root: &'a str,
) -> Cow<'a, str> {
    // Canonicalize `file` once (FIX 2b) rather than per-iteration.
    let p = canonicalize_path_cow(file);
    let p = p.as_ref();

    // Track the best match by CANONICAL length — a raw `//?/` extended prefix
    // inflates `root.len()` and could otherwise beat a genuinely deeper
    // canonical root. The winning raw root is re-canonicalized for the return.
    let mut best: Option<(&'a str, usize)> = None;
    for root in roots {
        let r = canonicalize_path_cow(root);
        let r = r.as_ref();
        let contained = p.starts_with(r)
            && (p.len() == r.len() || r.ends_with('/') || p.as_bytes()[r.len()] == b'/');
        if contained {
            let canon_len = r.len();
            match best {
                Some((_, best_len)) if best_len >= canon_len => {}
                _ => best = Some((root.as_str(), canon_len)),
            }
        }
    }
    match best {
        Some((root, _)) => canonicalize_path_cow(root),
        None => canonicalize_path_cow(workspace_root),
    }
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;
