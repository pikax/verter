//! Heap measurement of compiler- and language-owned records a session
//! cache retains, for the `#[heap_size(with = "...")]` fields carrying them.

use verter_retention::allocation::heap_block_bytes;
use verter_retention::heap_size::HeapMeter;

/// A diagnostic's argument list and the text its arguments own.
pub(crate) fn diagnostic_args<'t>(
    args: &'t Vec<verter_language::DiagnosticArg>,
    _meter: &mut HeapMeter<'_, 't>,
) -> usize {
    heap_block_bytes(args.capacity() * std::mem::size_of::<verter_language::DiagnosticArg>())
        + args
            .iter()
            .map(|arg| match arg {
                verter_language::DiagnosticArg::Text(text) => heap_block_bytes(text.capacity()),
                verter_language::DiagnosticArg::Bool(_)
                | verter_language::DiagnosticArg::Unsigned(_)
                | verter_language::DiagnosticArg::Signed(_)
                | verter_language::DiagnosticArg::Span { .. } => 0,
            })
            .sum::<usize>()
}

/// A destructured block's binding list and its names.
pub(crate) fn destructured_block<'t>(
    block: &'t Option<verter_compiler::compile::types::DestructuredBlockMeta>,
    _meter: &mut HeapMeter<'_, 't>,
) -> usize {
    block.as_ref().map_or(0, |block| {
        heap_block_bytes(
            block.bindings.capacity()
                * std::mem::size_of::<verter_compiler::compile::types::DestructuredBindingInfo>(),
        ) + block
            .bindings
            .iter()
            .map(|binding| heap_block_bytes(binding.name.capacity()))
            .sum::<usize>()
    })
}
