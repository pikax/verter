<script lang="ts">
import { defineComponent, ref, type Ref, type InjectionKey, computed } from "vue";

export const CounterKey: InjectionKey<Ref<number>> = Symbol("counter");
export const ThemeKey: InjectionKey<"light" | "dark"> = Symbol("theme");

export default defineComponent({
  name: "ProvideInject",
  setup() {
    const counterRef = ref(0);
    const theme = ref<"light" | "dark">("light");
    const readonlyCounter = computed(() => counterRef.value);

    const increment = () => {
      counterRef.value++;
    };

    const toggleTheme = () => {
      theme.value = theme.value === "light" ? "dark" : "light";
    };

    return {
      counterRef,
      theme,
      readonlyCounter,
      increment,
      toggleTheme,
    };
  },
  provide() {
    return {
      counter: this.readonlyCounter,
      theme: this.theme,
      message: "Hello from provider",
    };
  },
});
</script>

<template>
  <div>
    <p>Counter: {{ readonlyCounter }}</p>
    <p>Theme: {{ theme }}</p>
    <button @click="increment">Increment</button>
    <button @click="toggleTheme">Toggle Theme</button>
    <slot />
  </div>
</template>
