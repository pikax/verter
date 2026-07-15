<script>
	let c = $state(0);
	let { items } = $props();
</script>

{#each items as item (item.id)}
	<div animate:flip on:click={() => c++}>x</div>
{/each}
