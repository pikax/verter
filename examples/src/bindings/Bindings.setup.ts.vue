<script setup lang="ts">
// Class and Style binding patterns for parser testing - Setup API + TypeScript

import { ref, computed, reactive } from "vue";

// Boolean class bindings
const isActive = ref<boolean>(true);
const hasError = ref<boolean>(false);
const isDisabled = ref<boolean>(false);
const isHighlighted = ref<boolean>(true);

// Dynamic class name
const dynamicClassName = ref<string>("custom-class");

// Object-based class bindings
const classObject = reactive<Record<string, boolean>>({
  active: true,
  "text-danger": false,
  "has-border": true,
});

// Computed class object
const computedClassObject = computed<Record<string, boolean>>(() => ({
  active: isActive.value,
  "has-error": hasError.value,
  disabled: isDisabled.value,
}));

// Array-based class bindings
const baseClass = ref<string>("base");
const activeClass = ref<string>("active");
const errorClass = ref<string>("error");

// Inline style bindings
const activeColor = ref<string>("red");
const fontSize = ref<number>(16);
const marginValue = ref<number>(10);

// Object-based style bindings
const styleObject = reactive<Record<string, string>>({
  color: "blue",
  fontSize: "14px",
  fontWeight: "bold",
});

// Computed style object
const computedStyleObject = computed<Record<string, string>>(() => ({
  color: isActive.value ? activeColor.value : "gray",
  fontSize: `${fontSize.value}px`,
  marginTop: `${marginValue.value}px`,
}));

// Multiple style objects for merging
const baseStyles = reactive({
  padding: "10px",
  margin: "5px",
});

const additionalStyles = reactive({
  border: "1px solid black",
  borderRadius: "4px",
});

// CSS custom properties (variables)
const primaryColor = ref<string>("#3498db");
const secondaryColor = ref<string>("#2ecc71");

// Conditional values
type Size = "small" | "medium" | "large";
const size = ref<Size>("medium");

const sizeClasses = computed<Record<Size, string>>(() => ({
  small: "size-sm",
  medium: "size-md",
  large: "size-lg",
}));

// Toggle functions
function toggleActive(): void {
  isActive.value = !isActive.value;
}

function toggleError(): void {
  hasError.value = !hasError.value;
}

function setSize(newSize: Size): void {
  size.value = newSize;
}
</script>

<template>
  <div>
    <!-- Basic class binding with boolean -->
    <div :class="{ active: isActive }">Boolean class binding</div>

    <!-- Multiple class bindings -->
    <div :class="{ active: isActive, 'text-danger': hasError, disabled: isDisabled }">
      Multiple boolean classes
    </div>

    <!-- Binding to reactive object -->
    <div :class="classObject">Reactive object classes</div>

    <!-- Binding to computed object -->
    <div :class="computedClassObject">Computed object classes</div>

    <!-- Array syntax -->
    <div :class="[baseClass, activeClass]">Array of classes</div>

    <!-- Array with conditionals -->
    <div :class="[isActive ? activeClass : '', hasError ? errorClass : '']">
      Array with ternary
    </div>

    <!-- Array with object in array -->
    <div :class="[{ active: isActive }, errorClass]">Array with object</div>

    <!-- Dynamic class name binding -->
    <div :class="dynamicClassName">Dynamic class name</div>

    <!-- Static and dynamic classes combined -->
    <div class="static-class" :class="{ dynamic: isActive }">Static + dynamic</div>

    <!-- Class with computed value -->
    <div :class="sizeClasses[size]">Size-based class: {{ size }}</div>

    <!-- Basic inline style -->
    <div :style="{ color: activeColor, fontSize: fontSize + 'px' }">Inline style object</div>

    <!-- Style with camelCase -->
    <div :style="{ backgroundColor: 'yellow', paddingTop: '10px' }">CamelCase styles</div>

    <!-- Style with kebab-case (quoted) -->
    <div :style="{ 'background-color': 'lightblue', 'font-size': '18px' }">Kebab-case styles</div>

    <!-- Binding to style object -->
    <div :style="styleObject">Style object binding</div>

    <!-- Computed style object -->
    <div :style="computedStyleObject">Computed style object</div>

    <!-- Array of style objects (merging) -->
    <div :style="[baseStyles, additionalStyles]">Array of style objects</div>

    <!-- CSS custom properties -->
    <div :style="{ '--primary-color': primaryColor, '--secondary-color': secondaryColor }">
      CSS custom properties
    </div>

    <!-- Auto-prefixing demonstration -->
    <div :style="{ display: 'flex', justifyContent: 'center', alignItems: 'center' }">
      Flexbox styles (auto-prefixed)
    </div>

    <!-- Multiple values fallback (commented - Vue supports at runtime but TS style types don't) -->
    <!-- <div :style="{ display: ['-webkit-box', '-ms-flexbox', 'flex'] }"> -->
    <div :style="{ display: 'flex' }">
      Multiple values for fallback (simplified for type checking)
    </div>

    <!-- Complex combined example -->
    <div
      class="base-element"
      :class="[
        { active: isActive, error: hasError },
        sizeClasses[size],
        dynamicClassName,
      ]"
      :style="[
        computedStyleObject,
        baseStyles,
        { border: hasError ? '2px solid red' : 'none' },
      ]"
    >
      Complex combined bindings
    </div>

    <!-- v-bind shorthand note: requires actual ref named 'class' which conflicts -->
    <div :class="isActive ? 'active' : ''">Same-name shorthand example</div>

    <!-- Dynamic attribute binding -->
    <div :[`data-${dynamicClassName}`]="isActive">Dynamic attribute name</div>

    <!-- Bind multiple attributes with v-bind object -->
    <div v-bind="{ id: 'my-id', 'data-active': isActive }">v-bind object spread</div>

    <!-- Controls -->
    <button @click="toggleActive">Toggle Active: {{ isActive }}</button>
    <button @click="toggleError">Toggle Error: {{ hasError }}</button>
    <button @click="setSize('small')">Small</button>
    <button @click="setSize('medium')">Medium</button>
    <button @click="setSize('large')">Large</button>
  </div>
</template>

<style scoped>
.active {
  font-weight: bold;
}
.text-danger {
  color: red;
}
.disabled {
  opacity: 0.5;
}
.has-border {
  border: 1px solid gray;
}
.size-sm {
  font-size: 12px;
}
.size-md {
  font-size: 16px;
}
.size-lg {
  font-size: 20px;
}
</style>
