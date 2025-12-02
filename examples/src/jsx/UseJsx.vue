<script setup lang="ts">
// Using JSX components in Vue SFC

import {
  JsxComponent,
  SlottedComponent,
  RefComponent,
  VModelComponent,
  renderButton,
  renderList,
  renderFragment,
} from "./JsxPatterns";
import { ref, h } from "vue";

const count = ref(0);
const inputValue = ref("");

// Using h() to render JSX functions in template
const buttonVNode = () => renderButton("JSX Button", () => count.value++);
const listVNode = () => renderList(["Item 1", "Item 2", "Item 3"]);
</script>

<template>
  <div>
    <h2>JSX Components Usage</h2>

    <!-- JSX Component -->
    <JsxComponent title="Hello from JSX" />

    <!-- Slotted JSX Component -->
    <SlottedComponent>
      <template #header>
        <h3>Slot Header</h3>
      </template>
      <template #default>
        <p>Default slot content</p>
      </template>
      <template #footer>
        <small>Footer</small>
      </template>
    </SlottedComponent>

    <!-- Ref Component -->
    <RefComponent />

    <!-- v-model Component -->
    <VModelComponent v-model="inputValue" />
    <p>Input value: {{ inputValue }}</p>

    <!-- Using render functions via component :is -->
    <component :is="buttonVNode" />
    <p>Count: {{ count }}</p>

    <component :is="listVNode" />
  </div>
</template>
