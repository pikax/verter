<script lang="ts">
  let { name }: { name: string } = $props();
  export function focus() {}
  export const ready = true;
</script>
<span>{name}</span>
