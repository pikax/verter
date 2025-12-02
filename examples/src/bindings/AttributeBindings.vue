<script setup lang="ts">
// Attribute binding patterns for parser testing

import { ref, computed } from "vue";

const isDisabled = ref(false);
const isActive = ref(true);
const inputType = ref<"text" | "password">("text");
const dynamicId = ref("my-element");
const href = ref("https://example.com");
const imageSrc = ref("/images/logo.png");

const classObject = ref({
  active: true,
  "text-danger": false,
  highlighted: true,
});

const classArray = ref(["base-class", "theme-dark"]);

const styleObject = ref({
  color: "red",
  fontSize: "14px",
  "background-color": "#f0f0f0",
});

const dynamicAttrName = ref("data-custom");
const dynamicAttrValue = ref("custom-value");

const ariaLabel = ref("Accessible button");
const dataTestId = ref("test-button");

const width = computed(() => 100);
const height = computed(() => 200);
</script>

<template>
  <div>
    <h2>Attribute Bindings</h2>

    <!-- Basic v-bind -->
    <section>
      <h3>Basic Bindings</h3>
      <a v-bind:href="href">Full v-bind syntax</a>
      <a :href="href">Shorthand</a>
      <input :type="inputType" />
      <div :id="dynamicId">Dynamic ID</div>
      <img :src="imageSrc" :alt="'Logo image'" />
    </section>

    <!-- Boolean attributes -->
    <section>
      <h3>Boolean Attributes</h3>
      <button :disabled="isDisabled">Disabled Button</button>
      <input :readonly="true" />
      <input :checked="isActive" type="checkbox" />
      <option :selected="true">Selected</option>
      <details :open="isActive">
        <summary>Details</summary>
        Content
      </details>
    </section>

    <!-- Class bindings -->
    <section>
      <h3>Class Bindings</h3>
      <!-- Object syntax -->
      <div :class="{ active: isActive, disabled: isDisabled }">Object</div>
      <div :class="classObject">Object ref</div>

      <!-- Array syntax -->
      <div :class="[isActive ? 'active' : '', 'base']">Array with ternary</div>
      <div :class="classArray">Array ref</div>

      <!-- Mixed -->
      <div :class="[classObject, classArray, 'extra']">Mixed</div>

      <!-- With static class -->
      <div class="static" :class="{ dynamic: isActive }">Static + Dynamic</div>
    </section>

    <!-- Style bindings -->
    <section>
      <h3>Style Bindings</h3>
      <!-- Object syntax -->
      <div :style="{ color: 'red', fontSize: '14px' }">Inline object</div>
      <div :style="styleObject">Object ref</div>

      <!-- camelCase and kebab-case -->
      <div :style="{ backgroundColor: 'blue' }">camelCase</div>
      <div :style="{ 'background-color': 'green' }">kebab-case</div>

      <!-- Array (multiple style objects) -->
      <div :style="[styleObject, { fontWeight: 'bold' }]">Array</div>

      <!-- CSS custom properties -->
      <div :style="{ '--custom-color': 'purple' }">CSS variable</div>

      <!-- Auto-prefixing (comment: array syntax for fallback values) -->
      <div :style="{ display: 'flex' }">
        Auto-prefix array
      </div>
    </section>

    <!-- Dynamic attribute names -->
    <section>
      <h3>Dynamic Attribute Names</h3>
      <div :[dynamicAttrName]="dynamicAttrValue">Dynamic attr name</div>
    </section>

    <!-- Multiple attributes (v-bind without argument) -->
    <section>
      <h3>v-bind Object</h3>
      <input v-bind="{ type: 'text', placeholder: 'Enter value', disabled: isDisabled }" />
      <div v-bind="{ id: dynamicId, class: classObject, style: styleObject }">
        All bindings
      </div>
    </section>

    <!-- Aria attributes -->
    <section>
      <h3>Aria Attributes</h3>
      <button
        :aria-label="ariaLabel"
        :aria-disabled="isDisabled"
        :aria-pressed="isActive"
        :aria-expanded="isActive"
        role="button"
      >
        Accessible Button
      </button>
    </section>

    <!-- Data attributes -->
    <section>
      <h3>Data Attributes</h3>
      <div
        :data-testid="dataTestId"
        :data-id="123"
        :data-config="JSON.stringify({ a: 1 })"
        data-static="static-value"
      >
        Data attributes
      </div>
    </section>

    <!-- SVG attributes -->
    <section>
      <h3>SVG Attributes</h3>
      <svg :width="width" :height="height" :viewBox="`0 0 ${width} ${height}`">
        <circle :cx="width / 2" :cy="height / 2" :r="50" fill="blue" />
        <rect :x="10" :y="10" :width="width - 20" :height="height - 20" 
              fill="none" stroke="red" :stroke-width="2" />
      </svg>
    </section>

    <!-- Props with same name shorthand (Vue 3.4+) -->
    <section>
      <h3>Same-name Shorthand</h3>
      <ChildComponent :isActive :isDisabled />
    </section>
  </div>
</template>

<script setup lang="ts">
// Mock child component for shorthand demo
const ChildComponent = {
  props: ["isActive", "isDisabled"],
  template: "<div>Active: {{ isActive }}, Disabled: {{ isDisabled }}</div>",
};
</script>
