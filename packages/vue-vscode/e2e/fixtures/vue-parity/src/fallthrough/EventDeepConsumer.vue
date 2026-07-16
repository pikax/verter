<script setup lang="ts">
import OuterWrap from "./OuterWrap.vue";

function onNativeClick() {
  // deep fallthrough should accept native listeners on the outer component
}
</script>

<template>
  <OuterWrap tone="primary" class="evt" @click="onNativeClick" />
</template>
