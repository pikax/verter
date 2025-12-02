<script lang="ts">
// Options API patterns for parser testing

import { defineComponent, type PropType } from "vue";

interface User {
  id: number;
  name: string;
}

export default defineComponent({
  name: "OptionsApiComplete",

  // Component inheritance
  inheritAttrs: false,

  // Components registration
  components: {
    // LocalComponent: defineComponent({ ... })
  },

  // Directives registration
  directives: {
    focus: {
      mounted(el: HTMLElement) {
        el.focus();
      },
    },
  },

  // Props with full typing
  props: {
    title: {
      type: String,
      required: true,
    },
    count: {
      type: Number,
      default: 0,
    },
    user: {
      type: Object as PropType<User>,
      default: () => ({ id: 0, name: "Guest" }),
    },
    items: {
      type: Array as PropType<string[]>,
      default: () => [],
    },
    callback: {
      type: Function as PropType<(value: string) => void>,
    },
  },

  // Emits with validation
  emits: {
    change: (value: string) => typeof value === "string",
    update: (id: number, data: User) => true,
    submit: null,
  },

  // Data function
  data() {
    return {
      internalCount: 0,
      message: "",
      isLoading: false,
      localItems: [] as string[],
    };
  },

  // Computed properties
  computed: {
    doubleCount(): number {
      return this.count * 2;
    },
    fullMessage(): string {
      return `${this.title}: ${this.message}`;
    },
    // Computed with getter/setter
    upperMessage: {
      get(): string {
        return this.message.toUpperCase();
      },
      set(value: string) {
        this.message = value.toLowerCase();
      },
    },
  },

  // Watchers
  watch: {
    count(newVal: number, oldVal: number) {
      console.log(`Count changed from ${oldVal} to ${newVal}`);
    },
    message: {
      handler(newVal: string) {
        console.log("Message changed:", newVal);
      },
      immediate: true,
      deep: false,
    },
    // Watch nested property
    "user.name"(newName: string) {
      console.log("User name changed:", newName);
    },
  },

  // Lifecycle hooks
  beforeCreate() {
    console.log("beforeCreate");
  },
  created() {
    console.log("created");
  },
  beforeMount() {
    console.log("beforeMount");
  },
  mounted() {
    console.log("mounted");
  },
  beforeUpdate() {
    console.log("beforeUpdate");
  },
  updated() {
    console.log("updated");
  },
  beforeUnmount() {
    console.log("beforeUnmount");
  },
  unmounted() {
    console.log("unmounted");
  },

  // Methods
  methods: {
    increment() {
      this.internalCount++;
      this.$emit("change", String(this.internalCount));
    },
    async fetchData() {
      this.isLoading = true;
      try {
        await new Promise((r) => setTimeout(r, 1000));
        this.localItems = ["a", "b", "c"];
      } finally {
        this.isLoading = false;
      }
    },
    handleClick(event: MouseEvent) {
      console.log("Clicked at:", event.clientX, event.clientY);
    },
  },

  // Expose (Vue 3.2+)
  expose: ["increment", "internalCount"],

  // Render function alternative
  // render() {
  //   return h('div', {}, this.message)
  // }
});
</script>

<template>
  <div>
    <h2>{{ title }}</h2>
    <p>Count: {{ count }} (Double: {{ doubleCount }})</p>
    <p>Internal: {{ internalCount }}</p>
    <p>Message: {{ fullMessage }}</p>
    <p>User: {{ user.name }}</p>

    <input v-model="message" placeholder="Enter message" />
    <input v-model="upperMessage" placeholder="Uppercase binding" />

    <button @click="increment">Increment</button>
    <button @click="fetchData" :disabled="isLoading">
      {{ isLoading ? "Loading..." : "Fetch Data" }}
    </button>

    <ul>
      <li v-for="item in items" :key="item">{{ item }}</li>
    </ul>
  </div>
</template>
