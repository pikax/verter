<script lang="ts">
defineOptions({
  data() {
    return { foo: "hello" };
  },
});

export default {
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
  <span>{{ $data.foo }}</span>
  <span> {{ foo }} </span>
</template>
