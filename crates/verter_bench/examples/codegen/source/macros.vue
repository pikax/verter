<template>
  <div>
    <h1>{{ title }}</h1>
    <p>{{ description }}</p>
  </div>
</template>
<script setup lang="ts">
const props = defineProps<{
  titleXX?: string;
  description: string;
}>();
/*
const props = withDefaults(
  defineProps<{
    title: string;
    description: string;
  }>(),
  { title: "my-title" },
);*/

// const props = defineProps({ foo: String});

//const props = defineProps(['foo']);

/*defineEmits<{
  (e: "update"): void;
}>();*/
let emit = defineEmits<{
  updateXX: [true];
}>();

/*defineOptions({
  name: "MyComponent",
});*/

defineSlots<{
  header: {};
  footer: {};
}>();
//const model = defineModel();
// const foo = defineModel("foo");
// const bar = defineModel({ type: String });
const baz = defineModel<string>("baz");
const poo = defineModel<string>({ default: "xxx" });
const rrr = defineModel<string>("rrr", { default: "ddd" });
defineExpose({
  foo,
});
</script>
