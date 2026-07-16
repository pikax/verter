<script setup lang="ts">
import GenericSelect from "./GenericSelect.vue";
import GenericField from "./GenericField.vue";
import GenericDefault from "./GenericDefault.vue";
import GenericList from "./GenericList.vue";

/** T inferred as string from options — no GenericSelect&lt;string&gt; at call site */
const stringOptions = ["a", "b", "c"];
const stringValue = "a";

function onSelect(v: string) {
  void v;
}

function onUpdate(v: string) {
  void v;
}

/** T inferred as number via value + format */
const num = 42;
const numberOptions = [1, 2, 3];
const numberValue = 1;
function formatNum(v: number) {
  return v.toFixed(0);
}
function onNumChange(v: number) {
  void v;
}
function onNumSelect(v: number) {
  void v;
}

interface Row {
  id: string;
  label: string;
}
const rows: Row[] = [{ id: "1", label: "one" }];
</script>

<template>
  <!-- STRING inference: options → modelValue + events + slot props as string -->
  <GenericSelect
    :options="stringOptions"
    :model-value="stringValue"
    label="pick-str"
    @select="onSelect"
    @update:model-value="onUpdate"
  >
    <template #selected="{ value: selStr }">
      <!-- hover selStr must be string (inferred from options) -->
      <span class="sel-str">{{ selStr.toUpperCase() }}</span>
    </template>
    <template #option="{ item: optStr, index }">
      <!-- hover optStr must be string -->
      <span class="opt-str">{{ optStr }}:{{ index }}</span>
    </template>
  </GenericSelect>

  <!-- NUMBER inference: options number[] → modelValue + events + slots as number -->
  <GenericSelect
    :options="numberOptions"
    :model-value="numberValue"
    label="pick-num"
    @select="onNumSelect"
    @update:model-value="onNumChange"
  >
    <template #selected="{ value: selNum }">
      <span class="sel-num">{{ selNum.toFixed(0) }}</span>
    </template>
    <template #option="{ item: optNum }">
      <span class="opt-num">{{ optNum.toFixed(0) }}</span>
    </template>
  </GenericSelect>

  <!-- Multi-prop linkage: value + format + change all number -->
  <GenericField :value="num" :format="formatNum" @change="onNumChange" />
  <!-- Defaulted T = string -->
  <GenericDefault value="hello" prefix=">" />
  <GenericList :items="rows" selected-id="1" />
</template>
