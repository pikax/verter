<script lang="ts">
  let { value = $bindable(), label }: { value: string; label: string } = $props();
</script>
<input bind:value />{label}
