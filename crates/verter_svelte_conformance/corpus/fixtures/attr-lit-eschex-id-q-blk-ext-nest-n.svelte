<script>
	let open = $state(true);
</script>

{#if open}
	<div id="a&b">x</div>
{/if}

<style>
	[data-x="\7a z"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
