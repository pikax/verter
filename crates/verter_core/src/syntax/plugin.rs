use crate::syntax::types::SyntaxEvent;

#[derive(Debug)]
pub struct SyntaxPluginOptions {
    // if the tag does not need a closing tag
    pub is_void_tag: fn(&[u8]) -> bool,
    // if the tag is a custom element
    pub is_custom_element: fn(&[u8]) -> bool,
}
impl std::default::Default for SyntaxPluginOptions {
    fn default() -> Self {
        Self {
            is_void_tag: |tag_name: &[u8]| {
                matches!(
                    tag_name,
                    b"area"
                        | b"base"
                        | b"br"
                        | b"col"
                        | b"embed"
                        | b"hr"
                        | b"img"
                        | b"input"
                        | b"link"
                        | b"meta"
                        | b"param"
                        | b"source"
                        | b"track"
                        | b"wbr"
                )
            },

            is_custom_element: |_tag_name: &[u8]| false,
        }
    }
}

pub struct SyntaxPluginContext<'a> {
    pub input: &'a str,
    pub bytes: &'a [u8],
    pub options: &'a SyntaxPluginOptions,
}

impl<'a> SyntaxPluginContext<'a> {
    pub fn new(input: &'a str, bytes: &'a [u8], options: &'a SyntaxPluginOptions) -> Self {
        Self {
            input,
            bytes,
            options,
        }
    }

    pub fn slice(&self, start: u32, end: u32) -> &'a str {
        &self.input[start as usize..end as usize]
    }
}

pub trait SyntaxPlugin<'a> {
    fn name(&self) -> &str;

    fn start(&mut self, _ctx: &SyntaxPluginContext<'a>) {}
    fn end(&mut self, _ctx: &SyntaxPluginContext<'a>) {}
    fn process_event(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>>;
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
