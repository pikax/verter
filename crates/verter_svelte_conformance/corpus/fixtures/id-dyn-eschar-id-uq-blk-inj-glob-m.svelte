<svelte:options css="injected" />

<script>
	let open = $state(true);
	let { flag } = $props();
</script>

{#if open}
	<div id={flag ? 'a&b' : 'a\u0026b'}>x</div>
{/if}

<style>
	:global(#a\&b) {
		color: red;
	}
</style>
