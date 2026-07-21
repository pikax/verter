<script setup lang="ts">
// <script setup> consumer of the globally-registered GlobalCountComp (see
// global-components.d.ts). Never imported: the tag types through the
// GlobalComponents fallback const — PascalCase, kebab-case, and
// `<component :is>` string form.
const pingMsg = "ready";
</script>

<template>
  <div>
    <span>{{ pingMsg }}</span>
    <GlobalCountComp :count="42" />
    <global-count-comp :count="7" />
    <component :is="'GlobalCountComp'" :count="9" />
    <GlobalCountComp :count="'mistyped'" />
  </div>
</template>
