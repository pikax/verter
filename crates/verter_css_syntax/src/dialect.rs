#[path = "dialect/css.rs"]
pub mod css;
#[path = "dialect/less.rs"]
pub mod less;
#[path = "dialect/scss.rs"]
pub mod scss;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CssDialect {
    Css,
    Scss,
    Less,
}

impl CssDialect {
    #[inline]
    pub(crate) const fn allows_line_comments(self) -> bool {
        matches!(self, Self::Scss | Self::Less)
    }
}
