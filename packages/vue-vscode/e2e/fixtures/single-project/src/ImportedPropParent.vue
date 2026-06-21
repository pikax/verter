<script setup lang="ts">
// Parent using a child whose props are an IMPORTED type
// (`defineProps<ImportedType>()`). Renaming the `headline` prop USAGE here must
// propagate cross-file to the imported type's member declaration in
// `childImportedProps.ts` — the tgo<->tsserver PARITY case.
import ImportedPropChild from "./ImportedPropChild.vue";
</script>
<template>
  <div>
    <ImportedPropChild headline="hi there" />
  </div>
</template>
