<script setup lang="ts">
import TypeNegChild from "./TypeNegChild.vue";

/** Wrong payload: emit `pick` is string, this wants number. */
function wrongPick(n: number): void {
  void n;
}

/** Wrong payload: emit `change` is number, this wants string. */
function wrongChange(s: string): void {
  void s;
}

/** Native click expects MouseEvent; string-only param is not a valid overload. */
function wrongNativeClick(s: string): void {
  void s;
}
</script>

<template>
  <TypeNegChild title="ok" :count="1" @pick="wrongPick" @change="wrongChange" />
  <button type="button" @click="wrongNativeClick">native</button>
</template>
