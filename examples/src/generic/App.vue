<script setup lang="ts">
import Comp from "./Comp.vue";
import CompProps from "./Comp.props.vue";

const foo = {} as InstanceType<typeof Comp>;

defineOptions({
  data() {
    return {
      bar: "foo",
    };
  },
});

const c = new Comp<"foo">().$props["___VERTER___v-slot"];

const f = defineProps({
  foo: String,
});
</script>

<template>
  <Comp
    :ref="
      (x) => {
        // @ts-expect-error condition should not be valid
        if (x.$props.name === 'rr') {
        }

        if (x.$props.name === 'foo') {
          // expected to be here
        }
      }
    "
    name="foo"
    :onName="
      (e) => {
        // @ts-expect-error condition should not be valid
        if (e !== 'foo') {
          // not expected to be here
        }

        if (e === 'foo') {
        }
      }
    "
  >
    <template #foo="{ name }"> {{ $props }}</template>
    <template #header></template>
  </Comp>

  <CompProps
    :ref="
      (x) => {
        // @ts-expect-error condition should not be valid
        if (x.$props.name === 'rr') {
        }

        if (x.$props.name === 'foo') {
          // expected to be here
        }
      }
    "
    name="foo"
    :onName="
      (e) => {
        // @ts-expect-error condition should not be valid
        if (e !== 'foo') {
          // not expected to be here
        }

        if (e === 'foo') {
        }
      }
    "
  />
  <span>{{ $data.bar }}</span>
</template>
