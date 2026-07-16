<script lang="ts">
  let count = $state(0);
  let ticks = $state(0);

  $effect(() => {
    // read count so the effect depends on it
    void count;
    ticks += 1;
  });
</script>

<button onclick={() => (count += 1)}>{count} / {ticks}</button>
