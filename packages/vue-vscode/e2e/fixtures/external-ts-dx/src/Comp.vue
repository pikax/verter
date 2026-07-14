<script setup lang="ts">
// A Vue SFC whose public surface (its `$props`) carries a uniquely-named prop.
// A plain `.ts` importer of this file must see `verterDxHeadline` on the
// component's prop surface — the §2.9 "real public surface flows into the .ts"
// contract.
defineProps<{ verterDxHeadline: string }>();
</script>

<template>
  <div>{{ verterDxHeadline }}</div>
</template>
