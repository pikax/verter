//! The Browserslist query vocabulary this snapshot can answer.
//!
//! Expansion is an offline computation over the pinned browser release data, so
//! every form below is admitted or carries the reason it is not. A form needing
//! a statistic, a list or a clock the snapshot does not carry answers that
//! reason; it never fetches, guesses or narrows to a subset it can answer.
//!
//! A form that names its family is answerable target by target. A form that
//! ranges over every Browserslist family is answerable only together with the
//! families the snapshot does not carry: [`all_family_coverage`] names them, and
//! each one must stand in the expansion as an unknown entry rather than vanish.

use super::snapshot::CompatSnapshot;

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
    /// The form names its family, so each target it yields lies in that family.
    /// A named family the snapshot does not carry answers unknown on its own.
    /// For composition the combinator is admitted and each operand keeps its own
    /// standing.
    Admitted,
    /// The form ranges over every Browserslist family, so the release data
    /// answers only the families it carries. An expansion must hold one unknown
    /// entry for each family [`all_family_coverage`] lists as uncarried, and
    /// leaves out the dataset families it lists as excluded; it never stands as
    /// a complete answer over the carried subset.
    AllFamilies,
    Unsupported(UnsupportedQueryReason),
}

impl BrowserslistQueryForm {
    pub(in crate::compat) const fn admission(self) -> QueryAdmission {
        use UnsupportedQueryReason as Reason;
        let reason = match self {
            Self::ExactVersion
            | Self::VersionRange
            | Self::VersionComparison
            | Self::LastBrowserVersions
            | Self::LastBrowserMajorVersions
            | Self::FirefoxEsr
            | Self::UnreleasedBrowserVersions
            | Self::Composition => return QueryAdmission::Admitted,
            Self::LastVersions
            | Self::LastMajorVersions
            | Self::UnreleasedVersions
            | Self::SinceDate => return QueryAdmission::AllFamilies,
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

/// The families an all-family form ranges over: Browserslist's canonical
/// family names, the caniuse agents, in ascending order.
pub(in crate::compat) const BROWSERSLIST_FAMILIES: [&str; 19] = [
    "and_chr", "and_ff", "and_qq", "and_uc", "android", "baidu", "bb", "chrome", "edge", "firefox",
    "ie", "ie_mob", "ios_saf", "kaios", "op_mini", "op_mob", "opera", "safari", "samsung",
];

/// Why a Browserslist family has no release data in the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::compat) enum UncarriedFamilyReason {
    /// The dataset has no family this name denotes.
    NoDatasetFamily,
    /// The name denotes a dataset family this snapshot carries no record of.
    ReleaseDataAbsent,
}

/// A Browserslist family an all-family expansion must hold as an unknown entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::compat) struct UncarriedFamily {
    pub(in crate::compat) family: &'static str,
    pub(in crate::compat) reason: UncarriedFamilyReason,
}

/// What an all-family form ranges over in one snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct AllFamilyCoverage {
    /// Browserslist family and the dataset family whose releases answer it.
    pub(in crate::compat) carried: Vec<(&'static str, &'static str)>,
    /// Families the expansion holds as unknown entries, never drops.
    pub(in crate::compat) uncarried: Vec<UncarriedFamily>,
    /// Dataset families no Browserslist name denotes (`nodejs`, `deno`, ...).
    /// Browserslist never ranges over them, so an all-family form excludes them.
    pub(in crate::compat) excluded: Vec<String>,
}

/// Partition the Browserslist families, and the snapshot's own families, for an
/// all-family form.
pub(in crate::compat) fn all_family_coverage(snapshot: &CompatSnapshot) -> AllFamilyCoverage {
    let mut carried = Vec::new();
    let mut uncarried = Vec::new();
    for family in BROWSERSLIST_FAMILIES {
        match browserslist_family(family) {
            Some(dataset) if snapshot.browser(dataset).is_some() => {
                carried.push((family, dataset));
            }
            Some(_) => uncarried.push(UncarriedFamily {
                family,
                reason: UncarriedFamilyReason::ReleaseDataAbsent,
            }),
            None => uncarried.push(UncarriedFamily {
                family,
                reason: UncarriedFamilyReason::NoDatasetFamily,
            }),
        }
    }
    let excluded = snapshot
        .browsers()
        .map(|(dataset, _)| dataset)
        .filter(|dataset| {
            !BROWSERSLIST_FAMILIES
                .iter()
                .any(|family| browserslist_family(family) == Some(*dataset))
        })
        .map(str::to_owned)
        .collect();
    AllFamilyCoverage {
        carried,
        uncarried,
        excluded,
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
