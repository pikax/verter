#!/usr/bin/env node
// Retired at the Rev11 authority-root cutover. The legacy helper mutated the
// pre-cutover DAG and ledger directly, which is forbidden under live ORC0.
process.stderr.write("apply-tcm-amendment is retired; use docs/arch/refactor/rev11/tools/programctl.mjs and the live amendment lifecycle\n");
process.exitCode = 2;
