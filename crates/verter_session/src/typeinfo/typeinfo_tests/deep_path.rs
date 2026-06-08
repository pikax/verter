//! @ai-generated - Synthetic deep indexed-access path typeinfo tests.

use super::oracle;
use verter_session_oracle_macro::oracle_row;

// LIFTED: `DeepProjectedTarget` reduces a 16-hop indexed-access chain
// (`DeepRoot["level14"]…["target"]`) to the terminal `TerminalPayload` object
// `{ id: string; priority: 1 | 2 | 3 }` WITHOUT losing shape. The lifted body is
// the registry-keyed `oracle::run_row` shared-driver call the `#[oracle_row]`
// macro synthesizes: it resolves Verter's `Expanded` projection and compares it
// against the checked-in tsgo snapshot. The DAG-terminal producer is
// `IndexedAccessUnionDistribution` (block `U2.INDEXED_ACCESS`); the measured
// dispatch trace is `[IndexedAccess, Instantiate, ResolveDecl]`, proven live by
// `lifted_row_mechanism_trace_matches_manifest`.
#[oracle_row]
#[test]
fn deep_path_projection_resolves_terminal_without_losing_shape() {}
