<script setup lang="ts">
import { ref } from "vue";
// 3-hop @/ chain through `export *`: alias -> components/index.ts (export *)
// -> components/layouts/index.ts -> PageContent.vue
import { PageContent } from "@/components";

const count = ref(0);
</script>
<template>
  <div>{{ count }}</div>
  <PageContent heading="Title" :padded="true" />
</template>
