<script setup lang="ts">
import { ref, onMounted } from "vue";
import ExposeChild from "./ExposeChild.vue";

const childRef = ref<InstanceType<typeof ExposeChild> | null>(null);

onMounted(() => {
  childRef.value?.bumpExposed();
});
</script>

<template>
  <ExposeChild ref="childRef" />
</template>
