//! The lexical environment of generic binders one type-parameter
//! normalization walks under.
//!
//! Binders are introduced one at a time onto a single stack, in source
//! order, and a name index lists, per spelling, the stack positions that
//! introduce it. A scope is a contiguous run of that stack: entering one
//! records the current depth, leaving it releases everything introduced
//! since. Which binders a position sees is a floor on the stack — every
//! live binder at or above it — so a nested function's own declaration list
//! hides the scopes enclosing it by raising the floor, without copying
//! anything. Introducing, resolving and releasing a binder each touch only
//! that binder's own name entry.

use std::collections::HashMap;

use verter_type_expr::TypeParam;

/// Live generic binders, indexed by name. It lives for one normalization.
#[derive(Default)]
pub(crate) struct BinderEnv {
    /// Every live binder, in introduction order.
    binders: Vec<TypeParam>,
    /// Per spelling, the ascending stack positions of the live binders
    /// introducing it.
    by_name: HashMap<String, Vec<usize>>,
}

impl BinderEnv {
    /// The stack depth: the position the next introduced binder takes, and
    /// the floor that makes only binders introduced from now on visible.
    pub(crate) fn depth(&self) -> usize {
        self.binders.len()
    }

    /// Introduce `param` above every live binder.
    pub(crate) fn introduce(&mut self, param: TypeParam) {
        #[cfg(test)]
        work::record(|work| work.introductions += 1);
        let position = self.binders.len();
        match self.by_name.get_mut(param.name.as_str()) {
            Some(positions) => positions.push(position),
            None => {
                self.by_name.insert(param.name.clone(), vec![position]);
            }
        }
        self.binders.push(param);
    }

    /// Leave the scope that began at depth `start`: release every binder
    /// introduced since, returned in introduction order.
    pub(crate) fn release(&mut self, start: usize) -> Vec<TypeParam> {
        let released = self.binders.split_off(start);
        for param in released.iter().rev() {
            if let Some(positions) = self.by_name.get_mut(param.name.as_str()) {
                positions.pop();
                if positions.is_empty() {
                    self.by_name.remove(param.name.as_str());
                }
            }
        }
        released
    }

    /// The binder a reference named `name` resolves to among those visible
    /// from `floor` up: of that name, the one in the outermost visible
    /// scope, and within it the first introduced — the lowest position at
    /// or above the floor.
    pub(crate) fn resolve(&self, name: &str, floor: usize) -> Option<&TypeParam> {
        #[cfg(test)]
        work::record(|work| work.lookups += 1);
        let positions = self.by_name.get(name)?;
        #[cfg(test)]
        work::record(|work| work.positions_examined += positions.len());
        let first_visible = positions.partition_point(|&position| position < floor);
        positions
            .get(first_visible)
            .map(|&position| &self.binders[position])
    }
}

/// Work counters over every [`BinderEnv`] on the current thread, for tests
/// that bound how normalization work grows with the number of binders.
#[cfg(test)]
pub(crate) mod work {
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct BinderWork {
        /// Binders introduced.
        pub(crate) introductions: usize,
        /// References resolved through the name index.
        pub(crate) lookups: usize,
        /// Same-name index positions a lookup could consider, summed: an
        /// upper bound on the positions its search reads.
        pub(crate) positions_examined: usize,
    }

    thread_local! {
        static WORK: Cell<BinderWork> = const { Cell::new(BinderWork {
            introductions: 0,
            lookups: 0,
            positions_examined: 0,
        }) };
    }

    pub(super) fn record(update: impl FnOnce(&mut BinderWork)) {
        WORK.with(|cell| {
            let mut work = cell.get();
            update(&mut work);
            cell.set(work);
        });
    }

    /// The work `run` performs on this thread.
    pub(crate) fn measure<R>(run: impl FnOnce() -> R) -> (R, BinderWork) {
        let before = WORK.with(Cell::get);
        let result = run();
        let after = WORK.with(Cell::get);
        (
            result,
            BinderWork {
                introductions: after.introductions - before.introductions,
                lookups: after.lookups - before.lookups,
                positions_examined: after.positions_examined - before.positions_examined,
            },
        )
    }
}
