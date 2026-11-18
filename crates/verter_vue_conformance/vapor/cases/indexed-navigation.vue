<script setup>
import { ref } from 'vue'
const label = ref('')
const note = ref('')
const href = ref('')
const open = ref(false)
function go() {}
</script>
<template>
  <section>
    <ul>
      <li>
        <i>zero</i>
        <span :title="label">one</span>
        <em>two</em>
        <b>three</b>
        <strong :title="note">four</strong>
        <u @click="go">five</u>
      </li>
    </ul>
    <p>static</p>
    <div :title="href">{{ label }}<b>{{ note }}</b></div>
    <details><summary :aria-label="label"><Icon />{{ note }}</summary><div @click="go"><slot /></div></details>
    <nav><a>one</a><a>two</a><a :href="href">three</a><p v-if="open">x</p><b>tail</b><i :title="note">end</i></nav>
  </section>
</template>
