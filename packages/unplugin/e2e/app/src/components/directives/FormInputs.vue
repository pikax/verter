<script setup lang="ts">
import { ref } from "vue";

const text = ref("");
const checked = ref(false);
const radio = ref("opt1");
const selected = ref("a");
const textarea = ref("");
</script>

<template>
  <div data-testid="form-inputs">
    <input data-testid="text-input" v-model="text" />
    <span data-testid="text-mirror">{{ text }}</span>

    <input data-testid="checkbox-input" type="checkbox" v-model="checked" />
    <span data-testid="checkbox-mirror">{{ checked }}</span>

    <label><input data-testid="radio-opt1" type="radio" v-model="radio" value="opt1" /> Opt1</label>
    <label><input data-testid="radio-opt2" type="radio" v-model="radio" value="opt2" /> Opt2</label>
    <span data-testid="radio-mirror">{{ radio }}</span>

    <select data-testid="select-input" v-model="selected">
      <option value="a">A</option>
      <option value="b">B</option>
      <option value="c">C</option>
    </select>
    <span data-testid="select-mirror">{{ selected }}</span>

    <textarea data-testid="textarea-input" v-model="textarea"></textarea>
    <span data-testid="textarea-mirror">{{ textarea }}</span>
  </div>
</template>
