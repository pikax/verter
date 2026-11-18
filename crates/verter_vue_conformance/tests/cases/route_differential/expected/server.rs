//! Accepted differences of the server target.

use super::{ExpectedDifference, ExpectedKind};

/// A template whose only node is a comment pushes that comment, as Vue
/// does; the in-place route pushes an empty fragment, `<!--[--><!--]-->`.
const COMMENT_ONLY_PLAIN: ExpectedKind = ExpectedKind::Corrected {
    reason:
        "a comment-only template pushes its comment (golden ssr/ssr/comment-only-template-plain.js)",
};

/// As [`COMMENT_ONLY_PLAIN`], with the backticks in the comment escaped for
/// the template literal.
const COMMENT_ONLY_ESCAPED: ExpectedKind = ExpectedKind::Corrected {
    reason: "a comment-only template pushes its comment with backticks escaped \
             (golden ssr/ssr/comment-only-template.js)",
};

/// A slot under `v-if` is a dynamic slot, built with `_createSlots` as Vue
/// does; the in-place route writes an `if` statement inside the slots object.
const CONDITIONAL_SLOT: ExpectedKind = ExpectedKind::Corrected {
    reason: "a conditional slot is built with _createSlots \
             (goldens ssr/ssr/conditional-slot.js, ssr/ssr/conditional-slot-static.js)",
};

/// A forwarded slot outlet in a server slot passes the slot scope id, as Vue
/// does; the in-place route drops it.
const SLOT_SCOPE_ID: ExpectedKind = ExpectedKind::Corrected {
    reason: "ssrRenderSlot receives _scopeId (golden ssr/components/dynamic-multi-root.js)",
};

/// Native children of server slot functions receive the inherited slot scope.
const NATIVE_SLOT_SCOPE_ID: ExpectedKind = ExpectedKind::Corrected {
    reason: "native slot children receive inherited _scopeId \
             (goldens ssr/components/slots.js and corrections/goldens/3.6.0-rc.15/server/server-slot-handler-one-statement.js)",
};

/// A statement-list handler in a slot's client branch is a block body, as Vue
/// writes it; the in-place route wraps the statements in parentheses, which
/// does not parse.
const STATEMENT_LIST_HANDLER: ExpectedKind = ExpectedKind::Corrected {
    reason: "a statement-list handler is a block body (golden ssr/ssr/slot-handler-statements.js)",
};

/// A slot branch under `v-if` imports `createCommentVNode` for its empty
/// branch, as Vue does; the in-place route leaves it out.
const BRANCH_COMMENT_HELPER: ExpectedKind = ExpectedKind::Corrected {
    reason:
        "a v-if slot branch imports _createCommentVNode (golden ssr/ssr/scoped-slot-branches.js)",
};

/// A server module of a component without a script declares its empty
/// component object without a semicolon, as Vue does; the in-place server
/// scaffold ends it with one.
const SCRIPTLESS_COMPONENT_OBJECT: ExpectedKind = ExpectedKind::Corrected {
    reason: "a script-less component object has no semicolon \
             (goldens ssr/elements-text/static-element.js, ssr/elements-text/multi-root.js)",
};

pub(super) const ROWS: &[ExpectedDifference] = &[
    ExpectedDifference {
        case: "cases/comment-only-root.vue",
        kind: ExpectedKind::Corrected {
            reason: "a comment-only root emits its authored comment \
                     (golden corrections/goldens/3.6.0-rc.15/server/comment-only-root.js)",
        },
    },
    ExpectedDifference {
        case: "cases/unscoped-forwarded-outlet.vue",
        kind: ExpectedKind::Corrected {
            reason: "an unscoped forwarded outlet passes inherited _scopeId and its props \
                     (golden corrections/goldens/3.6.0-rc.15/server/unscoped-forwarded-outlet.js)",
        },
    },
    ExpectedDifference {
        case: "cases/preformatted-line-ending.vue",
        kind: ExpectedKind::Corrected {
            reason: "a line ending inside preformatted content stays a newline \
                     (golden corrections/goldens/3.6.0-rc.15/server/preformatted-line-ending.js)",
        },
    },
    ExpectedDifference {
        case: "cases/foreign-pre-whitespace.vue",
        kind: ExpectedKind::Corrected {
            reason: "a `pre` under a MathML text integration point keeps its whitespace \
                     (golden corrections/goldens/3.6.0-rc.15/server/foreign-pre-whitespace.js)",
        },
    },
    ExpectedDifference {
        case: "cases/components/dynamic-multi-root.vue",
        kind: SLOT_SCOPE_ID,
    },
    ExpectedDifference {
        case: "cases/components/slots.vue",
        kind: NATIVE_SLOT_SCOPE_ID,
    },
    ExpectedDifference {
        case: "cases/elements-text/multi-root.vue",
        kind: SCRIPTLESS_COMPONENT_OBJECT,
    },
    ExpectedDifference {
        case: "cases/elements-text/static-class-style.vue",
        kind: SCRIPTLESS_COMPONENT_OBJECT,
    },
    ExpectedDifference {
        case: "cases/elements-text/static-element.vue",
        kind: SCRIPTLESS_COMPONENT_OBJECT,
    },
    ExpectedDifference {
        case: "cases/ssr/comment-only-template-plain.vue",
        kind: COMMENT_ONLY_PLAIN,
    },
    ExpectedDifference {
        case: "cases/ssr/comment-only-template.vue",
        kind: COMMENT_ONLY_ESCAPED,
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
        case: "cases/ssr/scoped-slot-branches.vue",
        kind: BRANCH_COMMENT_HELPER,
    },
    ExpectedDifference {
        case: "cases/ssr/slot-handler-statements.vue",
        kind: STATEMENT_LIST_HANDLER,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_comment_only_template.vue",
        kind: COMMENT_ONLY_PLAIN,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_comment_only_template_backticks.vue",
        kind: COMMENT_ONLY_ESCAPED,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_comment_only_template_with_script.vue",
        kind: COMMENT_ONLY_ESCAPED,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_conditional_slot_from_binding.vue",
        kind: CONDITIONAL_SLOT,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_conditional_slot_from_slots.vue",
        kind: CONDITIONAL_SLOT,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_nested_comment_backticks.vue",
        kind: SCRIPTLESS_COMPONENT_OBJECT,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_scoped_slot_if_else.vue",
        kind: BRANCH_COMMENT_HELPER,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_slot_handler_one_statement.vue",
        kind: NATIVE_SLOT_SCOPE_ID,
    },
    ExpectedDifference {
        case: "sfc_regression_fixtures/server_slot_handler_two_statements.vue",
        kind: STATEMENT_LIST_HANDLER,
    },
    ExpectedDifference {
        case: "cases/loop-source-compound-reads.vue",
        kind: ExpectedKind::Corrected {
            reason: "spread and callback reads in a loop source are prefixed \
                     (golden corrections/goldens/3.6.0-rc.15/server/loop-source-compound-reads.js)",
        },
    },
];
