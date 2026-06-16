<template>
  <p>{{ tally }} {{ message }}</p>
</template>

<script setup lang="ts">
import { ref } from "vue";

// A genuine type error: a string is assigned to a number-typed binding. The
// diagnostics surface MUST report this.
const tally: number = "not a number"; // @dx-anchor diag.realError

// A valid `let` reassignment that must NOT be spuriously flagged — it guards the
// historical false "invalid let" diagnostic regression.
let message = ref("hello").value; // @dx-anchor diag.validLet
message = "updated";
</script>
