<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x>x</div>
{/if}

<style>
	#\zz {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
