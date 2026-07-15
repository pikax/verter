<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div class=a&amp;b>x</div>
{/if}

<style>
	[data-x="\zz"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
