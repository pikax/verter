<script setup lang="ts">
// Full defineOptions patterns

defineOptions({
  name: "FullOptionsComponent",
  inheritAttrs: false,
  customOptions: {
    // Custom options for plugins
    permission: "admin",
  },
});
</script>

<template>
  <div>Full defineOptions</div>
</template>
