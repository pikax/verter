<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div id=a&#x26;b&#x26;c>x</div>
{/if}

<style>
	#a\26 b\&c {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
