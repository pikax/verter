//! A transform created with map capture, as a type.
//!
//! A [`CodeTransform`] asked for a map product it did not capture refuses
//! with [`CodeTransformError::MapNotCaptured`](super::CodeTransformError::MapNotCaptured).
//! An owner that creates its own transform with capture cannot meet that
//! refusal, so it holds a [`MappedTransform`] instead: the same transform
//! (every edit goes through it unchanged, by `Deref`), whose map products
//! are infallible. They share the one implementation with the checked
//! products; only the capture check is settled by construction.

use std::ops::{Deref, DerefMut};

use oxc_allocator::Allocator;
use oxc_sourcemap::SourceMap;

use super::code_transform::{CodeTransform, GeneratedSourceRange};
use super::mapping_product::MappingProduct;
use super::source_map::SourceMapOptions;

/// A [`CodeTransform`] that captured map facts from its creation.
pub struct MappedTransform<
    'a,
    M: verter_audit::observe::ObserveExec = verter_audit::observe::Uncaptured,
> {
    inner: CodeTransform<'a, M>,
}

impl<'a> CodeTransform<'a> {
    /// An uncaptured-observation transform that captures map facts.
    pub fn new_mapped(source: &'a str, allocator: &'a Allocator) -> MappedTransform<'a> {
        CodeTransform::new_mapped_in(source, allocator)
    }
}

impl<'a, M: verter_audit::observe::ObserveExec> CodeTransform<'a, M> {
    /// A transform compiled for `M` that captures map facts.
    pub fn new_mapped_in(source: &'a str, allocator: &'a Allocator) -> MappedTransform<'a, M> {
        MappedTransform {
            inner: CodeTransform::with_mode_in(source, allocator, crate::emission::MapMode::Map),
        }
    }
}

impl<'a, M: verter_audit::observe::ObserveExec> Deref for MappedTransform<'a, M> {
    type Target = CodeTransform<'a, M>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<M: verter_audit::observe::ObserveExec> DerefMut for MappedTransform<'_, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<M: verter_audit::observe::ObserveExec> MappedTransform<'_, M> {
    /// See [`CodeTransform::generate_map`].
    #[must_use]
    pub fn generate_map(&self, options: SourceMapOptions) -> SourceMap<'static> {
        self.inner.captured_map_product(options).0
    }

    /// See [`CodeTransform::generate_map_with_preamble`].
    #[must_use]
    pub fn generate_map_with_preamble(
        &self,
        options: SourceMapOptions,
    ) -> (SourceMap<'static>, Option<(u32, u32)>) {
        let (map, preamble_end, _) = self.inner.captured_map_product(options);
        (map, preamble_end)
    }

    /// See [`CodeTransform::generate_map_json`].
    #[must_use]
    pub fn generate_map_json(&self, options: SourceMapOptions) -> String {
        self.generate_map(options).to_json_string()
    }

    /// See [`CodeTransform::generate_map_json_with_preamble`].
    #[must_use]
    pub fn generate_map_json_with_preamble(&self, options: SourceMapOptions) -> String {
        self.inner.captured_map_json_with_preamble(options)
    }

    /// See [`CodeTransform::build_string_with_source_ranges`].
    #[must_use]
    pub fn build_string_with_source_ranges(&self) -> (String, Vec<GeneratedSourceRange>) {
        let (output, ranges, _) = self.inner.captured_source_ranges();
        (output, ranges)
    }

    /// See [`CodeTransform::build_string_with_source_ranges_and_unmapped_boundaries`].
    #[must_use]
    pub(crate) fn build_string_with_source_ranges_and_unmapped_boundaries(
        &self,
    ) -> (String, Vec<GeneratedSourceRange>, Vec<u32>) {
        self.inner.captured_source_ranges()
    }

    /// See [`MappingProduct::of`].
    #[must_use]
    pub fn mapping_product(&self) -> MappingProduct {
        MappingProduct::of_captured(&self.inner)
    }
}
