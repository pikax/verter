<script setup lang="ts">
import { reactive } from "vue";

interface State {
  name: string;
  items: string[];
}

const initialState = (): State => ({ name: "Hello", items: ["a", "b", "c"] });
const state = reactive<State>(initialState());

function changeName() {
  state.name = "World";
}

function addItem() {
  state.items.push("item-" + (state.items.length + 1));
}

function reset() {
  Object.assign(state, initialState());
}
</script>

<template>
  <div data-testid="reactive-object">
    <span data-testid="name">{{ state.name }}</span>
    <ul data-testid="items">
      <li v-for="(item, i) in state.items" :key="i">{{ item }}</li>
    </ul>
    <button data-testid="change-name" @click="changeName">Change Name</button>
    <button data-testid="add-item" @click="addItem">Add Item</button>
    <button data-testid="reset" @click="reset">Reset</button>
  </div>
</template>
