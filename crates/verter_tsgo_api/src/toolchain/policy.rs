//! Supported-tsgo-version policy: the ONE TypeScript pin and the release line
//! it admits.
//!
//! tsgo is the TypeScript 7 native compiler and follows TypeScript SemVer
//! (`major.minor.patch[-prerelease]`). Verter targets exactly one TypeScript
//! build and supports the release line that build belongs to:
//!
//! - [`TYPESCRIPT_PIN`] — the exact TypeScript build Verter targets. It is the
//!   workspace `typescript` devDependency, the oracle harness pin, the
//!   reference build of the `--api` wire pin, and the offline bundled sidecar.
//!   It is the ONE Rust spelling of the version; every other spelling in the
//!   repository (package manifests, fixtures, the oracle toolchain record) is
//!   checked against the workspace `package.json` by
//!   `scripts/typescript-pin.mjs --check`, and `--set <version>` rewrites them
//!   all, so moving the pin is one command plus the oracle regeneration.
//! - [`SUPPORTED_TSGO_RANGE_LABEL`] — the pin's release line, from the pin up
//!   to the next minor. A candidate qualifies iff it carries no build metadata,
//!   its core is on the line, and it is either a stable release at or after the
//!   pin, or a `beta`/`rc` prerelease of the pin's own core at or after the pin.
//!   Nightlies (`-dev.*`), alphas and other prerelease channels, earlier
//!   releases and other minors/majors are refused. Once the pin is a stable
//!   release, prereleases stop qualifying by construction (they order before
//!   it).
//!
//! "Latest supported" always means the greatest qualifying version — never a
//! registry `latest` tag.

use std::fmt;
use std::sync::LazyLock;

/// The pin as a literal, so labels can be assembled at compile time (here and
/// in dependent crates via `verter_tsgo_api::typescript_pin!()`). Change the pin
/// HERE (or via `scripts/typescript-pin.mjs --set`).
#[macro_export]
macro_rules! typescript_pin {
    () => {
        "7.1.0-beta"
    };
}

/// The release line the pin belongs to (`major.minor`).
macro_rules! typescript_line {
    () => {
        "7.1"
    };
}

/// The exclusive upper bound of the line: the next minor.
#[macro_export]
macro_rules! typescript_line_ceiling {
    () => {
        "7.2.0"
    };
}

/// The exact TypeScript build Verter targets (see the module docs).
pub const TYPESCRIPT_PIN: &str = typescript_pin!();

/// Human-readable supported window, embedded in every policy diagnostic.
pub const SUPPORTED_TSGO_RANGE_LABEL: &str =
    concat!(">=", typescript_pin!(), ", <", typescript_line_ceiling!());

/// The policy identity used in the on-disk temp-cache layout
/// (`…/<target-triple>/<policy-id>/<version>/…`), so a line change can never
/// collide with cache entries written under a different support window.
pub const SUPPORTED_POLICY_ID: &str = concat!("ts", typescript_line!());

static PINNED_VERSION: LazyLock<TsgoVersion> =
    LazyLock::new(|| TsgoVersion::parse(TYPESCRIPT_PIN).expect("TYPESCRIPT_PIN is strict SemVer"));

static LINE_CEILING: LazyLock<TsgoVersion> = LazyLock::new(|| {
    TsgoVersion::parse(typescript_line_ceiling!()).expect("the line ceiling is strict SemVer")
});

/// The parsed [`TYPESCRIPT_PIN`]: the inclusive floor of the supported window
/// and the version the offline bundled sidecar ships.
pub fn pinned_version() -> &'static TsgoVersion {
    &PINNED_VERSION
}

/// The EXCLUSIVE upper bound of the supported window (the next minor): any
/// later minor or major is refused until Verter explicitly moves the pin.
pub fn supported_range_ceiling() -> &'static TsgoVersion {
    &LINE_CEILING
}

