<script setup lang="ts">
import { ref, markRaw, shallowRef, defineComponent, h } from "vue";

const CompA = markRaw(
  defineComponent({
    render() {
      return h("div", { "data-testid": "dynamic-comp-a" }, "Component A");
    },
  }),
);

const CompB = markRaw(
  defineComponent({
    render() {
      return h("div", { "data-testid": "dynamic-comp-b" }, "Component B");
    },
  }),
);

const CompC = markRaw(
  defineComponent({
    render() {
      return h("div", { "data-testid": "dynamic-comp-c" }, "Component C");
    },
  }),
);

const components = [CompA, CompB, CompC];
const currentIndex = ref(0);
const currentComp = shallowRef(components[0]);

function switchComponent() {
  currentIndex.value = (currentIndex.value + 1) % components.length;
  currentComp.value = components[currentIndex.value];
}
</script>

<template>
  <div data-testid="dynamic-component">
    <button data-testid="dynamic-switch" @click="switchComponent">Switch</button>
    <component :is="currentComp" />
  </div>
</template>
