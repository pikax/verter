<svelte:options css="injected" />

<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x="a&amp;b&#38;c">x</div>
{/if}

<style>
	#\7a \z {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