/// A strictly-parsed SemVer version. tsgo versions are TypeScript SemVer:
/// `major.minor.patch` with an optional `-prerelease` and optional `+build`.
///
/// Equality, hash, and ordering all follow SemVer PRECEDENCE (§11): build
/// metadata is ignored by ALL THREE (hand-rolled `PartialEq`/`Eq`/`Hash` below,
/// so `a == b` iff `a.cmp(b) == Ordering::Equal` — derived `Eq` would include
/// build metadata and violate the `Eq`/`Ord` consistency contract).
#[derive(Debug, Clone)]
pub struct TsgoVersion {
    /// SemVer major.
    pub major: u64,
    /// SemVer minor.
    pub minor: u64,
    /// SemVer patch.
    pub patch: u64,
    /// The prerelease suffix without the leading `-` (`beta`,
    /// `rc.1`), or `None` for a stable release.
    pub prerelease: Option<String>,
    /// The build-metadata suffix without the leading `+`, or `None`.
    pub build: Option<String>,
}

impl TsgoVersion {
    /// Construct a stable (no prerelease, no build metadata) version.
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            prerelease: None,
            build: None,
        }
    }

    /// Strictly parse a SemVer string. Anything not conforming to SemVer 2.0.0
    /// (numeric core without leading zeros, dot-separated prerelease/build
    /// identifiers over `[0-9A-Za-z-]`) is a hard [`VersionParseError`].
    pub fn parse(input: &str) -> Result<Self, VersionParseError> {
        let malformed = |reason: &str| VersionParseError {
            input: input.to_string(),
            reason: reason.to_string(),
        };

        if input.is_empty() || input != input.trim() {
            return Err(malformed("empty or whitespace-padded version"));
        }

        // Build metadata: exactly one optional `+` suffix.
        let (core_and_pre, build) = match input.split('+').collect::<Vec<_>>().as_slice() {
            [head] => (*head, None),
            [head, build] => {
                validate_identifiers(build, "build metadata", false).map_err(|r| malformed(&r))?;
                (*head, Some(build.to_string()))
            }
            _ => return Err(malformed("more than one `+` build-metadata separator")),
        };

        // Prerelease: one optional `-` suffix (identifiers may themselves
        // contain `-`, so split at the FIRST one).
        let (core, prerelease) = match core_and_pre.split_once('-') {
            Some((core, pre)) => {
                validate_identifiers(pre, "prerelease", true).map_err(|r| malformed(&r))?;
                (core, Some(pre.to_string()))
            }
            None => (core_and_pre, None),
        };

        let components: Vec<&str> = core.split('.').collect();
        let [major, minor, patch] = components.as_slice() else {
            return Err(malformed(
                "the version core must be exactly `major.minor.patch`",
            ));
        };
        let parse_component = |component: &str| -> Result<u64, VersionParseError> {
            if component.is_empty() || !component.bytes().all(|b| b.is_ascii_digit()) {
                return Err(malformed("core components must be ASCII digits"));
            }
            if component.len() > 1 && component.starts_with('0') {
                return Err(malformed("core components must not have leading zeros"));
            }
            component
                .parse()
                .map_err(|_| malformed("core component overflows u64"))
        };

        Ok(Self {
            major: parse_component(major)?,
            minor: parse_component(minor)?,
            patch: parse_component(patch)?,
            prerelease,
            build,
        })
    }

    /// Whether this is a stable release (no prerelease suffix).
    pub fn is_stable(&self) -> bool {
        self.prerelease.is_none()
    }

    /// The numeric version core, for range comparison.
    pub fn core(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

impl PartialEq for TsgoVersion {
    /// SemVer precedence equality (§11): build metadata is IGNORED, matching
    /// [`Ord`] exactly (`a == b` iff `a.cmp(b) == Ordering::Equal`).
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major
            && self.minor == other.minor
            && self.patch == other.patch
            && self.prerelease == other.prerelease
    }
}

impl Eq for TsgoVersion {}

