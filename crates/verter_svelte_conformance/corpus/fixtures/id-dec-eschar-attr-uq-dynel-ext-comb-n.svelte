<script>
	let tag = $state('div');
</script>

<div class="wrap">
	<svelte:element this={tag} data-x=a&#38;b>x</svelte:element>
</div>

<style>
	.wrap #\zz {
		color: red;
	}
</style>
