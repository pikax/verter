<script lang="jsx">
export default {
  props: {
    label: { type: String, required: true },
  },
  methods: {
    bump(step) {
      return step + 1;
    },
    renderBadge() {
      return <span class="badge">{this.label}</span>;
    },
  },
};
</script>
<template>
  <div>{{ label }}</div>
</template>
