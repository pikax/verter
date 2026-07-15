<script>
  let count = $state(0);
</script>

<button class="counter" data-testid="counter" onclick={() => count += 1}>
  count {count}
</button>

<style>
  .counter {
    color: rgb(255, 0, 0);
  }
</style>
