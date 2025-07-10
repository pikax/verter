<script lang="ts">
import Comp from "./Comp.vue";
import { defineComponent } from "vue";

const c = new Comp();

// const Foo;
const Foo = defineComponent({
  props: undefined,
  data() {
    return {};
  },
});

const f = new Foo();
</script>
<template></template>
