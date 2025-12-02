<script setup lang="ts">
import { ref, useTemplateRef } from "vue";
import DefineExpose from "./DefineExpose.vue";
import GenericExpose from "./GenericExpose.vue";

// Template ref to DOM element
const inputRef = ref<HTMLInputElement | null>(null);
const buttonRef = ref<HTMLButtonElement>();

// Vue 3.5+ useTemplateRef
const divRef = useTemplateRef("myDiv");
const canvasRef = useTemplateRef("canvas");

// Template ref to component with exposed methods
const exposeRef = ref<InstanceType<typeof DefineExpose> | null>(null);

// Generic component ref requires ComponentExposed helper for proper typing
// This is a known limitation with generic components
const genericRef = ref<{
  items: { id: number; name: string }[];
  addItem: (item: { id: number; name: string }) => void;
  getItems: () => { id: number; name: string }[];
  findItem: (predicate: (item: { id: number; name: string }) => boolean) => { id: number; name: string } | undefined;
} | null>(null);

// Accessing exposed methods (for type checking)
function testAccess() {
  // DOM ref access
  inputRef.value?.focus();
  buttonRef.value?.click();
  divRef.value?.scrollIntoView();

  // Component exposed access
  exposeRef.value?.publicMethod("test");
  exposeRef.value?.asyncMethod();
  exposeRef.value?.arrowMethod(1, 2);
  exposeRef.value?.count;

  // Generic component exposed access
  genericRef.value?.addItem({ id: 1, name: "test" });
  genericRef.value?.getItems();
}
</script>

<template>
  <div>
    <input ref="inputRef" type="text" />
    <button ref="buttonRef">Click</button>
    <div ref="myDiv">Content</div>
    <canvas ref="canvas"></canvas>

    <DefineExpose ref="exposeRef" />
    <GenericExpose ref="genericRef" />

    <button @click="testAccess">Test</button>
  </div>
</template>
