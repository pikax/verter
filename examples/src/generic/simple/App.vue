<script setup lang="ts">
import Child from "./Child.vue";
import Foo from "./Foo.vue";
function fooA(a) {
  a.item.myItem;
  //@ts-expect-error
  a.item.yourItem;
}

function fooB(b) {
  b.item.yourItem;
  //@ts-expect-error
  b.item.myItem;
}

function fooClose(e) {
  e.item.foo;
  // @ts-expect-error
  e.item.not;
}
</script>
<template>
  <Child :item="{ myItem: 1 }" v-on:close="fooA"></Child>
  <Child :item="{ yourItem: 2 }" @close="fooB"></Child>

  <Foo @close="fooClose" test="ss" v-foo/>
</template>
