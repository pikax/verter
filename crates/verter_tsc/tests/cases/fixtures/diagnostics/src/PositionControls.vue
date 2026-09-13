<template>
  <div>{{ label }}</div>
</template>
<script setup lang="ts">
// Position controls (pins live in diagnostics.rs and
// diagnostic_set_parity.rs):
// - Block offset: the script follows the template, so the authored full-SFC
//   line differs from both the generated carrier line and a block-relative
//   line; only full-SFC coordinates are correct.
// - Non-BMP: U+1D11E before each anchor is 2 UTF-16 units / 4 UTF-8 bytes /
//   1 codepoint, so a column counted in anything but UTF-16 units lands on a
//   different character.
// PINNED: line 20 TS2322 at the declarator `n` (col 17); line 21 TS2345 at
// the ref<string>(7) argument `7` (col 40) — the argument anchor has no
// projection-map token of its own, so its exact column survives only through
// verbatim-line column recovery (without it the diagnostic degrades to the
// covering declarator anchor's column 19).
import { ref } from "vue";
const label = "ok";
const t = "𝄞", n: string = 7;
const tag = "𝄞", strict = ref<string>(7);
void t;
void n;
void tag;
void strict;
</script>
