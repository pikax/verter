//! Accepted differences of the client tree target.

use super::{ExpectedDifference, ExpectedKind};

/// A handler that reads a template-scope name is passed fresh on every
/// render, as Vue does; the in-place route stores it in a component-level
/// `_cache` slot, so every row reuses the first row's handler.
const SCOPE_BOUND_HANDLERS: ExpectedKind = ExpectedKind::Corrected {
    reason: "a handler reading a v-for alias or v-slot parameter is uncached and a dynamic prop \
             (goldens corrections/goldens/3.6.0-rc.15/client*/loop-scoped-handlers.js, \
             slot-scoped-handlers.js, merged-scope-listener.js)",
};

/// A spread makes the element FULL_PROPS alone: the dynamic listener it
/// carries adds neither PROPS nor NEED_PATCH, and the bound names still follow
/// the flag.
const SPREAD_FULL_PROPS: ExpectedKind = ExpectedKind::Corrected {
    reason: "a spread element is `16 /* FULL_PROPS */` with its bound-name list \
             (goldens corrections/goldens/3.6.0-rc.15/client*/spread-scope-listener.js)",
};

/// A context binding read inside a compound loop source (a spread element or
/// a callback body) goes through the context; the in-place route leaves it
/// bare.
const COMPOUND_LOOP_SOURCE: ExpectedDifference = ExpectedDifference {
    case: "cases/loop-source-compound-reads.vue",
    kind: ExpectedKind::Corrected {
        reason: "spread and callback reads in a loop source are prefixed \
                 (goldens corrections/goldens/3.6.0-rc.15/client*/loop-source-compound-reads.js)",
    },
};

pub(super) const ROWS: &[ExpectedDifference] = &[
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_slot_handler_one_statement.vue",
        kind: ExpectedKind::Corrected {
            reason:
                "a single-assignment handler is an expression body, `$event => ($setup.a = '')`, \
                     as Vue writes an inline expression handler (golden vdom/v-on/inline.js: \
                     `$event => ($setup.count++)`); the in-place route writes a block body",
        },
    },
    ExpectedDifference {
        case: "cases/loop-scoped-handlers.vue",
        kind: SCOPE_BOUND_HANDLERS,
    },
    ExpectedDifference {
        case: "cases/slot-scoped-handlers.vue",
        kind: SCOPE_BOUND_HANDLERS,
    },
    ExpectedDifference {
        case: "cases/merged-scope-listener.vue",
        kind: SCOPE_BOUND_HANDLERS,
    },
    ExpectedDifference {
        case: "cases/spread-scope-listener.vue",
        kind: SPREAD_FULL_PROPS,
    },
    ExpectedDifference {
        case: "cases/preformatted-line-ending.vue",
        kind: ExpectedKind::Corrected {
            reason: "a line ending inside preformatted content stays a newline \
                     (golden corrections/goldens/3.6.0-rc.15/client/preformatted-line-ending.js)",
        },
    },
    ExpectedDifference {
        case: "cases/foreign-pre-whitespace.vue",
        kind: ExpectedKind::Corrected {
            reason: "a constant interpolation carries no TEXT flag; only an HTML `pre` keeps its whitespace \
                     (golden corrections/goldens/3.6.0-rc.15/client/foreign-pre-whitespace.js)",
        },
    },
    COMPOUND_LOOP_SOURCE,
];

pub(super) const INLINE_ROWS: &[ExpectedDifference] = &[
    ROWS[0],
    ROWS[1],
    ROWS[2],
    ROWS[3],
    ROWS[4],
    ExpectedDifference {
        case: "cases/inline-maybe-ref.vue",
        kind: ExpectedKind::Corrected {
            reason: "maybe-ref reads use unref and model writes guard with isRef \
                     (golden corrections/goldens/3.6.0-rc.15/client-inline/inline-maybe-ref.js)",
        },
    },
    COMPOUND_LOOP_SOURCE,
];
