<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x="b">x</div>
{/if}

<style>
	[data-x="b"] {
		color: red;
	}
</style>
