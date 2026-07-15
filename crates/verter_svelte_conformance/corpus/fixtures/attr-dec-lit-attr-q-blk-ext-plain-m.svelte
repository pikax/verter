<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x="&#98;">x</div>
{/if}

<style>
	[data-x="b"] {
		color: red;
	}
</style>
