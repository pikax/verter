use crate::runner::runner::RunnerResult;
use crate::tokenizer::byte::tokenize;
use crate::tokenizer::plugin::TokenizerOptions;
use crate::tokenizer::plugin::TokenizerPlugin;

pub struct TokenizerRunner<'a, 'bump> {
    bump: &'bump bumpalo::Bump,

    pipeline: Vec<Box<dyn TokenizerPlugin + 'bump>>,
    options: &'a TokenizerOptions,
}
impl<'a, 'bump> TokenizerRunner<'a, 'bump> {
    pub fn new(
        bump: &'bump bumpalo::Bump,
        options: &'a TokenizerOptions,
        pipeline: Vec<Box<dyn TokenizerPlugin + 'bump>>,
    ) -> Self {
        Self {
            bump,
            pipeline,
            options,
        }
    }

    pub fn run(&mut self, input: &'a str) {
        let bytes = input.as_bytes();

        let context = crate::tokenizer::plugin::TokenizerPluginContext {
            runner: &crate::runner::runner::RunnerContext::new(
                self.bump,
                input,
                bytes,
                self.options,
            ),
            position: crate::cursor::position::PositionResolver::new(input),
        };

        tokenize(bytes, |mut event| {
            for plugin in &mut self.pipeline {
                match plugin.process_event(&event, &context) {
                    RunnerResult::Replace(new_ev) => {
                        event = new_ev; // replace and continue pipeline
                    }
                    RunnerResult::Keep => {
                        // keep current `ev` as-is
                    }
                    RunnerResult::Drop => {
                        return; // stop processing and emit nothing
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::runner::RunnerResult;
    use crate::tokenizer::plugin::{TokenizerOptions, TokenizerPlugin, TokenizerPluginContext};
    use crate::tokenizer::Event;
    use bumpalo::Bump;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn default_options() -> TokenizerOptions {
        TokenizerOptions {
            keep_whitespace: false,
            lower_idents: false,

            delimiter_close_len: 2,
            delimiter_open_len: 2,
        }
    }

    /// A test plugin that collects all events it sees
    struct CollectorPlugin {
        events: Rc<RefCell<Vec<Event<'static>>>>,
    }

    impl CollectorPlugin {
        fn new(events: Rc<RefCell<Vec<Event<'static>>>>) -> Self {
            Self { events }
        }
    }

    impl TokenizerPlugin for CollectorPlugin {
        fn name(&self) -> &str {
            "collector"
        }

        fn process_event<'a, 'bump>(
            &mut self,
            event: &Event<'static>,
            _ctx: &TokenizerPluginContext<'a, 'bump>,
        ) -> RunnerResult<Event<'static>> {
            self.events.borrow_mut().push(event.clone());
            RunnerResult::Keep
        }
    }

    /// A plugin that drops all Text events
    struct TextDropperPlugin;

    impl TokenizerPlugin for TextDropperPlugin {
        fn name(&self) -> &str {
            "text_dropper"
        }

        fn process_event<'a, 'bump>(
            &mut self,
            event: &Event<'static>,
            _ctx: &TokenizerPluginContext<'a, 'bump>,
        ) -> RunnerResult<Event<'static>> {
            match event {
                Event::Text { .. } => RunnerResult::Drop,
                _ => RunnerResult::Keep,
            }
        }
    }

    /// A plugin that replaces Text events with modified spans
    struct TextModifierPlugin;

    impl TokenizerPlugin for TextModifierPlugin {
        fn name(&self) -> &str {
            "text_modifier"
        }

        fn process_event<'a, 'bump>(
            &mut self,
            event: &Event<'static>,
            _ctx: &TokenizerPluginContext<'a, 'bump>,
        ) -> RunnerResult<Event<'static>> {
            match event {
                Event::Text { start, end } => {
                    // Just modify by adding 1000 to start (for testing replacement)
                    RunnerResult::Replace(Event::Text {
                        start: start + 1000,
                        end: *end,
                    })
                }
                _ => RunnerResult::Keep,
            }
        }
    }

    #[test]
    fn test_runner_new() {
        let bump = Bump::new();
        let options = default_options();
        let pipeline: Vec<Box<dyn TokenizerPlugin>> = vec![];

        let runner = TokenizerRunner::new(&bump, &options, pipeline);
        assert!(runner.pipeline.is_empty());
    }

    #[test]
    fn test_runner_empty_pipeline() {
        let bump = Bump::new();
        let options = default_options();
        let pipeline: Vec<Box<dyn TokenizerPlugin>> = vec![];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        // Should not panic with empty pipeline
        runner.run("hello world");
    }

    #[test]
    fn test_runner_with_collector_simple_text() {
        let bump = Bump::new();
        let options = default_options();
        let events = Rc::new(RefCell::new(Vec::new()));

        let pipeline: Vec<Box<dyn TokenizerPlugin>> =
            vec![Box::new(CollectorPlugin::new(events.clone()))];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        runner.run("hello");

        let collected = events.borrow();
        // Should have at least a Text event and End event
        assert!(!collected.is_empty());
        assert!(collected.iter().any(|e| matches!(e, Event::Text { .. })));
        assert!(collected.iter().any(|e| matches!(e, Event::End)));
    }

    #[test]
    fn test_runner_with_collector_html_tag() {
        let bump = Bump::new();
        let options = default_options();
        let events = Rc::new(RefCell::new(Vec::new()));

        let pipeline: Vec<Box<dyn TokenizerPlugin>> =
            vec![Box::new(CollectorPlugin::new(events.clone()))];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        runner.run("<div>content</div>");

        let collected = events.borrow();
        // Should have OpenTagName, OpenTagEnd, Text, CloseTag, End
        assert!(collected
            .iter()
            .any(|e| matches!(e, Event::OpenTagName { .. })));
        assert!(collected
            .iter()
            .any(|e| matches!(e, Event::OpenTagEnd { .. })));
        assert!(collected.iter().any(|e| matches!(e, Event::Text { .. })));
        assert!(collected
            .iter()
            .any(|e| matches!(e, Event::CloseTag { .. })));
        assert!(collected.iter().any(|e| matches!(e, Event::End)));
    }

    #[test]
    fn test_runner_drop_text_events() {
        let bump = Bump::new();
        let options = default_options();
        let events = Rc::new(RefCell::new(Vec::new()));

        // First dropper, then collector - collector should not see Text events
        let pipeline: Vec<Box<dyn TokenizerPlugin>> = vec![
            Box::new(TextDropperPlugin),
            Box::new(CollectorPlugin::new(events.clone())),
        ];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        runner.run("hello world");

        let collected = events.borrow();
        // Text events should be dropped, collector should not see them
        assert!(!collected.iter().any(|e| matches!(e, Event::Text { .. })));
        // But End event should still be there
        assert!(collected.iter().any(|e| matches!(e, Event::End)));
    }

    #[test]
    fn test_runner_replace_text_events() {
        let bump = Bump::new();
        let options = default_options();
        let events = Rc::new(RefCell::new(Vec::new()));

        // First modifier, then collector - collector should see modified Text events
        let pipeline: Vec<Box<dyn TokenizerPlugin>> = vec![
            Box::new(TextModifierPlugin),
            Box::new(CollectorPlugin::new(events.clone())),
        ];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        runner.run("hello");

        let collected = events.borrow();
        // Check that Text events have modified start values (>= 1000)
        for event in collected.iter() {
            if let Event::Text { start, .. } = event {
                assert!(
                    *start >= 1000,
                    "Text event start should be >= 1000 after modification"
                );
            }
        }
    }

    #[test]
    fn test_runner_with_interpolation() {
        let bump = Bump::new();
        let options = default_options();
        let events = Rc::new(RefCell::new(Vec::new()));

        let pipeline: Vec<Box<dyn TokenizerPlugin>> =
            vec![Box::new(CollectorPlugin::new(events.clone()))];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        runner.run("{{ message }}");

        let collected = events.borrow();
        // Should have Interpolation event
        assert!(collected
            .iter()
            .any(|e| matches!(e, Event::Interpolation { .. })));
    }

    #[test]
    fn test_runner_with_directive() {
        let bump = Bump::new();
        let options = default_options();
        let events = Rc::new(RefCell::new(Vec::new()));

        let pipeline: Vec<Box<dyn TokenizerPlugin>> =
            vec![Box::new(CollectorPlugin::new(events.clone()))];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        runner.run("<div v-if=\"show\"></div>");

        let collected = events.borrow();
        // Should have DirName event for v-if
        assert!(collected.iter().any(|e| matches!(e, Event::DirName { .. })));
    }

    #[test]
    fn test_runner_self_closing_tag() {
        let bump = Bump::new();
        let options = default_options();
        let events = Rc::new(RefCell::new(Vec::new()));

        let pipeline: Vec<Box<dyn TokenizerPlugin>> =
            vec![Box::new(CollectorPlugin::new(events.clone()))];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        runner.run("<br/>");

        let collected = events.borrow();
        // Should have SelfClosingTag event
        assert!(collected
            .iter()
            .any(|e| matches!(e, Event::SelfClosingTag { .. })));
    }

    #[test]
    fn test_runner_comment() {
        let bump = Bump::new();
        let options = default_options();
        let events = Rc::new(RefCell::new(Vec::new()));

        let pipeline: Vec<Box<dyn TokenizerPlugin>> =
            vec![Box::new(CollectorPlugin::new(events.clone()))];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        runner.run("<!-- comment -->");

        let collected = events.borrow();
        // Should have Comment event
        assert!(collected.iter().any(|e| matches!(e, Event::Comment { .. })));
    }

    #[test]
    fn test_runner_multiple_plugins_pipeline_order() {
        let bump = Bump::new();
        let options = default_options();
        let events1 = Rc::new(RefCell::new(Vec::new()));
        let events2 = Rc::new(RefCell::new(Vec::new()));

        // Two collectors to verify both receive events
        let pipeline: Vec<Box<dyn TokenizerPlugin>> = vec![
            Box::new(CollectorPlugin::new(events1.clone())),
            Box::new(CollectorPlugin::new(events2.clone())),
        ];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        runner.run("test");

        let collected1 = events1.borrow();
        let collected2 = events2.borrow();

        // Both collectors should receive the same events
        assert_eq!(collected1.len(), collected2.len());
        for (e1, e2) in collected1.iter().zip(collected2.iter()) {
            assert_eq!(e1, e2);
        }
    }

    #[test]
    fn test_runner_empty_input() {
        let bump = Bump::new();
        let options = default_options();
        let events = Rc::new(RefCell::new(Vec::new()));

        let pipeline: Vec<Box<dyn TokenizerPlugin>> =
            vec![Box::new(CollectorPlugin::new(events.clone()))];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        runner.run("");

        let collected = events.borrow();
        // Should only have End event
        assert_eq!(collected.len(), 1);
        assert!(matches!(collected[0], Event::End));
    }

    #[test]
    fn test_runner_with_attributes() {
        let bump = Bump::new();
        let options = default_options();
        let events = Rc::new(RefCell::new(Vec::new()));

        let pipeline: Vec<Box<dyn TokenizerPlugin>> =
            vec![Box::new(CollectorPlugin::new(events.clone()))];

        let mut runner = TokenizerRunner::new(&bump, &options, pipeline);
        runner.run("<div class=\"container\" id=\"main\"></div>");

        let collected = events.borrow();
        // Should have AttribName and AttribData events
        assert!(collected
            .iter()
            .any(|e| matches!(e, Event::AttribName { .. })));
        assert!(collected
            .iter()
            .any(|e| matches!(e, Event::AttribData { .. })));
    }
}
