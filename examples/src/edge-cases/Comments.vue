<script setup lang="ts">
// Comments in various positions

// Single line comment before import
import { ref } from "vue"; // trailing comment

/* 
  Multi-line comment
  before code
*/
const count = ref(0);

/**
 * JSDoc comment
 * @param value - the value to process
 * @returns processed value
 */
function process(value: string): string {
  return value.toUpperCase();
}

// Inline comments
const result = process(
  "test" // argument comment
); // call comment

// Comment with special characters: < > & " ' ` $ { } [ ] ( )
// Comment with unicode: 你好 مرحبا שלום

/*
 * Comment with code-like content:
 * const x = 1;
 * function foo() {}
 * <template></template>
 */

// TODO: This is a todo comment
// FIXME: This needs fixing
// NOTE: Important note
// HACK: Temporary workaround
// @ts-ignore next line
// eslint-disable-next-line
</script>

<template>
  <!-- HTML comment in template -->
  <div>
    <!-- Comment before element -->
    <p>{{ count }}</p>
    <!-- Comment after element -->
  </div>
  <!--
    Multi-line
    HTML comment
  -->
</template>

<style>
/* CSS comment */
.class {
  /* Property comment */
  color: red; /* inline comment */
}
</style>
