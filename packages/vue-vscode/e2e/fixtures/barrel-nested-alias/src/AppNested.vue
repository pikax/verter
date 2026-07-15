<script setup lang="ts">
import { ref } from "vue";
// 2-hop @/ chain: alias -> components/layouts/index.ts -> PageContent.vue
import { PageContent } from "@/components/layouts";

const count = ref(0);
</script>
<template>
  <div>{{ count }}</div>
  <PageContent heading="Title" :padded="true" />
</template>
