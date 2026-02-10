use oxc_span::SourceType;

pub struct OxcParserPlugin<'alloc> {
    source_type: SourceType,

    alloc: &'alloc oxc_allocator::Allocator,

}
