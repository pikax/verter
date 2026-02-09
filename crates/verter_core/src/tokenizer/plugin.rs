use crate::cursor::position::PositionResolver;
use crate::runner::runner::{RunnerContext, RunnerResult};
use crate::tokenizer::Event as TokenizerEvent;

#[derive(Debug)]
pub struct TokenizerOptions {
    // TODO add more information here
    pub keep_whitespace: bool,
    pub lower_idents: bool,

    pub delimiter_open_len: usize,
    pub delimiter_close_len: usize,
}

/// Tokenizer plugin context: it *uses RunnerContext* (borrows it) and can add tokenizer-specific helpers.
pub struct TokenizerPluginContext<'a, 'bump> {
    pub runner: &'a RunnerContext<'a, 'bump, TokenizerOptions>,
    pub position: PositionResolver<'a>,
}
impl<'a, 'bump> TokenizerPluginContext<'a, 'bump> {
    pub fn runner(&self) -> &RunnerContext<'a, 'bump, TokenizerOptions> {
        self.runner
    }
    pub fn position(&self) -> &PositionResolver<'a> {
        &self.position
    }
}

/// Specialized tokenizer plugin trait: types are already defined.
pub trait TokenizerPlugin {
    fn name(&self) -> &str;

    fn start<'a, 'bump>(&mut self, _ctx: &RunnerContext<'a, 'bump, TokenizerOptions>) {}
    fn end<'a, 'bump>(&mut self, _ctx: &RunnerContext<'a, 'bump, TokenizerOptions>) {}

    fn process_event<'a, 'bump>(
        &mut self,
        event: &TokenizerEvent<'static>,
        ctx: &TokenizerPluginContext<'a, 'bump>,
    ) -> RunnerResult<TokenizerEvent<'static>>;
}
