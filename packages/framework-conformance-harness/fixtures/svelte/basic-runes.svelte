<script>
  let count = $state(0);
  let items = ["a", "b", "c"];
</script>

<div class="root">
  {#if count > 0}
    <p>{count}</p>
  {:else}
    <p>zero</p>
  {/if}
  <ul>
    {#each items as item (item)}
      <li>{item}</li>
    {/each}
  </ul>
</div>
