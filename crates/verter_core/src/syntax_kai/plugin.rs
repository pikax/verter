use crate::{syntax_kai::types::Event, utils::vue::is_void_tag};

/// Predicate for checking whether a tag is a custom element.
pub type IsCustomElementFn = Box<dyn Fn(&[u8]) -> bool>;

pub struct SyntaxPluginOptions {
    // if the tag does not need a closing tag
    pub is_void_tag: fn(&[u8]) -> bool,
    // if the tag is a custom element
    pub is_custom_element: IsCustomElementFn,
}

impl std::fmt::Debug for SyntaxPluginOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyntaxPluginOptions")
            .field("is_void_tag", &"fn(&[u8]) -> bool")
            .field("is_custom_element", &"Box<dyn Fn(&[u8]) -> bool>")
            .finish()
    }
}

impl std::default::Default for SyntaxPluginOptions {
    fn default() -> Self {
        Self {
            is_void_tag,
            is_custom_element: Box::new(|_tag_name: &[u8]| false),
        }
    }
}

pub struct SyntaxPluginContext<'a> {
    pub input: &'a str,
    pub bytes: &'a [u8],
    pub options: &'a SyntaxPluginOptions,
}

pub trait SyntaxPlugin<'a> {
    fn name(&self) -> &str;

    fn start(&mut self, _ctx: &SyntaxPluginContext<'a>) {}
    fn end(&mut self, _ctx: &SyntaxPluginContext<'a>) {}
    fn process_event(
        &mut self,
        event: Event<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<Event<'a>>;
}

pub enum SyntaxResult<E> {
    Keep(E),    // forward the original event (must pass it back)
    Replace(E), // forward a new event instead
    Drop,       // emit nothing
}

impl<E> SyntaxResult<E> {
    pub fn keep(ev: E) -> Self {
        Self::Keep(ev)
    }
    pub fn drop_() -> Self {
        Self::Drop
    }
    pub fn replace(ev: E) -> Self {
        Self::Replace(ev)
    }
}
