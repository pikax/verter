//! The Browserslist query vocabulary this snapshot can answer.
//!
//! Expansion is an offline computation over the pinned browser release data, so
//! every form below is admitted or carries the reason it is not. A form needing
//! a statistic, a list or a clock the snapshot does not carry answers that
//! reason; it never fetches, guesses or narrows to a subset it can answer.

/// Every Browserslist query form, closed so a new one must state its admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::compat) enum BrowserslistQueryForm {
    /// `chrome 100`
    ExactVersion,
    /// `chrome 90-100`
    VersionRange,
    /// `chrome >= 100`
    VersionComparison,
    /// `last 2 versions`
    LastVersions,
    /// `last 2 chrome versions`
    LastBrowserVersions,
    /// `last 2 major versions`
    LastMajorVersions,
    /// `last 2 chrome major versions`
    LastBrowserMajorVersions,
    /// `firefox esr`
    FirefoxEsr,
    /// `unreleased versions`
    UnreleasedVersions,
    /// `unreleased chrome versions`
    UnreleasedBrowserVersions,
    /// `since 2015`, `since 2015-03`
    SinceDate,
    /// `a, b`, `a or b`, `a and b`, `not a`
    Composition,
    /// `defaults`, which expands through usage share and `dead`
    Defaults,
    /// `dead`, `not dead`
    Dead,
    /// `> 0.25%`, `>= 1%`, `< 1%`
    GlobalUsage,
    /// `> 1% in US`, `> 1% in alt-AS`, `> 1% in my stats`
    RegionalUsage,
    /// `cover 99.5%`
    Coverage,
    /// `last 2 years`, relative to a clock
    LastYears,
    /// `supports es6-module`, resolved through caniuse
    SupportsFeature,
    /// `node 20`, `current node`, `maintained node versions`
    NodeRuntime,
    /// `electron 30`
    Electron,
    /// `extends browserslist-config-x`, resolved through the project model
    Extends,
}

/// Why a query form cannot be answered from the pinned snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::compat) enum UnsupportedQueryReason {
    UsageStatisticNotCarried,
    RegionalStatisticNotCarried,
    DeadListNotCarried,
    ClockDependent,
    CaniuseNotAdmitted,
    NodeReleaseScheduleNotCarried,
    ElectronMappingNotCarried,
    ResolvedByProjectModel,
}

/// A form's standing against the pinned snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::compat) enum QueryAdmission {
    Admitted,
    Unsupported(UnsupportedQueryReason),
}

impl BrowserslistQueryForm {
    pub(in crate::compat) const fn admission(self) -> QueryAdmission {
        use UnsupportedQueryReason as Reason;
        let reason = match self {
            Self::ExactVersion
            | Self::VersionRange
            | Self::VersionComparison
            | Self::LastVersions
            | Self::LastBrowserVersions
            | Self::LastMajorVersions
            | Self::LastBrowserMajorVersions
            | Self::FirefoxEsr
            | Self::UnreleasedVersions
            | Self::UnreleasedBrowserVersions
            | Self::SinceDate
            | Self::Composition => return QueryAdmission::Admitted,
            Self::Defaults | Self::GlobalUsage | Self::Coverage => Reason::UsageStatisticNotCarried,
            Self::RegionalUsage => Reason::RegionalStatisticNotCarried,
            Self::Dead => Reason::DeadListNotCarried,
            Self::LastYears => Reason::ClockDependent,
            Self::SupportsFeature => Reason::CaniuseNotAdmitted,
            Self::NodeRuntime => Reason::NodeReleaseScheduleNotCarried,
            Self::Electron => Reason::ElectronMappingNotCarried,
            Self::Extends => Reason::ResolvedByProjectModel,
        };
        QueryAdmission::Unsupported(reason)
    }
}

/// The dataset browser id a Browserslist family name denotes, matched
/// case-insensitively as Browserslist does. `None` means the dataset carries no
/// such family (`op_mini`, `and_uc`, `baidu`, ...), which stays unknown.
pub(in crate::compat) fn browserslist_family(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "chrome" => "chrome",
        "and_chr" | "chromeandroid" => "chrome_android",
        "edge" => "edge",
        "firefox" | "ff" | "fx" => "firefox",
        "and_ff" | "firefoxandroid" => "firefox_android",
        "explorer" | "ie" => "ie",
        "opera" => "opera",
        "op_mob" | "operamobile" => "opera_android",
        "safari" => "safari",
        "ios" | "ios_saf" => "safari_ios",
        "samsung" => "samsunginternet_android",
        "android" => "webview_android",
        _ => return None,
    })
}