impl std::hash::Hash for TsgoVersion {
    /// Hashes exactly the fields [`PartialEq`] compares (never build metadata),
    /// so equal values hash equally.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.major.hash(state);
        self.minor.hash(state);
        self.patch.hash(state);
        self.prerelease.hash(state);
    }
}

/// Validate a dot-separated SemVer identifier list (prerelease or build). When
/// `prerelease` is set, SemVer §9 applies: numeric identifiers MUST NOT carry
/// leading zeroes (build metadata, §10, MAY).
fn validate_identifiers(list: &str, what: &str, prerelease: bool) -> Result<(), String> {
    if list.is_empty() {
        return Err(format!("empty {what}"));
    }
    for identifier in list.split('.') {
        if identifier.is_empty() {
            return Err(format!("empty identifier in {what}"));
        }
        if !identifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(format!(
                "illegal character in {what} identifier `{identifier}`"
            ));
        }
        if prerelease
            && identifier.len() > 1
            && identifier.bytes().all(|b| b.is_ascii_digit())
            && identifier.starts_with('0')
        {
            return Err(format!(
                "numeric {what} identifier `{identifier}` must not include leading zeroes"
            ));
        }
    }
    Ok(())
}

impl fmt::Display for TsgoVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.prerelease {
            write!(f, "-{pre}")?;
        }
        if let Some(build) = &self.build {
            write!(f, "+{build}")?;
        }
        Ok(())
    }
}

impl PartialOrd for TsgoVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TsgoVersion {
    /// SemVer §11 ordering: numeric core first; a prerelease orders BEFORE its
    /// release; prereleases compare IDENTIFIER-BY-IDENTIFIER (numeric
    /// identifiers numerically, alphanumeric lexically, numeric < alphanumeric,
    /// a shorter set before a longer prefix-equal one); build metadata is
    /// ignored for precedence.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match self.core().cmp(&other.core()) {
            Ordering::Equal => match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => compare_prerelease(a, b),
            },
            ordering => ordering,
        }
    }
}

/// SemVer §11.4 prerelease precedence: identifier-by-identifier left to right;
/// when every compared identifier is equal, the SHORTER identifier set orders
/// before the longer one.
fn compare_prerelease(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut ai = a.split('.');
    let mut bi = b.split('.');
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let ordering = compare_prerelease_identifier(x, y);
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
        }
    }
}

/// One prerelease identifier pair: numeric identifiers compare NUMERICALLY
/// (strict parse forbids leading zeroes, so length-then-lexical IS the numeric
/// order, with no integer overflow), and numeric identifiers order BEFORE
/// alphanumeric ones.
fn compare_prerelease_identifier(x: &str, y: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let x_numeric = x.bytes().all(|b| b.is_ascii_digit());
    let y_numeric = y.bytes().all(|b| b.is_ascii_digit());
    match (x_numeric, y_numeric) {
        (true, true) => x.len().cmp(&y.len()).then_with(|| x.cmp(y)),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => x.cmp(y),
    }
}

/// A strict-SemVer parse failure, naming the offending input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionParseError {
    /// The input that failed to parse.
    pub input: String,
    /// Why it is not strict SemVer.
    pub reason: String,
}

impl fmt::Display for VersionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a strict SemVer version ({}); tsgo versions look like `{TYPESCRIPT_PIN}`",
            self.input, self.reason
        )
    }
}

impl std::error::Error for VersionParseError {}

/// The prerelease channel of a version's prerelease suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrereleaseChannel {
    /// `-rc` / `-rc.N` release candidates.
    ReleaseCandidate,
    /// `-beta` builds.
    Beta,
    /// `-alpha` builds.
    Alpha,
    /// `-dev.<date>.<n>` nightlies.
    Nightly,
    /// Any other prerelease suffix.
    Other,
}

impl PrereleaseChannel {
    /// The channel's name in diagnostics.
    fn name(self) -> &'static str {
        match self {
            Self::ReleaseCandidate => "release-candidate",
            Self::Beta => "beta",
            Self::Alpha => "alpha",
            Self::Nightly => "nightly",
            Self::Other => "prerelease",
        }
    }
}

