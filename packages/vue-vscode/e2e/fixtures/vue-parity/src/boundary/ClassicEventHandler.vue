<script>
export default {
  methods: {
    // @ts-expect-error classic JavaScript methods remain uncontextual
    handlePointer(domEvent) {
      return domEvent.notADomEventMember;
    },
  },
};
</script>

<template>
  <button type="button" @click="handlePointer">classic boundary</button>
</template>
