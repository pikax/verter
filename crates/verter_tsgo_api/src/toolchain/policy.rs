//! Supported-tsgo-version policy (the ratified stable-only contract).
//!
//! tsgo is the TypeScript 7 native compiler and follows TypeScript SemVer
//! (`major.minor.patch`). Verter supports ONE channel:
//!
//! - [`BUNDLED_TSGO_VERSION`] — the known-good floor Verter ships as the
//!   offline bundled sidecar (Phase B): `7.0.2`.
//! - [`SUPPORTED_TSGO_RANGE`] — `>=7.0.2, <7.1.0`, STABLE ONLY. A candidate
//!   qualifies iff its version has no prerelease AND lands in range. Stable
//!   `7.0.x` patches auto-qualify; `7.1`, `8.0`, RCs, betas, and nightlies do
//!   NOT.
//!
//! "Latest supported" always means the greatest STABLE version satisfying the
//! range — never a registry `latest` tag.
//!
//! A DEV-ONLY escape hatch ([`VersionPolicy::with_dev_nightly_override`],
//! env-gated via [`DEV_NIGHTLY_OVERRIDE_ENV`]) re-admits integer-handle
//! nightlies for nightly gate testing. It is never on in production, and even
//! under the override RC/beta builds and out-of-range versions stay refused.

use std::fmt;

/// The bundled offline floor: the known-good tsgo build Verter ships as a
/// sidecar (the binary itself lands in Phase B; the version contract is here).
pub const BUNDLED_TSGO_VERSION: TsgoVersion = TsgoVersion::new(7, 0, 2);

/// The inclusive lower bound of the supported window.
pub const SUPPORTED_TSGO_RANGE_MIN: TsgoVersion = TsgoVersion::new(7, 0, 2);

/// The EXCLUSIVE upper bound of the supported window: any `7.1.x` or newer
/// minor/major is refused until Verter explicitly adds support.
pub const SUPPORTED_TSGO_RANGE_MAX_EXCLUSIVE: TsgoVersion = TsgoVersion::new(7, 1, 0);

/// Human-readable supported window, embedded in every policy diagnostic.
pub const SUPPORTED_TSGO_RANGE_LABEL: &str = ">=7.0.2, <7.1.0";

/// The policy identity used in the on-disk temp-cache layout
/// (`…/<target-triple>/<policy-id>/<version>/…`), so a policy bump can never
/// collide with cache entries written under a different support window.
pub const SUPPORTED_POLICY_ID: &str = "ts7-stable";

/// The earliest nightly build date (`YYYYMMDD` in `7.0.0-dev.YYYYMMDD.N`)
/// whose `--api` wire issues opaque handles as bare JSON integers. Earlier
/// nightlies issue STRING handles — a different wire class the codec does not
/// speak — and are refused even under the dev override.
pub const NIGHTLY_INTEGER_HANDLE_FLIP_DATE: u32 = 20260604;

/// DEV-ONLY escape hatch: when this env var is exactly `1`,
/// [`VersionPolicy::from_env`] re-admits integer-handle nightlies (for nightly
/// gate testing). Never set this in production or CI product lanes.
pub const DEV_NIGHTLY_OVERRIDE_ENV: &str = "VERTER_TSGO_DEV_ALLOW_NIGHTLY";

