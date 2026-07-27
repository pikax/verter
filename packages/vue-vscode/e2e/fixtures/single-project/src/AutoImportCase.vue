<script setup lang="ts">
// `ref` is imported; `computed` is intentionally NOT imported, so the type
// provider offers it as an auto-import completion. Accepting it must insert
// `import { computed } from "vue"` (or merge it into the existing vue import)
// via completionItem/resolve — the provider-neutral carrier re-anchor.
//
// The call below is written as an INCOMPLETE identifier prefix rather than the
// finished name: the acceptance test puts the cursor at the end of that prefix
// so the suggestion widget filters down to a single candidate and the accept
// resolves exactly it. Spelled in full there is nothing left to filter.
//
// Nothing in this comment may repeat the call text verbatim — the test locates
// its cursor with a plain substring search over the whole file, so a prose copy
// of the anchor wins over the code and the probe lands in a comment.
import { ref } from "vue";

const base = ref(1);
const doubled = comput(() => base.value * 2);
</script>

<template>
  <div>{{ doubled }}</div>
</template>
