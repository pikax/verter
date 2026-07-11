<script>
	let open = $state(true);
</script>

{#if open}
	<div id=&#98;&#x62;>x</div>
{/if}

<style>
	div {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
