<script lang="ts">
  let { value = $bindable("") }: { value?: string } = $props();
</script>

<input bind:value />
