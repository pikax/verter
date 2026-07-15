<script>
	let open = $state(true);
</script>

{#if open}
	<div class=b>x</div>
{/if}

<style>
	div {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
