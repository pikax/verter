<script setup lang="ts">
// Template ref patterns for parser testing

import {
  ref,
  useTemplateRef,
  onMounted,
  type ComponentPublicInstance,
} from "vue";

// DOM element refs
const inputRef = ref(null);
const divRef = ref<HTMLDivElement>();
const canvasRef = ref<HTMLCanvasElement | null>(null);

// Vue 3.5+ useTemplateRef
const buttonRef = useTemplateRef("myButton");
const formRef = useTemplateRef("myForm");

// Multiple refs (v-for)
const itemRefs = ref<HTMLLIElement[]>([]);
const iii = useTemplateRef("itemRefs");

// Function ref
const setItemRef = (el: HTMLLIElement | null, index: number) => {
  if (el) {
    itemRefs.value[index] = el;
  }
};

// Component ref
import ChildComponent from "./ChildComponent.vue";
const childRef = ref(null);

// Generic component ref
const genericRef = ref<ComponentPublicInstance | null>(null);
const genRef = useTemplateRef("genericRef");

// Ref with exposed methods
interface ExposedMethods {
  focus: () => void;
  reset: () => void;
  getValue: () => string;
}
const exposedRef = ref<ExposedMethods | null>(null);

// Accessing refs
onMounted(() => {
  // DOM access
  inputRef.value?.focus();
  inputRef.value?.select();

  divRef.value?.scrollIntoView();
  divRef.value?.classList.add("mounted");

  const ctx = canvasRef.value?.getContext("2d");
  ctx?.fillRect(0, 0, 100, 100);

  // useTemplateRef access
  buttonRef.value?.click();
  formRef.value?.reset();

  // Array refs
  itemRefs.value.forEach((el, idx) => {
    el.dataset.index = String(idx);
  });

  // Component ref access
  childRef.value?.someMethod();

  // Exposed methods access
  exposedRef.value?.focus();
  const value = exposedRef.value?.getValue();
});

const items = ["a", "b", "c"];
</script>

<template>
  <div>
    <h2>Template Refs</h2>

    <!-- Basic ref -->
    <input ref="inputRef" type="text" />

    <!-- Typed ref -->
    <div ref="divRef">Div with ref</div>

    <!-- Canvas ref -->
    <canvas ref="canvasRef" width="200" height="200"></canvas>

    <!-- useTemplateRef (string must match) -->
    <button ref="myButton">Button</button>
    <form ref="myForm">
      <input type="text" />
    </form>

    <!-- Array refs with v-for -->
    <ul>
      <li v-for="(item, index) in items" :key="item" ref="itemRefs">
        {{ item }}
      </li>
    </ul>

    <!-- Component ref -->
    <ChildComponent ref="childRef" />

    <!-- Generic ref -->
    <component :is="'div'" ref="genericRef">Dynamic</component>
  </div>
</template>
