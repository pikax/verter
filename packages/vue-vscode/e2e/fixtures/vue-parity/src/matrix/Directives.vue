<script setup lang="ts">
import { ref, computed } from "vue";

const show = ref(true);
const items = ref(["a", "b"]);
const model = ref("");
const raw = ref("<b>x</b>");
const klass = ref("active");
const color = ref("red");
const disabled = ref(false);

const label = computed(() => (show.value ? "on" : "off"));

function onClick() {
  show.value = !show.value;
}
</script>

<template>
  <div>
    <p v-if="show" v-show="show">{{ label }}</p>
    <p v-else>off</p>
    <ul>
      <li v-for="(item, index) in items" :key="index">{{ item }}:{{ index }}</li>
    </ul>
    <input v-model="model" :class="klass" :style="{ color }" :disabled="disabled" />
    <div v-html="raw"></div>
    <div v-text="label"></div>
    <button v-on:click="onClick" @click.prevent="onClick">toggle</button>
    <input v-bind:value="model" />
  </div>
</template>
