<script>
export default {
  name: "PropsEmits",
  props: {
    name: { type: String, required: true },
    count: { type: Number, required: true },
    active: { type: Boolean, default: false },
  },
  emits: ["change", "update"],
  methods: {
    handleClick() {
      this.$emit("change", this.name);
      this.$emit("update", this.count);
    },
  },
};
</script>

<template>
  <div>
    <p>Name: {{ name }}</p>
    <p>Count: {{ count }}</p>
    <p>Active: {{ active }}</p>
    <button @click="handleClick">Emit</button>
  </div>
</template>
