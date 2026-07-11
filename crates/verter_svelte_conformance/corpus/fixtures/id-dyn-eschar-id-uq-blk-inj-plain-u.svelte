<svelte:options css="injected" />

<script>
	let open = $state(true);
	let { value } = $props();
</script>

{#if open}
	<div id={value}>x</div>
{/if}

<style>
	#a\&b {
		color: red;
	}
</style>
