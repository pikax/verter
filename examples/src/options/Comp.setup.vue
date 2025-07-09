<script setup lang="ts">
defineOptions({
  data() {
    return { foo: "hello" };
  },
});

defineProps({
  bar: String,
});

let value = defineModel<boolean>("bar");
const baz = defineModel<{ jjj: boolean }>("baz", {
  default: () => ({ jjj: true }),
});

const foo = 1;

new ___VERTER___default().$props;
</script>
<template>
  <span>{{ $data.foo }}</span>

  <span>
    {{
      /*  @ts-expect-error */
      $props.modelValue
    }}
  </span>
  <span> {{ foo }} </span>

  <span>
    {{
      /* @ts-expect-error */
      $props.bar?.split("n")
    }}
  </span>
  <span>
    {{ $props.baz.jjj }}
  </span>
</template>
<style>
.foo {
  background: red;
  overflow: auto;

  display: flex;
}
</style>
<style lang="scss">
.bar {
  background: blue;
}
</style>
<style lang="less">
.baz {
  display: contents;
}
</style>
