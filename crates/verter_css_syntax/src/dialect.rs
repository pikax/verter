#[path = "dialect/css.rs"]
pub mod css;
#[path = "dialect/less.rs"]
pub mod less;
#[path = "dialect/sass.rs"]
pub mod sass;
#[path = "dialect/scss.rs"]
pub mod scss;
#[path = "dialect/stylus.rs"]
pub mod stylus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CssDialect {
    Css,
    Scss,
    Sass,
    Less,
    Stylus,
}

impl CssDialect {
    /// Closed dialect universe this crate parses natively.
    pub const ALL: [Self; 5] = [Self::Css, Self::Scss, Self::Sass, Self::Less, Self::Stylus];

    /// Every `lang` spelling that names a dialect in [`Self::ALL`], paired
    /// with the dialect it names. Exact bytes, in the one casing the
    /// ecosystem's preprocessor tables are keyed by.
    pub const LANG_SPELLINGS: [(&'static str, Self); 6] = [
        ("css", Self::Css),
        ("scss", Self::Scss),
        ("sass", Self::Sass),
        ("less", Self::Less),
        ("stylus", Self::Stylus),
        ("styl", Self::Stylus),
    ];

    /// Map an authored `lang` / dialect spelling onto the closed native set.
    /// Unknown strings are `None` — callers must fail closed, not default to CSS.
    ///
    /// This is the single authority for the spelling → dialect identity within
    /// the native set. A consumer that keeps its own table drifts from this
    /// one, and the drift is silent: the same `lang="…"` resolves in one route
    /// and fails closed in another.
    ///
    /// Matching is byte-exact for every dialect that needs an external tool:
    /// `lang="SCSS"` does not name the `scss` processor and therefore must not
    /// be classified as SCSS. This function answers exact dialect identity,
    /// not the caller's fallback policy. In particular, Vue's compiler treats
    /// a missing processor-table entry as plain CSS; a caller implementing
    /// that policy must do so after this exact lookup returns `None`. `styl`
    /// is accepted because it is a real table key, not a casing variant.
    ///
    /// `css` is the one exception, and it is an exception because the reason
    /// above does not reach it: plain CSS is handed to no preprocessor table
    /// at all, so no downstream tool can fail on the spelling, and
    /// `<style lang="CSS">` is unambiguously a CSS block that the reference
    /// Vue pipeline compiles. Failing it closed would cost the block every
    /// CSS diagnostic, hover and colour the editor otherwise serves, and buy
    /// no agreement with anything. It is therefore matched
    /// ASCII-case-insensitively — the only spelling for which the exact-key
    /// rule has nothing to protect.
    ///
    /// The wider carrier-level dialect classification (`PostCss` and the
    /// unrecognised state, which have no [`CssDialect`] to map onto) is a
    /// separate, deliberately larger universe owned by the carrier parse
    /// projection; this function is not its authority.
    #[must_use]
    pub fn from_lang(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("css") {
            return Some(Self::Css);
        }
        Self::LANG_SPELLINGS
            .into_iter()
            .find_map(|(spelling, dialect)| (value == spelling).then_some(dialect))
    }

    /// Whether bytes authored in this dialect need an external preprocessor
    /// before any plain-CSS-only stage can run over them.
    ///
    /// The one authority for "is this already CSS". Spelling it as
    /// `!= CssDialect::Css` at each decision site reads as a dialect
    /// comparison rather than the pipeline question it actually is, and every
    /// such site has to be found again when the answer changes.
    #[must_use]
    pub const fn requires_external_preprocessing(self) -> bool {
        match self {
            Self::Css => false,
            Self::Scss | Self::Sass | Self::Less | Self::Stylus => true,
        }
    }

    #[inline]
    pub(crate) const fn allows_line_comments(self) -> bool {
        matches!(self, Self::Scss | Self::Sass | Self::Less | Self::Stylus)
    }
}
