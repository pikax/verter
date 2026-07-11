<script>
	let open = $state(true);
</script>

{#if open}
	<div class=a&#x20;b>x</div>
{/if}

<style>
	* {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
