<template>
  <div>
    <!-- Basic v-bind object spread -->
    <div v-bind="attrs">Spread all attrs</div>

    <!-- Shorthand v-bind spread -->
    <div v-bind="buttonAttrs">Button with spread</div>

    <!-- v-bind spread with other props (spread should merge) -->
    <div class="static-class" v-bind="attrs">Static + spread</div>
    <div :class="dynamicClass" v-bind="attrs">Dynamic + spread</div>

    <!-- v-bind spread with inline object -->
    <input v-bind="{ type: 'text', placeholder: 'Enter value', disabled: isDisabled }" />

    <!-- v-bind spread on component -->
    <MyComponent v-bind="componentProps" />

    <!-- v-bind spread with $attrs (inherit parent attrs) -->
    <div v-bind="$attrs">Inherit $attrs</div>

    <!-- Multiple v-bind spreads (later overrides earlier) -->
    <div v-bind="baseAttrs" v-bind="overrideAttrs">Multiple spreads</div>

    <!-- v-bind spread with events -->
    <button v-bind="buttonWithEvents">Click me</button>

    <!-- Computed spread object -->
    <div v-bind="computedAttrs">Computed spread</div>
  </div>
</template>

<script setup>
import { ref, computed } from "vue";
import MyComponent from "./MyComponent.vue";

const attrs = ref({
  id: "my-id",
  "data-test": "test-value",
  title: "My title",
});

const buttonAttrs = ref({
  type: "button",
  disabled: false,
  "aria-label": "Action button",
});

const dynamicClass = ref("dynamic-class");
const isDisabled = ref(false);

const componentProps = ref({
  title: "Component Title",
  count: 5,
  items: [1, 2, 3],
});

const baseAttrs = ref({ class: "base", id: "base-id" });
const overrideAttrs = ref({ class: "override", title: "Override title" });

const buttonWithEvents = ref({
  onClick: () => console.log("clicked"),
  onMouseenter: () => console.log("hover"),
});

const computedAttrs = computed(() => ({
  class: isDisabled.value ? "disabled" : "enabled",
  "aria-disabled": isDisabled.value,
}));
</script>
