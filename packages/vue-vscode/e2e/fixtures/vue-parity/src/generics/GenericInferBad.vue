<script setup lang="ts">
import GenericSelect from "./GenericSelect.vue";
import GenericField from "./GenericField.vue";
import GenericDefault from "./GenericDefault.vue";

const stringOptions = ["a", "b"];
const numberValue = 1;

function onSelectString(v: string) {
  void v;
}

function onSelectNumber(v: number) {
  void v;
}

function formatString(v: string) {
  return v.toUpperCase();
}

function onChangeNumber(v: number) {
  void v;
}
</script>

<template>
  <!-- BAD: options string[] but modelValue number -->
  <GenericSelect :options="stringOptions" :model-value="numberValue" @select="onSelectString" />
  <!-- BAD: event handler expects number but T inferred as string -->
  <GenericSelect :options="stringOptions" model-value="a" @select="onSelectNumber" />
  <!-- BAD: slot treats inferred string as number -->
  <GenericSelect :options="stringOptions" model-value="a">
    <template #selected="{ value }">
      <span>{{ value.toFixed(2) }}</span>
    </template>
  </GenericSelect>
  <!-- BAD: value number but format expects string -->
  <GenericField :value="1" :format="formatString" @change="onChangeNumber" />
  <!-- BAD: defaulted T=string, value is number -->
  <GenericDefault :value="99" />
</template>
