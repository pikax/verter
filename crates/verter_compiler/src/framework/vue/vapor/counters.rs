//! Name counters of one component's render body.

/// Allocator of the numbered names a render body uses.
///
/// Counters restart at zero for each component: a state is built per
/// component and owns its own counters.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct VaporCounters {
    /// Node and text references (`n0`, `n1`, ... and `x0`, `x1`, ...).
    node: u32,
    /// Navigation paths (`p0`, `p1`, ...).
    path: u32,
    /// Template declarations (`t0`, `t1`, ...).
    template: u32,
    memo: u32,
}

impl VaporCounters {
    pub(super) fn next_node(&mut self) -> u32 {
        Self::take(&mut self.node)
    }

    pub(super) fn next_path(&mut self) -> u32 {
        Self::take(&mut self.path)
    }

    pub(super) fn next_template(&mut self) -> u32 {
        Self::take(&mut self.template)
    }

    pub(super) fn next_memo(&mut self) -> u32 {
        Self::take(&mut self.memo)
    }

    fn take(counter: &mut u32) -> u32 {
        let value = *counter;
        *counter += 1;
        value
    }
}
