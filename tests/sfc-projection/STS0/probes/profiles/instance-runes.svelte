<script lang="ts">
  let count = $state(0);
  const doubled: number = $derived(count * 2);
</script>

<p>{doubled}</p>