/// A strictly-parsed SemVer version. tsgo versions are TypeScript SemVer:
/// `major.minor.patch` with an optional `-prerelease` and optional `+build`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TsgoVersion {
    /// SemVer major.
    pub major: u64,
    /// SemVer minor.
    pub minor: u64,
    /// SemVer patch.
    pub patch: u64,
    /// The prerelease suffix without the leading `-` (`rc.1`,
    /// `dev.20260604.1`), or `None` for a stable release.
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
                validate_identifiers(build, "build metadata").map_err(|r| malformed(&r))?;
                (*head, Some(build.to_string()))
            }
            _ => return Err(malformed("more than one `+` build-metadata separator")),
        };

        // Prerelease: one optional `-` suffix (identifiers may themselves
        // contain `-`, so split at the FIRST one).
        let (core, prerelease) = match core_and_pre.split_once('-') {
            Some((core, pre)) => {
                validate_identifiers(pre, "prerelease").map_err(|r| malformed(&r))?;
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

/// Validate a dot-separated SemVer identifier list (prerelease or build).
fn validate_identifiers(list: &str, what: &str) -> Result<(), String> {
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
    /// release; build metadata is ignored for precedence.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match self.core().cmp(&other.core()) {
            Ordering::Equal => match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            },
            ordering => ordering,
        }
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
            "`{}` is not a strict SemVer version ({}); tsgo versions look like `7.0.2`",
            self.input, self.reason
        )
    }
}

impl std::error::Error for VersionParseError {}

/// The prerelease channel a refused (or dev-admitted) build belongs to.
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
    /// A stable version below [`SUPPORTED_TSGO_RANGE_MIN`].
    BelowSupportedFloor {
        /// The offending version.
        version: TsgoVersion,
    },
    /// A stable version at or past [`SUPPORTED_TSGO_RANGE_MAX_EXCLUSIVE`].
    OutOfSupportedRange {
        /// The offending version.
        version: TsgoVersion,
    },
    /// A prerelease build (rc/beta/alpha/nightly/other), which production
    /// never accepts.
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
                 (TypeScript 7 native) stable {SUPPORTED_TSGO_RANGE_LABEL}"
            ),
            Self::BelowSupportedFloor { version } => write!(
                f,
                "tsgo {version} is below the supported floor; Verter supports tsgo stable \
                 {SUPPORTED_TSGO_RANGE_LABEL} — install typescript@{BUNDLED_TSGO_VERSION} or newer 7.0.x"
            ),
            Self::OutOfSupportedRange { version } => write!(
                f,
                "tsgo {version} is outside the supported window; Verter supports tsgo stable \
                 {SUPPORTED_TSGO_RANGE_LABEL} — a new TypeScript minor/major needs explicit \
                 Verter support, install a 7.0.x stable instead"
            ),
            Self::Prerelease { version, channel } => {
                let dev_hint = if *channel == PrereleaseChannel::Nightly {
                    format!(" (nightly gate testing may set {DEV_NIGHTLY_OVERRIDE_ENV}=1)")
                } else {
                    String::new()
                };
                write!(
                    f,
                    "tsgo {version} is a {channel_name} prerelease; Verter supports tsgo \
                     STABLE {SUPPORTED_TSGO_RANGE_LABEL} only — install the matching stable \
                     7.0.x release{dev_hint}",
                    channel_name = match channel {
                        PrereleaseChannel::ReleaseCandidate => "release-candidate",
                        PrereleaseChannel::Beta => "beta",
                        PrereleaseChannel::Alpha => "alpha",
                        PrereleaseChannel::Nightly => "nightly",
                        PrereleaseChannel::Other => "prerelease",
                    }
                )
            }
            Self::BuildMetadata { version } => write!(
                f,
                "tsgo {version} carries build metadata (an unverified local build); Verter \
                 supports tsgo stable {SUPPORTED_TSGO_RANGE_LABEL} from the official packages"
            ),
        }
    }
}

impl std::error::Error for PolicyRejection {}

/// The supported-version acceptance policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionPolicy {
    allow_dev_nightly: bool,
}

impl VersionPolicy {
    /// The production policy: stable `7.0.x` in range only.
    pub const fn production() -> Self {
        Self {
            allow_dev_nightly: false,
        }
    }

