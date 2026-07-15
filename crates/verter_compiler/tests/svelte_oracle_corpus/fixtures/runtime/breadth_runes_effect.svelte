<script>
    let count = $state.raw(0);
    $effect(() => console.log('breadth-effect', count));
</script>
<button onclick={() => count++}>{count}</button>
