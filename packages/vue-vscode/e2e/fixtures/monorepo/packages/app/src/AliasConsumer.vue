<script setup lang="ts">
// Vue-FREE on purpose (mirrors external-ts-engine/AliasConsumer.vue): the ONLY
// resolution that can fail here is the cross-package project-binding mechanism
// this fixture characterizes — the app's referenced leaf configured project must
// be loaded with its `paths` to the sibling package wired. No `import "vue"` /
// `defineProps`, so a missing `vue` dependency can never contaminate the
// assertion.

// Cross-package TS import — resolves only when the app leaf project is loaded
// with its `paths` mapping to the sibling package.
import { helper } from "@monorepo/shared/src/utils";

const greeting = helper();

// Ambient global reached only via the app leaf tsconfig `types` + `typeRoots`.
const token = AMBIENT_TOKEN;
</script>
<template>
  <div>{{ greeting }} {{ token }}</div>
</template>