/// Why a candidate version does not satisfy the support policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyRejection {
    /// The version string is not strict SemVer.
    Malformed {
        /// The offending input.
        input: String,
        /// Why it failed to parse.
        reason: String,
    },
    /// A release that orders before the pin ([`pinned_version`]).
    BelowSupportedFloor {
        /// The offending version.
        version: TsgoVersion,
    },
    /// A version at or past [`supported_range_ceiling`].
    OutOfSupportedRange {
        /// The offending version.
        version: TsgoVersion,
    },
    /// A prerelease the line does not admit: a nightly, an alpha, another
    /// channel, or a `beta`/`rc` of a core other than the pin's.
    Prerelease {
        /// The offending version.
        version: TsgoVersion,
        /// Which prerelease channel it belongs to.
        channel: PrereleaseChannel,
    },
    /// A build carrying `+metadata` — an unverified local build.
    BuildMetadata {
        /// The offending version.
        version: TsgoVersion,
    },
}

impl fmt::Display for PolicyRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { input, reason } => write!(
                f,
                "`{input}` is not a strict SemVer version ({reason}); Verter supports tsgo \
                 (TypeScript 7 native) {SUPPORTED_TSGO_RANGE_LABEL}"
            ),
            Self::BelowSupportedFloor { version } => write!(
                f,
                "tsgo {version} is below the supported floor; Verter supports tsgo \
                 {SUPPORTED_TSGO_RANGE_LABEL} — install typescript@{TYPESCRIPT_PIN}"
            ),
            Self::OutOfSupportedRange { version } => write!(
                f,
                "tsgo {version} is outside the supported window; Verter supports tsgo \
                 {SUPPORTED_TSGO_RANGE_LABEL} — a new TypeScript minor/major needs explicit \
                 Verter support, install typescript@{TYPESCRIPT_PIN} instead"
            ),
            Self::Prerelease { version, channel } => write!(
                f,
                "tsgo {version} is a {} prerelease Verter does not support; Verter supports \
                 tsgo {SUPPORTED_TSGO_RANGE_LABEL} (stable releases, and beta/rc builds of the \
                 pinned release only) — install typescript@{TYPESCRIPT_PIN}",
                channel.name()
            ),
            Self::BuildMetadata { version } => write!(
                f,
                "tsgo {version} carries build metadata (an unverified local build); Verter \
                 supports tsgo {SUPPORTED_TSGO_RANGE_LABEL} from the official packages"
            ),
        }
    }
}

impl std::error::Error for PolicyRejection {}

/// The supported-version acceptance policy. There is one policy: the pin's
/// release line (see the module docs). The type stays a value so the
/// validator, the wire gate and the provisioning walk share one instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionPolicy {
    _private: (),
}

impl VersionPolicy {
    /// The production policy: the pin's release line.
    pub const fn production() -> Self {
        Self { _private: () }
    }

    /// Check a parsed version against the support window.
    ///
    /// A candidate qualifies iff it carries no build metadata, it orders before
    /// [`supported_range_ceiling`], and it is either a stable release at or
    /// after the pin, or a `beta`/`rc` prerelease of the pin's own core at or
    /// after the pin.
    pub fn check(&self, version: &TsgoVersion) -> Result<(), PolicyRejection> {
        if version.build.is_some() {
            return Err(PolicyRejection::BuildMetadata {
                version: version.clone(),
            });
        }
        if version.core() >= supported_range_ceiling().core() {
            return Err(PolicyRejection::OutOfSupportedRange {
                version: version.clone(),
            });
        }
        let pin = pinned_version();
        if let Some(prerelease) = &version.prerelease {
            let channel = classify_prerelease(prerelease);
            let admitted = matches!(
                channel,
                PrereleaseChannel::Beta | PrereleaseChannel::ReleaseCandidate
            ) && version.core() == pin.core()
                && version >= pin;
            if !admitted {
                return Err(PolicyRejection::Prerelease {
                    version: version.clone(),
                    channel,
                });
            }
            return Ok(());
        }
        if version < pin {
            return Err(PolicyRejection::BelowSupportedFloor {
                version: version.clone(),
            });
        }
        Ok(())
    }

