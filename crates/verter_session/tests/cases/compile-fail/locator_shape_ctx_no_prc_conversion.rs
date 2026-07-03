//! Compile-FAIL fixture: the sealed `LocatorShapeCtx` neither contains nor
//! converts to a `ProjectionReductionContext` — no `From`/`Into`, no `AsRef`,
//! no `Deref` yields a PRC — so the reducing lowering entry (which REQUIRES a
//! `ProjectionReductionContext` argument) is unreachable from the locator
//! path BY TYPE. Each bound below fails E0277; if any conversion were ever
//! added, the matching line would COMPILE and trybuild would fail this
//! fixture.

use verter_session::semantic_query::ProjectionReductionContext;
use verter_session::LocatorShapeCtx;

fn requires_into_prc<T: Into<ProjectionReductionContext>>() {}
fn requires_asref_prc<T: AsRef<ProjectionReductionContext>>() {}
fn requires_deref_prc<T: std::ops::Deref<Target = ProjectionReductionContext>>() {}

fn main() {
    requires_into_prc::<LocatorShapeCtx<'static>>();
    requires_asref_prc::<LocatorShapeCtx<'static>>();
    requires_deref_prc::<LocatorShapeCtx<'static>>();
}
