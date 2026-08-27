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

    #[inline]
    pub(crate) const fn allows_line_comments(self) -> bool {
        matches!(self, Self::Scss | Self::Sass | Self::Less | Self::Stylus)
    }
}