    /// Parse + check in one step; parse failures surface as
    /// [`PolicyRejection::Malformed`].
    pub fn check_str(&self, input: &str) -> Result<TsgoVersion, PolicyRejection> {
        let version = TsgoVersion::parse(input).map_err(|e| PolicyRejection::Malformed {
            input: e.input,
            reason: e.reason,
        })?;
        self.check(&version)?;
        Ok(version)
    }
}

/// Classify a prerelease suffix into its channel.
fn classify_prerelease(prerelease: &str) -> PrereleaseChannel {
    let first = prerelease.split('.').next().unwrap_or("");
    match first {
        "rc" => PrereleaseChannel::ReleaseCandidate,
        "beta" => PrereleaseChannel::Beta,
        "alpha" => PrereleaseChannel::Alpha,
        "dev" => PrereleaseChannel::Nightly,
        _ => PrereleaseChannel::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(input: &str) -> TsgoVersion {
        TsgoVersion::parse(input).unwrap_or_else(|e| panic!("{e}"))
    }

    /// `major.minor.patch` of the pin with `patch` replaced.
    fn on_line(patch: u64) -> String {
        let pin = pinned_version();
        format!("{}.{}.{patch}", pin.major, pin.minor)
    }

    // ── strict SemVer parse ────────────────────────────────────────────────

    #[test]
    fn parse_accepts_stable_prerelease_and_build_shapes() {
        let parsed = v("7.1.2");
        assert_eq!((parsed.major, parsed.minor, parsed.patch), (7, 1, 2));
        assert_eq!(parsed.prerelease, None);
        assert_eq!(parsed.build, None);
        assert!(parsed.is_stable());

        let parsed = v("7.1.0-rc.1");
        assert_eq!(parsed.prerelease.as_deref(), Some("rc.1"));
        assert!(!parsed.is_stable());

        let parsed = v("7.1.0-dev.20261022.1");
        assert_eq!(parsed.prerelease.as_deref(), Some("dev.20261022.1"));
        assert!(!parsed.is_stable());

        let parsed = v("7.1.2+build.5");
        assert_eq!(parsed.build.as_deref(), Some("build.5"));
        assert!(parsed.is_stable(), "build metadata is not a prerelease");
    }

    // ── DISCRIMINATING: the parser is STRICT — every malformed shape below is
    //    a hard parse error, never a silent partial parse. ───────────────────
    #[test]
    fn parse_rejects_malformed_versions() {
        for input in [
            "",
            "   ",
            "7.1",
            "7",
            "7.1.2.3",
            "7.1.x",
            "x.1.2",
            "v7.1.2",
            "7.1.2 ",
            " 7.1.2",
            "7..2",
            "7.01.2",      // leading zero in a core component is not strict SemVer
            "07.1.2",      // leading zero in major
            "7.1.2-",      // empty prerelease
            "7.1.2+",      // empty build
            "7.1.2-rc..1", // empty prerelease identifier
            "7.1.2-rc_1",  // illegal identifier char
            "garbage",
            "7.1.2+b+b", // a second `+` is not SemVer
        ] {
            assert!(
                TsgoVersion::parse(input).is_err(),
                "malformed version `{input}` must be rejected"
            );
        }
    }

    // ── DISCRIMINATING: ordering is NUMERIC, not lexicographic, and a
    //    prerelease orders BEFORE its release (SemVer §11). ─────────────────
    #[test]
    fn version_ordering_is_semver_not_lexicographic() {
        assert!(v("7.1.2") < v("7.1.10"));
        assert!(v("7.1.9") < v("7.2.0"));
        assert!(v("7.1.99") < v("8.0.0"));
        assert!(
            v("7.1.0-rc.1") < v("7.1.0"),
            "a prerelease orders before its release"
        );
        assert!(v("7.1.0-beta") < v("7.1.0-rc"), "beta orders before rc");
    }

    // ── DISCRIMINATING: prerelease precedence is IDENTIFIER-BY-IDENTIFIER
    //    (SemVer §11.4): numeric identifiers compare NUMERICALLY (rc.2 < rc.10 —
    //    a whole-string compare gets this wrong), numeric < alphanumeric, and a
    //    shorter identifier set precedes a longer one when prefix-equal. ────────
    #[test]
    fn prerelease_ordering_is_identifier_by_identifier() {
        use std::cmp::Ordering;
        assert_eq!(
            v("7.1.0-rc.2").cmp(&v("7.1.0-rc.10")),
            Ordering::Less,
            "rc.2 < rc.10 (numeric, not string, comparison)"
        );
        assert!(
            v("7.1.0-dev.20261022.9") < v("7.1.0-dev.20261022.10"),
            "nightly sequence numbers compare numerically"
        );
        assert!(v("7.1.0-rc.1") < v("7.1.0-rc.a"), "numeric < alphanumeric");
        assert!(v("7.1.0-alpha") < v("7.1.0-beta"));
        assert!(
            v("7.1.0-rc") < v("7.1.0-rc.1"),
            "a shorter identifier set precedes a longer prefix-equal one"
        );
        assert_ne!(v("7.1.0-rc.2").cmp(&v("7.1.0-rc.10")), Ordering::Equal);
    }

    // ── DISCRIMINATING: numeric PRERELEASE identifiers with leading zeroes are
    //    not strict SemVer (§9) and are rejected; build metadata MAY carry them
    //    (§10), and alphanumeric identifiers are unaffected. ──────────────────
    #[test]
    fn parse_rejects_leading_zero_numeric_prerelease_identifiers() {
        for input in ["7.1.0-rc.01", "7.1.0-01", "7.1.0-rc.1.02", "7.1.0-00"] {
            assert!(
                TsgoVersion::parse(input).is_err(),
                "leading-zero numeric prerelease `{input}` must be rejected"
            );
        }
        assert!(TsgoVersion::parse("7.1.0-rc.0").is_ok());
        assert!(TsgoVersion::parse("7.1.0-0a").is_ok());
        assert!(TsgoVersion::parse("7.1.0+build.01").is_ok());
    }

    // ── DISCRIMINATING: Eq and Ord are CONSISTENT — SemVer precedence ignores
    //    build metadata, so BOTH ignore it: equal iff Ordering::Equal, and Hash
    //    agrees with Eq. ─────────────────────────────────────────────────────
    #[test]
    fn equality_and_hash_are_consistent_with_ordering() {
        use std::cmp::Ordering;
        let a = v("7.1.2+build.1");
        let b = v("7.1.2+build.2");
        let bare = v("7.1.2");
        assert_eq!(
            a.cmp(&b),
            Ordering::Equal,
            "precedence ignores build metadata"
        );
        assert_eq!(a, b, "Eq must ignore build metadata exactly like Ord");
        assert_eq!(a, bare);
        assert!(a != v("7.1.3"));
        assert!(
            a != v("7.1.2-rc.1+build.1"),
            "prerelease still discriminates equality"
        );
        let mut set = std::collections::HashSet::new();
        set.insert(a.clone());
        assert!(set.contains(&b), "equal values must hash equally");
        assert!(set.contains(&bare));
    }

    // ── the pin and the line ───────────────────────────────────────────────

    // ── DISCRIMINATING: the three literals the module spells stay coherent —
    //    the line is the pin's `major.minor`, the ceiling is the next minor,
    //    and the label and the cache-policy id are built from them. ──────────
    #[test]
    fn pin_line_and_ceiling_are_coherent() {
        let pin = pinned_version();
        assert_eq!(pin.to_string(), TYPESCRIPT_PIN);
        assert_eq!(
            typescript_line!(),
            format!("{}.{}", pin.major, pin.minor),
            "the line is the pin's major.minor"
        );
        assert_eq!(
            *supported_range_ceiling(),
            TsgoVersion::new(pin.major, pin.minor + 1, 0),
            "the ceiling is the pin's next minor"
        );
        assert_eq!(
            SUPPORTED_TSGO_RANGE_LABEL,
            format!(">={TYPESCRIPT_PIN}, <{}", supported_range_ceiling())
        );
        assert_eq!(
            SUPPORTED_POLICY_ID,
            format!("ts{}.{}", pin.major, pin.minor)
        );
    }

    // ── DISCRIMINATING: the pin is the workspace `typescript` devDependency —
    //    the version `pnpm install` actually provisions. ───────────────────────
    #[test]
    fn pin_matches_the_workspace_typescript_dev_dependency() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../package.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let root: serde_json::Value = serde_json::from_str(&text).expect("parse package.json");
        assert_eq!(
            root["devDependencies"]["typescript"].as_str(),
            Some(TYPESCRIPT_PIN),
            "the workspace `typescript` devDependency must be TYPESCRIPT_PIN \
             (move both with `node scripts/typescript-pin.mjs --set <version>`)"
        );
    }

    // ── DISCRIMINATING: the pin (the shipped offline sidecar) ALWAYS satisfies
    //    the production policy — the fallback can never be self-refusing. ───
    #[test]
    fn pin_always_satisfies_production_policy() {
        VersionPolicy::production()
            .check(pinned_version())
            .expect("the pin must qualify under the production policy");
        assert!(pinned_version() < supported_range_ceiling());
    }

    // ── DISCRIMINATING: stable releases on the line at or after the pin
    //    qualify — the auto-accept-patch contract. ───────────────────────────
    #[test]
    fn production_accepts_stable_releases_on_the_line() {
        let policy = VersionPolicy::production();
        for patch in [0, 1, 2, 9, 13, 99] {
            let candidate = v(&on_line(patch));
            if candidate < *pinned_version() {
                continue;
            }
            assert!(
                policy.check(&candidate).is_ok(),
                "stable `{candidate}` on the line must qualify"
            );
        }
    }

    // ── DISCRIMINATING: beta/rc builds of the pin's core at or after the pin
    //    qualify; the same channels on any other core do not. ─────────────────
    #[test]
    fn production_admits_only_pinned_core_beta_and_rc() {
        let policy = VersionPolicy::production();
        let pin = pinned_version();
        let core = format!("{}.{}.{}", pin.major, pin.minor, pin.patch);
        for suffix in ["beta", "beta.2", "rc", "rc.1", "rc.10"] {
            let candidate = v(&format!("{core}-{suffix}"));
            let expected = candidate >= *pin;
            assert_eq!(
                policy.check(&candidate).is_ok(),
                expected,
                "`{candidate}`: admitted iff at or after the pin `{pin}`"
            );
        }
        let next_patch = format!("{}.{}.{}-rc", pin.major, pin.minor, pin.patch + 1);
        assert!(
            matches!(
                policy.check(&v(&next_patch)),
                Err(PolicyRejection::Prerelease {
                    channel: PrereleaseChannel::ReleaseCandidate,
                    ..
                })
            ),
            "an rc of a later patch is not the pinned build"
        );
    }

    // ── DISCRIMINATING: production refuses nightlies, alphas and unknown
    //    channels even on the pin's core, and earlier-minor prereleases. ────
    #[test]
    fn production_rejects_other_prerelease_channels() {
        let policy = VersionPolicy::production();
        let pin = pinned_version();
        let core = format!("{}.{}.{}", pin.major, pin.minor, pin.patch);
        let cases = [
            (format!("{core}-dev.20261022.1"), PrereleaseChannel::Nightly),
            (format!("{core}-alpha"), PrereleaseChannel::Alpha),
            (format!("{core}-insiders"), PrereleaseChannel::Other),
        ];
        for (candidate, channel) in cases {
            match policy.check(&v(&candidate)) {
                Err(PolicyRejection::Prerelease { channel: got, .. }) => {
                    assert_eq!(
                        got, channel,
                        "wrong channel classification for `{candidate}`"
                    );
                }
                other => panic!("`{candidate}` must be a Prerelease rejection, got {other:?}"),
            }
        }
    }

    // ── DISCRIMINATING: production refuses releases before the pin and
    //    versions past the line — with DISTINCT reasons. ─────────────────────
    #[test]
    fn production_rejects_below_floor_and_out_of_range() {
        let policy = VersionPolicy::production();
        let pin = pinned_version();
        let previous_minor = format!("{}.{}.9", pin.major, pin.minor - 1);
        for candidate in [previous_minor.as_str(), "6.9.9", "0.0.1"] {
            assert!(
                matches!(
                    policy.check(&v(candidate)),
                    Err(PolicyRejection::BelowSupportedFloor { .. })
                ),
                "`{candidate}` is below the pin"
            );
        }
        let ceiling = supported_range_ceiling().to_string();
        let past = format!("{}.{}.3", pin.major, pin.minor + 1);
        for candidate in [ceiling.as_str(), past.as_str(), "8.0.0", "9.3.1"] {
            assert!(
                matches!(
                    policy.check(&v(candidate)),
                    Err(PolicyRejection::OutOfSupportedRange { .. })
                ),
                "`{candidate}` is past the supported line"
            );
        }
    }

    // ── DISCRIMINATING: build metadata is an unverified build and is refused
    //    even on an otherwise admitted version. ─────────────────────────────
    #[test]
    fn production_rejects_build_metadata() {
        let candidate = v(&format!("{TYPESCRIPT_PIN}+build.5"));
        assert!(matches!(
            VersionPolicy::production().check(&candidate),
            Err(PolicyRejection::BuildMetadata { .. })
        ));
    }

    // ── DISCRIMINATING: malformed input surfaces as a Malformed rejection
    //    naming the input, never a panic or a silent default. ───────────────
    #[test]
    fn check_str_surfaces_malformed_input_as_typed_rejection() {
        match VersionPolicy::production().check_str("7.1") {
            Err(PolicyRejection::Malformed { input, .. }) => assert_eq!(input, "7.1"),
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    // ── DISCRIMINATING: rejection diagnostics are ACTIONABLE — they name the
    //    offending version, the supported window and the pin to install. ──
    #[test]
    fn rejection_messages_are_actionable() {
        let policy = VersionPolicy::production();
        let pin = pinned_version();

        let past = format!("{}.{}.0", pin.major, pin.minor + 1);
        let msg = policy
            .check(&v(&past))
            .expect_err("out of range")
            .to_string();
        assert!(msg.contains(&past), "names the version: {msg}");
        assert!(
            msg.contains(SUPPORTED_TSGO_RANGE_LABEL),
            "names the window: {msg}"
        );
        assert!(msg.contains(TYPESCRIPT_PIN), "names the pin: {msg}");

        let below = format!("{}.{}.9", pin.major, pin.minor - 1);
        let msg = policy
            .check(&v(&below))
            .expect_err("below floor")
            .to_string();
        assert!(msg.contains(&below), "names the version: {msg}");
        assert!(msg.contains(TYPESCRIPT_PIN), "names the floor: {msg}");

        let nightly = format!("{}.{}.{}-dev.20261022.1", pin.major, pin.minor, pin.patch);
        let msg = policy.check(&v(&nightly)).expect_err("nightly").to_string();
        assert!(msg.contains(&nightly), "names the version: {msg}");
        assert!(msg.contains("nightly"), "names the channel: {msg}");
        assert!(msg.contains(TYPESCRIPT_PIN), "names the pin: {msg}");
    }
}
