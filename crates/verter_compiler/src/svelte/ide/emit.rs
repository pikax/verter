//! Small projection helpers shared by the Svelte IDE projector.
//!
//! These are pure text/transform utilities — no type lowering, no resolution.

/// The reporting severity of a projector diagnostic.
///
/// Most diagnostics are `Error` (an unsupported construct the projection could
/// not check). The experimental await-EXPRESSION (F6) is REAL-checkable TSX —
/// it is reported as `Information` (a heads-up that the syntax is experimental),
/// never an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// An unsupported / uncheckable construct.
    Error,
    /// An informational heads-up — the construct IS checked.
    Information,
}

/// A machine-stable diagnostic code for a matrix construct the projector flags.
///
/// The projector emits the construct's expressions checked and records one of
/// these codes so the session surfaces a typed diagnostic (never a crash, never
/// a silent drop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedKind {
    /// An experimental Svelte await-EXPRESSION (5.36+, `experimental.async`).
    /// F6: REAL-checkable projection (`__verter_await_expr`) + an INFORMATIONAL
    /// diagnostic — the syntax is experimental, not unsupported.
    AwaitExperimental,
    /// An unrecognised construct parsed without crash.
    Unknown,
}

impl UnsupportedKind {
    /// The machine-stable diagnostic code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::AwaitExperimental => "svelte-await-experimental",
            Self::Unknown => "svelte-unsupported-construct",
        }
    }

    /// A human-readable message.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::AwaitExperimental => {
                "Svelte await-expressions are experimental (5.36+, `experimental.async`); type-checked here, behavior may change before Svelte 6."
            }
            Self::Unknown => "unsupported Svelte construct",
        }
    }

    /// The reporting severity for this kind. The experimental await-EXPRESSION
    /// is REAL-checkable, so it is `Information`; every other kind is an `Error`.
    #[must_use]
    pub fn severity(self) -> DiagnosticSeverity {
        match self {
            Self::AwaitExperimental => DiagnosticSeverity::Information,
            Self::Unknown => DiagnosticSeverity::Error,
        }
    }
}

/// Whether an attribute name is a CSS custom property (`--x`) — these are
/// stripped from the projected JSX attribute position (D-ap), their value
/// void-checked.
#[must_use]
pub fn is_css_custom_property(name: &str) -> bool {
    name.starts_with("--")
}
