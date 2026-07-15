<script>
	let tag = $state('div');
	let { flag } = $props();
</script>

<svelte:element this={tag} class="{flag ? 'a b' : '\u0062'}">x</svelte:element>

<style>
	.zz {
		color: red;
	}
</style>
