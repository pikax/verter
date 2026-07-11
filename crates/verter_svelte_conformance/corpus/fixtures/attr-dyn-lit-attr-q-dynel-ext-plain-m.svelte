<script>
	let tag = $state('div');
	let { flag } = $props();
</script>

<svelte:element this={tag} data-x="{flag ? 'b' : '\u0062'}">x</svelte:element>

<style>
	[data-x="b"] {
		color: red;
	}
</style>
