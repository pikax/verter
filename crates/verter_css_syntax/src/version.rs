/// Cache-visible identity of the CSS syntax grammar and emitted event shape.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CssSyntaxGrammarVersion(u32);

impl CssSyntaxGrammarVersion {
    /// Current grammar/event-stream version.
    pub const CURRENT: Self = Self(1);

    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}
