<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x="&#98;&#x62;">x</div>
{/if}

<style>
	* {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
