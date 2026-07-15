<script setup lang="ts">
import { ref } from "vue";
// Cross-project component carrier: app imports the ui project's component
// through a composite project reference + `@ui/*` path alias.
import UiButton from "@ui/UiButton.vue";

const count = ref(0);
</script>
<template>
  <div>{{ count }}</div>
  <UiButton uiButtonOnly="a" />
</template>
