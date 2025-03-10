<script lang="ts">
export default {
  props: {
    bar: String,
  },
  data() {
    return {
      foo: "hello",
    };
  },
  methods: {
    calc() {
      this.foo;
    },
  },
};
const foo = 1;
</script>
<template>
  <span>{{ $data.foo + $props.bar }}</span>
  <span> {{ foo }} </span>
</template>
