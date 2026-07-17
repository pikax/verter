<script lang="ts">
export default {
  methods: {
    // @ts-expect-error classic TypeScript methods remain uncontextual
    handlePointer(domEvent) {
      return domEvent.notADomEventMember;
    },
  },
};
</script>

<template>
  <button type="button" @click="handlePointer">classic TypeScript boundary</button>
</template>