    /// The policy the running process uses: production, unless the DEV-ONLY
    /// [`DEV_NIGHTLY_OVERRIDE_ENV`] var is exactly `1`.
    pub fn from_env() -> Self {
        match std::env::var(DEV_NIGHTLY_OVERRIDE_ENV).as_deref() {
            Ok("1") => Self::with_dev_nightly_override(),
            _ => Self::production(),
        }
    }

    /// The DEV-ONLY override policy: production rules PLUS integer-handle
    /// nightlies (for nightly gate testing). Never enable in production.
    pub const fn with_dev_nightly_override() -> Self {
        Self {
            allow_dev_nightly: true,
        }
    }

    /// Whether the dev nightly override is active.
    pub fn allows_dev_nightly(&self) -> bool {
        self.allow_dev_nightly
    }

    /// Check a parsed version against the support window.
    ///
    /// A candidate qualifies iff it is stable (no prerelease, no build
    /// metadata) and `>=7.0.2, <7.1.0`. Under the DEV-ONLY override an
    /// integer-handle nightly (`7.0.0-dev.<date>.<n>` at/after
    /// [`NIGHTLY_INTEGER_HANDLE_FLIP_DATE`]) also qualifies; every other
    /// prerelease stays refused.
    pub fn check(&self, version: &TsgoVersion) -> Result<(), PolicyRejection> {
        if version.build.is_some() {
            return Err(PolicyRejection::BuildMetadata {
                version: version.clone(),
            });
        }
        if let Some(prerelease) = &version.prerelease {
            let channel = classify_prerelease(prerelease);
            if self.allow_dev_nightly && is_integer_handle_nightly(version) {
                return Ok(());
            }
            return Err(PolicyRejection::Prerelease {
                version: version.clone(),
                channel,
            });
        }
        if version.core() < SUPPORTED_TSGO_RANGE_MIN.core() {
            return Err(PolicyRejection::BelowSupportedFloor {
                version: version.clone(),
            });
        }
        if version.core() >= SUPPORTED_TSGO_RANGE_MAX_EXCLUSIVE.core() {
            return Err(PolicyRejection::OutOfSupportedRange {
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

/// Classify a prerelease suffix into its channel for diagnostics.
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

/// Whether `version` is a `7.0.0-dev.<YYYYMMDD>.<n>` nightly at or after the
/// integer-handle wire flip ([`NIGHTLY_INTEGER_HANDLE_FLIP_DATE`]).
fn is_integer_handle_nightly(version: &TsgoVersion) -> bool {
    if version.core() != (7, 0, 0) {
        return false;
    }
    let Some(prerelease) = &version.prerelease else {
        return false;
    };
    let Some(dev) = prerelease.strip_prefix("dev.") else {
        return false;
    };
    let Some((date, seq)) = dev.split_once('.') else {
        return false;
    };
    if date.len() != 8
        || !date.bytes().all(|b| b.is_ascii_digit())
        || seq.is_empty()
        || !seq.bytes().all(|b| b.is_ascii_digit())
    {
        return false;
    }
    let Ok(date_num) = date.parse::<u32>() else {
        return false;
    };
    date_num >= NIGHTLY_INTEGER_HANDLE_FLIP_DATE
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── strict SemVer parse ────────────────────────────────────────────────

    #[test]
    fn parse_accepts_stable_prerelease_and_build_shapes() {
        let v = TsgoVersion::parse("7.0.2").expect("stable parses");
        assert_eq!((v.major, v.minor, v.patch), (7, 0, 2));
        assert_eq!(v.prerelease, None);
        assert_eq!(v.build, None);
        assert!(v.is_stable());

        let v = TsgoVersion::parse("7.0.2-rc.1").expect("dotted rc parses");
        assert_eq!(v.prerelease.as_deref(), Some("rc.1"));
        assert!(!v.is_stable());

        let v = TsgoVersion::parse("7.0.0-dev.20260604.1").expect("nightly parses");
        assert_eq!(v.prerelease.as_deref(), Some("dev.20260604.1"));
        assert!(!v.is_stable());

        let v = TsgoVersion::parse("7.0.2+build.5").expect("build metadata parses");
        assert_eq!(v.build.as_deref(), Some("build.5"));
        assert!(v.is_stable(), "build metadata is not a prerelease");
    }

    // ── DISCRIMINATING: the parser is STRICT — every malformed shape below is
    //    a hard parse error, never a silent partial parse. ───────────────────
    #[test]
    fn parse_rejects_malformed_versions() {
        for input in [
            "",
            "   ",
            "7.0",
            "7",
            "7.0.2.3",
            "7.0.x",
            "x.0.2",
            "v7.0.2",
            "7.0.2 ",
            " 7.0.2",
            "7..2",
            "7.00.2",      // leading zero in a core component is not strict SemVer
            "07.0.2",      // leading zero in major
            "7.0.2-",      // empty prerelease
            "7.0.2+",      // empty build
            "7.0.2-rc..1", // empty prerelease identifier
            "7.0.2-rc_1",  // illegal identifier char
            "garbage",
            "7.0.2+b+b", // a second `+` is not SemVer
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
        assert!(TsgoVersion::parse("7.0.2").unwrap() < TsgoVersion::parse("7.0.10").unwrap());
        assert!(TsgoVersion::parse("7.0.9").unwrap() < TsgoVersion::parse("7.1.0").unwrap());
        assert!(TsgoVersion::parse("7.0.99").unwrap() < TsgoVersion::parse("8.0.0").unwrap());
        assert!(
            TsgoVersion::parse("7.0.2-rc.1").unwrap() < TsgoVersion::parse("7.0.2").unwrap(),
            "a prerelease orders before its release"
        );
    }

    // ── the supported-window constants ─────────────────────────────────────

    #[test]
    fn policy_constants_pin_the_stable_7_0_window() {
        assert_eq!(
            BUNDLED_TSGO_VERSION,
            TsgoVersion::new(7, 0, 2),
            "the bundled offline floor is pinned to 7.0.2"
        );
        assert_eq!(SUPPORTED_TSGO_RANGE_MIN, TsgoVersion::new(7, 0, 2));
        assert_eq!(
            SUPPORTED_TSGO_RANGE_MAX_EXCLUSIVE,
            TsgoVersion::new(7, 1, 0)
        );
        assert!(
            SUPPORTED_TSGO_RANGE_LABEL.contains(">=7.0.2")
                && SUPPORTED_TSGO_RANGE_LABEL.contains("<7.1.0"),
            "the range label must state the window for diagnostics: {SUPPORTED_TSGO_RANGE_LABEL}"
        );
    }

    // ── DISCRIMINATING: the bundled floor ALWAYS satisfies the production
    //    policy (the shipped offline fallback can never be self-refusing). ──
    #[test]
    fn bundled_version_always_satisfies_production_policy() {
        VersionPolicy::production()
            .check(&BUNDLED_TSGO_VERSION)
            .expect("the bundled floor must qualify under the production policy");
        assert!(BUNDLED_TSGO_VERSION >= SUPPORTED_TSGO_RANGE_MIN);
        assert!(BUNDLED_TSGO_VERSION < SUPPORTED_TSGO_RANGE_MAX_EXCLUSIVE);
        assert!(BUNDLED_TSGO_VERSION.is_stable());
    }

    // ── DISCRIMINATING: production accepts stable 7.0.x patches at/above the
    //    floor — the auto-accept-patch contract. ─────────────────────────────
    #[test]
    fn production_accepts_stable_patches_in_range() {
        let policy = VersionPolicy::production();
        for v in ["7.0.2", "7.0.3", "7.0.9", "7.0.13", "7.0.99"] {
            let parsed = TsgoVersion::parse(v).unwrap();
            assert!(
                policy.check(&parsed).is_ok(),
                "stable patch `{v}` must qualify"
            );
        }
    }

    // ── DISCRIMINATING: production refuses versions below the floor and
    //    outside the supported minor/major — with DISTINCT reasons. ─────────
    #[test]
    fn production_rejects_below_floor_and_out_of_range() {
        let policy = VersionPolicy::production();
        for v in ["7.0.0", "7.0.1", "6.9.9", "0.0.1"] {
            let parsed = TsgoVersion::parse(v).unwrap();
            assert!(
                matches!(
                    policy.check(&parsed),
                    Err(PolicyRejection::BelowSupportedFloor { .. })
                ),
                "`{v}` is below the 7.0.2 floor"
            );
        }
        for v in ["7.1.0", "7.1.2", "7.2.0", "8.0.0", "9.3.1"] {
            let parsed = TsgoVersion::parse(v).unwrap();
            assert!(
                matches!(
                    policy.check(&parsed),
                    Err(PolicyRejection::OutOfSupportedRange { .. })
                ),
                "`{v}` is outside the supported 7.0 window"
            );
        }
    }

    // ── DISCRIMINATING: production refuses EVERY prerelease channel — rc,
    //    beta, alpha, nightly — even when the core is in range. This is the
    //    removed production RC/nightly acceptance: these used to qualify. ────
    #[test]
    fn production_rejects_all_prerelease_channels() {
        let policy = VersionPolicy::production();
        let cases = [
            ("7.0.2-rc", PrereleaseChannel::ReleaseCandidate),
            ("7.0.2-rc.1", PrereleaseChannel::ReleaseCandidate),
            ("7.0.1-rc", PrereleaseChannel::ReleaseCandidate),
            ("7.1.0-rc", PrereleaseChannel::ReleaseCandidate),
            ("7.0.2-beta", PrereleaseChannel::Beta),
            ("7.0.2-beta.3", PrereleaseChannel::Beta),
            ("7.0.2-alpha", PrereleaseChannel::Alpha),
            ("7.0.0-dev.20260703.1", PrereleaseChannel::Nightly),
            ("7.0.0-dev.20260604.1", PrereleaseChannel::Nightly),
            ("7.0.2-insiders", PrereleaseChannel::Other),
        ];
        for (v, channel) in cases {
            let parsed = TsgoVersion::parse(v).unwrap();
            match policy.check(&parsed) {
                Err(PolicyRejection::Prerelease { channel: got, .. }) => {
                    assert_eq!(got, channel, "wrong channel classification for `{v}`");
                }
                other => panic!("`{v}` must be a Prerelease rejection, got {other:?}"),
            }
        }
    }

    // ── DISCRIMINATING: build metadata is an unverified build and is refused
    //    even when the core is an in-range stable patch. ─────────────────────
    #[test]
    fn production_rejects_build_metadata() {
        let policy = VersionPolicy::production();
        let parsed = TsgoVersion::parse("7.0.2+build.5").unwrap();
        assert!(matches!(
            policy.check(&parsed),
            Err(PolicyRejection::BuildMetadata { .. })
        ));
    }

    // ── DISCRIMINATING: malformed input surfaces as a Malformed rejection
    //    naming the input, never a panic or a silent default. ───────────────
    #[test]
    fn check_str_surfaces_malformed_input_as_typed_rejection() {
        let policy = VersionPolicy::production();
        match policy.check_str("7.0") {
            Err(PolicyRejection::Malformed { input, .. }) => assert_eq!(input, "7.0"),
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    // ── DISCRIMINATING: the DEV-ONLY override re-admits integer-handle
    //    nightlies ONLY — never rc/beta, never pre-flip string-handle
    //    nightlies, never out-of-range cores. ────────────────────────────────
    #[test]
    fn dev_override_admits_only_integer_handle_nightlies() {
        let policy = VersionPolicy::with_dev_nightly_override();
        assert!(policy.allows_dev_nightly());

        // Integer-handle nightlies (at/after the wire flip) qualify.
        for v in [
            "7.0.0-dev.20260604.1",
            "7.0.0-dev.20260703.1",
            "7.0.0-dev.20270101.42",
        ] {
            let parsed = TsgoVersion::parse(v).unwrap();
            assert!(
                policy.check(&parsed).is_ok(),
                "integer-handle nightly `{v}` must qualify under the dev override"
            );
        }

        // A pre-flip nightly speaks the STRING-handle wire: still refused.
        let pre_flip = TsgoVersion::parse("7.0.0-dev.20260603.1").unwrap();
        assert!(matches!(
            policy.check(&pre_flip),
            Err(PolicyRejection::Prerelease {
                channel: PrereleaseChannel::Nightly,
                ..
            })
        ));

        // The override is NOT a general prerelease/range bypass.
        for v in [
            "7.0.2-rc",
            "7.0.2-rc.1",
            "7.0.2-beta",
            "7.0.1",
            "7.1.0",
            "8.0.0",
            "7.0.2+build.1",
        ] {
            let parsed = TsgoVersion::parse(v).unwrap();
            assert!(
                policy.check(&parsed).is_err(),
                "`{v}` must stay refused under the dev override"
            );
        }
    }

    #[test]
    fn production_policy_disallows_dev_nightly() {
        assert!(!VersionPolicy::production().allows_dev_nightly());
    }

    // ── DISCRIMINATING: rejection diagnostics are ACTIONABLE — they name the
    //    offending version, the supported window, the stable-only rule, and
    //    (for nightlies) the dev override knob. ──────────────────────────────
    #[test]
    fn rejection_messages_are_actionable() {
        let policy = VersionPolicy::production();

        let err = policy
            .check(&TsgoVersion::parse("7.1.0").unwrap())
            .expect_err("out of range");
        let msg = err.to_string();
        assert!(msg.contains("7.1.0"), "names the version: {msg}");
        assert!(msg.contains(">=7.0.2, <7.1.0"), "names the window: {msg}");
        assert!(msg.contains("stable"), "states the stable-only rule: {msg}");

        let err = policy
            .check(&TsgoVersion::parse("7.0.1").unwrap())
            .expect_err("below floor");
        let msg = err.to_string();
        assert!(msg.contains("7.0.1"), "names the version: {msg}");
        assert!(msg.contains("7.0.2"), "names the floor: {msg}");

        let err = policy
            .check(&TsgoVersion::parse("7.0.2-rc.1").unwrap())
            .expect_err("rc refused");
        let msg = err.to_string();
        assert!(msg.contains("7.0.2-rc.1"), "names the version: {msg}");
        assert!(msg.contains("stable"), "states the stable-only rule: {msg}");

        let err = policy
            .check(&TsgoVersion::parse("7.0.0-dev.20260703.1").unwrap())
            .expect_err("nightly refused in production");
        let msg = err.to_string();
        assert!(
            msg.contains("7.0.0-dev.20260703.1"),
            "names the version: {msg}"
        );
        assert!(
            msg.contains(DEV_NIGHTLY_OVERRIDE_ENV),
            "points at the dev-only override knob: {msg}"
        );
    }

    // ── env-gated override construction (the env var itself is never mutated
    //    in tests; from_env only READS). ─────────────────────────────────────
    #[test]
    fn from_env_mirrors_production_unless_the_override_var_is_set() {
        // With the var unset (the normal test environment) from_env IS the
        // production policy. If a dev shell exports the override, from_env
        // must mirror the override instead — either way it must equal one of
        // the two explicit constructors.
        let from_env = VersionPolicy::from_env();
        let expected = match std::env::var(DEV_NIGHTLY_OVERRIDE_ENV).as_deref() {
            Ok("1") => VersionPolicy::with_dev_nightly_override(),
            _ => VersionPolicy::production(),
        };
        assert_eq!(from_env, expected);
    }
}
