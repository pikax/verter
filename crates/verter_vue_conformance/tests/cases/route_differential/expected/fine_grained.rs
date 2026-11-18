//! Accepted differences of the fine-grained client target.

use super::{ExpectedDifference, ExpectedKind};

/// A `<template>` branch whose only child is text is its text node: the
/// branch sets that node's text, as Vue does; the in-place route reads the
/// text node's own first child.
const TEXT_ROOT_BRANCH: ExpectedKind = ExpectedKind::Corrected {
    reason: "a text-only template branch sets its own text node \
             (golden vapor/ssr/scoped-slot-branches.js)",
};

/// A `v-if` slot template is an element of the component's dynamic slot
/// list, present only while its condition holds; the in-place route always
/// passes the slot.
const CONDITIONAL_SLOT: ExpectedKind = ExpectedKind::Corrected {
    reason: "a conditional slot template is a dynamic slot entry \
             (golden vapor/ssr/conditional-slot.js)",
};

pub(super) const ROWS: &[ExpectedDifference] = &[
    ExpectedDifference {
        case: "cases/ssr/scoped-slot-branches.vue",
        kind: TEXT_ROOT_BRANCH,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_scoped_slot_if_else.vue",
        kind: TEXT_ROOT_BRANCH,
    },
    ExpectedDifference {
        case: "cases/ssr/conditional-slot-static.vue",
        kind: CONDITIONAL_SLOT,
    },
    ExpectedDifference {
        case: "cases/ssr/conditional-slot.vue",
        kind: CONDITIONAL_SLOT,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_conditional_slot_from_binding.vue",
        kind: CONDITIONAL_SLOT,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_conditional_slot_from_slots.vue",
        kind: CONDITIONAL_SLOT,
    },
];
