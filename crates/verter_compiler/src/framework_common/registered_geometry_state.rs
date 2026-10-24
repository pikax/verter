/// Private proof that a framework artifact's inventory was produced by the
/// sealed registered projector.
#[derive(verter_retention::heap_size::HeapSize)]
pub(super) struct RegisteredGeometry {
    pub(super) _private: (),
}
