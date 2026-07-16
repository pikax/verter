<script setup lang="ts">
// Intentionally does NOT import PropChild — typing after TAG_COMPLETE_SITE offers auto-import.
const title = "auto";
</script>

<template>
  <div>
    <!-- TAG_COMPLETE_SITE -->
    <p>{{ title }}</p>
  </div>
</template>
