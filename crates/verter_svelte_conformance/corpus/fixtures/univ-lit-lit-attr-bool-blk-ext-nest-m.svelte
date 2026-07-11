<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x>x</div>
{/if}

<style>
	* {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
