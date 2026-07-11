<svelte:options css="injected" />

<script>
	let tag = $state('div');
</script>

<div class="wrap">
	<svelte:element this={tag} class=&#98;&#x62;>x</svelte:element>
</div>

<style>
	.wrap #zz {
		color: red;
	}
</style>
