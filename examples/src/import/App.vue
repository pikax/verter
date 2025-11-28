<script lang="ts" setup>
import Comp from "./Comp.vue";
import { defineComponent, SlotsType } from "vue";

const c = new Comp();

// const Foo;
const Foo = defineComponent({
  props: undefined,
  data() {
    return {};
  },
  slots: {} as SlotsType<{
    foo: ()=> []
}>
});

const f = new Foo();
</script>
<template>
  <Foo v-slot="e=> {
    ___VERTER___slotInstance.$slots.foo
    }"/>
  <Comp />
</template>
