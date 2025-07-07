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

// const $props = {jjj: true}
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
