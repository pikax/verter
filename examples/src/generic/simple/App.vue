<script setup lang="ts">
import Child from "./Child.vue";
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
</script>
<template>
  <Child :item="{ myItem: 1 }" v-on:close="fooA"></Child>
  <Child :item="{ yourItem: 2 }" @close="fooB"></Child>
</template>
