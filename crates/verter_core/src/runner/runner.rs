use bumpalo::Bump;

use crate::common::Span;

pub struct RunnerContext<'a, 'bump, Options = ()> {
    pub bump: &'bump Bump,
    pub input: &'a str,
    pub bytes: &'a [u8],
    pub options: &'a Options,
}

impl<'a, 'bump, Options> RunnerContext<'a, 'bump, Options> {
    pub fn new(bump: &'bump Bump, input: &'a str, bytes: &'a [u8], options: &'a Options) -> Self {
        Self {
            bump,
            input,
            bytes,
            options,
        }
    }

    pub fn bump(&self) -> &'bump Bump {
        self.bump
    }

    pub fn input(&self) -> &'a str {
        self.input
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn options(&self) -> &Options {
        self.options
    }

    pub fn slice(&self, span: Span) -> &'a str {
        &self.input[span.start as usize..span.end as usize]
    }
}

pub enum RunnerResult<E> {
    Keep,       // forward the original event unchanged
    Replace(E), // forward a new event instead
    Drop,       // emit nothing
}

impl<E> RunnerResult<E> {
    pub fn keep() -> Self {
        Self::Keep
    }
    pub fn drop_() -> Self {
        Self::Drop
    }
    pub fn replace(ev: E) -> Self {
        Self::Replace(ev)
    }
}

pub struct PluginContext<'a, 'bump, Options> {
    pub runner: RunnerContext<'a, 'bump, Options>,
}

pub trait RunnerPlugin {
    type Options;
    type Event<'bump>;

    fn name(&self) -> &str;

    fn start<'a, 'bump>(&mut self, _ctx: &RunnerContext<'a, 'bump, Self::Options>) {}
    fn end<'a, 'bump>(&mut self, _ctx: &RunnerContext<'a, 'bump, Self::Options>) {}

    fn process_event<'a, 'bump>(
        &mut self,
        event: &Self::Event<'bump>,
        ctx: &PluginContext<'a, 'bump, Self::Options>,
    ) -> RunnerResult<Self::Event<'bump>>;
}
