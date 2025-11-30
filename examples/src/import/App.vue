<script lang="ts" setup>
import Comp from "./Comp.vue";
import { defineComponent, SlotsType } from "vue";

const c = new Comp();

// const Foo;
const Foo = defineComponent({
  props: {
    a: String
  },
  data() {
    return {};
  },
  slots: {} as SlotsType<{
    foo: (args: { a: 1 }) => [];
  }>,
});

const f = new Foo();

const aa = {
  Foo,
  random: {
    a: 1,
    aa: 2,
  },
};
</script>
<template>
  <Foo  >
    <template #foo="args">
      {{ ___VERTER___slotInstance.$slots.foo }}
      {{ ___VERTER___renderSlotJSX(___VERTER___slotInstance.$slots.foo) }}
      {{ args }}
    </template>
  </Foo>
  <Comp />

  <div></div>

  {{ aa }}

  {{ ___DEBUG_Components.aa.Foo }}

  {{
    // @ts-expect-error
    ___DEBUG_Components.aa.random
  }}
  {{ ___VERTER___components.Foo }}

  {{
    () => {
      let a = {} as ___VERTER___OmitNever<{
        foo: 1;
        bar: () => any;
        kkk: never;
      }>;

      a;
    }
  }}
</template>
