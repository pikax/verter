<script>
	let tag = $state('div');
	let { value } = $props();
</script>

<div class="wrap">
	<svelte:element this={tag} id={value}>x</svelte:element>
</div>

<style>
	.wrap #a\26 b {
		color: red;
	}
</style>
