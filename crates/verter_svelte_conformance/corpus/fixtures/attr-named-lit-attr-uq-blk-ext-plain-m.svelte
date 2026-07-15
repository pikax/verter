<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x=a&copy;b>x</div>
{/if}

<style>
	[data-x="a©b"] {
		color: red;
	}
</style>
