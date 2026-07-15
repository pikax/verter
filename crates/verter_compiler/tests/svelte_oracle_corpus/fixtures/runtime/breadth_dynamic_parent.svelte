<script>
    let { Child } = $props();
    let label = $state('a');
</script>
<button onclick={() => label += '!'}>update</button>
<svelte:component this={Child} text={label} />
