# Component-Meta Trace Progress

## Methodology

Each component is traced with a 40s hard timeout using `packages/benchmark/src/_trace-component.ts`.
Traces are validated against desired trace specs under `packages/benchmark/trace-specs/component-meta/`.
A component is "done" when its trace passes validation and workspace tests pass.

## Full Corpus Results (120 components)

### Summary

| Category | Count |
|----------|-------|
| Fast (<1s) | 54 |
| Medium (1-5s) | 39 |
| Slow (5-10s) | 15 |
| Very slow (10-20s) | 8 |
| Failed/timed out | 4 |
| **Total** | **120** |

### Failed/No-result Components

| Component | Issue | Trace lines |
|-----------|-------|-------------|
| Input | Query returned no result (Closed without Done) | 3754 |
| Select | Query returned no result (Closed without Done) | 8315 |
| Table | Timed out at 40s (25K trace lines, still resolving) | 24897 |
| Textarea | Query returned no result (Closed without Done) | 2925 |

### Slowest Components (>5s)

| Component | Duration (ms) | Props |
|-----------|--------------|-------|
| EditorSuggestionMenu | 18449 | 11 |
| EditorMentionMenu | 18343 | 13 |
| EditorEmojiMenu | 18330 | 11 |
| DropdownMenuContent | 15634 | 35 |
| EditorToolbar | 12305 | 8 |
| Toast | 12025 | 17 |
| InputMenu | 11075 | 63 |
| SelectMenu | 10894 | 54 |
| NavigationMenu | 10044 | 30 |
| ContextMenuContent | 9582 | 25 |
| DropdownMenu | 9617 | 20 |
| ContextMenu | 9314 | 14 |
| Popover | 8608 | 13 |
| DashboardSearch | 7784 | 25 |
| DashboardSidebar | 7481 | 17 |

### Common Bottleneck

All slow components share the same pattern: `compute_evaluated_types_expand_macros` dominates
(>90% of total time). The type solver performs deep recursive resolution through reka-ui and
Vue's type definition barrels. The overhead is inherent to the type complexity, not a cache miss.

## Desired Trace Specs

Committed specs exist for:
- Batch 1: Accordion, Alert, App
- Batch 2: AuthForm, Avatar, AvatarGroup

## Artifact Directories

| Directory | Description |
|-----------|-------------|
| `tmp/batch1-trace-001/` | Batch 1 baseline |
| `tmp/batch1-trace-002/` | After FileWholeHash acceptance fix |
| `tmp/batch2-trace-001/` | Batch 2 traces |
| `tmp/batch3-5-trace/` | All remaining components (batches 3-end) |
