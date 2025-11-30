<script lang="ts" setup>
import {} from "$verter/types$";

withDefaults(
  defineProps({
    a: String,
  }),
  {}
);

___VERTER___defineProps_Boxed.a = String;
// @ts-expect-error
___VERTER___defineProps_Boxed.a = Number;

___VERTER___withDefaults_Boxed[1] = {};

if (___VERTER___withDefaults.a === true) {
  // @ts-expect-error narrowed
  ___VERTER___withDefaults.a === false;
}

// @ts-expect-error readonly
___VERTER___withDefaults.a = false;
</script>

<template>
  <div>{{ $props }}</div>
  <div>{{ bar }}</div>

  <span> {{ ___DEBUG_Verter }} </span>
  <span> {{ ___DEBUG_Default }} </span>
  <span> {{ ___DEBUG_Props }} </span>
  <span> {{ ___DEBUG_Components }} </span>
  <span> {{ ___DEBUG_FullContext }} </span>
  <span> {{ ___DEBUG_Binding }} </span>
</template>
