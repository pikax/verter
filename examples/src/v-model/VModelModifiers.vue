<script setup lang="ts">
import { ref } from "vue";

const text = ref("  hello world  ");
const number = ref(42);
const title = ref("My Title");

// v-model modifiers testing
const lazyText = ref("");
const trimmedText = ref("");
const numericValue = ref(0);
const combinedValue = ref("");

// Additional refs for form elements
const isChecked = ref(false);
const picked = ref<"a" | "b">("a");
const selected = ref("");
const multiSelected = ref<string[]>([]);
const textareaContent = ref("");
</script>

<template>
  <div>
    <h2>v-model Modifiers</h2>

    <!-- .trim modifier -->
    <section>
      <h3>.trim</h3>
      <input v-model.trim="trimmedText" placeholder="Trimmed input" />
      <p>Value: "{{ trimmedText }}"</p>
    </section>

    <!-- .number modifier -->
    <section>
      <h3>.number</h3>
      <input v-model.number="numericValue" type="text" placeholder="Numeric" />
      <p>Value: {{ numericValue }} (type: {{ typeof numericValue }})</p>
    </section>

    <!-- .lazy modifier -->
    <section>
      <h3>.lazy</h3>
      <input v-model.lazy="lazyText" placeholder="Updates on change/blur" />
      <p>Value: "{{ lazyText }}"</p>
    </section>

    <!-- Combined modifiers -->
    <section>
      <h3>Combined: .lazy.trim</h3>
      <input v-model.lazy.trim="combinedValue" placeholder="Lazy + Trim" />
      <p>Value: "{{ combinedValue }}"</p>
    </section>

    <!-- Checkbox v-model -->
    <section>
      <h3>Checkbox</h3>
      <input type="checkbox" v-model="isChecked" />
      <p>Checked: {{ isChecked }}</p>
    </section>

    <!-- Radio v-model -->
    <section>
      <h3>Radio</h3>
      <input type="radio" v-model="picked" value="a" /> A
      <input type="radio" v-model="picked" value="b" /> B
      <p>Picked: {{ picked }}</p>
    </section>

    <!-- Select v-model -->
    <section>
      <h3>Select</h3>
      <select v-model="selected">
        <option value="">Select...</option>
        <option value="one">One</option>
        <option value="two">Two</option>
      </select>
      <p>Selected: {{ selected }}</p>
    </section>

    <!-- Multiple select -->
    <section>
      <h3>Multiple Select</h3>
      <select v-model="multiSelected" multiple>
        <option value="a">A</option>
        <option value="b">B</option>
        <option value="c">C</option>
      </select>
      <p>Selected: {{ multiSelected }}</p>
    </section>

    <!-- Textarea -->
    <section>
      <h3>Textarea</h3>
      <textarea v-model="textareaContent" rows="3"></textarea>
      <p>Content: {{ textareaContent }}</p>
    </section>
  </div>
</template>
