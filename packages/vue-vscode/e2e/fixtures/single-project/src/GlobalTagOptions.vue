<script lang="ts">
// Options-API consumer of the globally-registered GlobalCountComp (see
// global-components.d.ts). The component is NEVER imported here: the tag must
// type through the synthesized GlobalComponents fallback const in the
// Options-API arm — PascalCase, kebab-case, and `<component :is>` string form.
export default {
  name: "GlobalTagOptions",
};
</script>

<template>
  <div>
    <GlobalCountComp :count="42" @ping="(evtPayload) => { void evtPayload; }" />
    <global-count-comp :count="7" />
    <component :is="'GlobalCountComp'" :count="9" />
    <GlobalCountComp :count="'mistyped'" />
  </div>
</template>
