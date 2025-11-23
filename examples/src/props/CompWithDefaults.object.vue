<script lang="ts" setup>
import {} from "$verter/types$";

const props = withDefaults(
  defineProps({
    a: String,
  }),
  {}
);

const props = defineProps({
  a: String,
});

const pp = ___VERTER___defineProps_Box<{ a: string }>();
const dd = ___VERTER___withDefaults_Box(defineProps<{ a: string }>(), {});
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
