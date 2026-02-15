<template>
  <div>
    <!-- Basic dynamic component -->
    <component :is="currentComponent" />

    <!-- Dynamic component with static is (renders as that element) -->
    <component is="div">Static div</component>
    <component is="span">Static span</component>

    <!-- Dynamic component with props -->
    <component :is="currentComponent" :title="title" :count="count" />

    <!-- Dynamic component with events -->
    <component :is="currentComponent" @click="handleClick" @custom="handleCustom" />

    <!-- Dynamic component with slots -->
    <component :is="currentComponent">
      <template #default>Default slot content</template>
      <template #header>Header slot content</template>
    </component>

    <!-- Dynamic component with v-model -->
    <component :is="inputComponent" v-model="inputValue" />

    <!-- Conditional dynamic component -->
    <component :is="condition ? CompA : CompB" :shared-prop="sharedValue" />

    <!-- Dynamic component from computed -->
    <component :is="computedComponent" />

    <!-- Dynamic component in v-for -->
    <component v-for="item in components" :key="item.id" :is="item.component" :data="item.data" />
  </div>
</template>

<script setup>
import { ref, computed } from "vue";
import CompA from "./CompA.vue";
import CompB from "./CompB.vue";

const currentComponent = ref("CompA");
const title = ref("Dynamic Title");
const count = ref(0);
const inputComponent = ref("input");
const inputValue = ref("");
const condition = ref(true);
const sharedValue = ref("shared");

const computedComponent = computed(() => {
  return condition.value ? CompA : CompB;
});

const components = ref([
  { id: 1, component: "CompA", data: { x: 1 } },
  { id: 2, component: "CompB", data: { y: 2 } },
]);

const handleClick = () => console.log("clicked");
const handleCustom = () => console.log("custom event");
</script>
