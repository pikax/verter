<script>
    import { writable } from 'svelte/store';
    const count = writable(1);
    let doubled;
    $: doubled = $count * 2;
</script>
<button on:click={() => $count++}>{doubled}</button>
