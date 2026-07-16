<script setup>
// @ts-check
import { ref } from "vue";

/** @type {number} */
const jsCount = 1;
/** @type {{ name: string }} */
const jsUser = { name: "ada" };
const jsRef = ref(0);

/** @param {number} n */
function jsBump(n) {
  return n + 1;
}

const jsNext = jsBump(jsCount);
</script>

<template>
  <div>
    <p>{{ jsCount }} {{ jsUser.name }} {{ jsNext }}</p>
    <button @click="jsBump(jsCount)">{{ jsRef }}</button>
  </div>
</template>
