<script>
	let open = $state(true);
</script>

{#if open}
	<div data-x="a&copy;b">x</div>
{/if}

<style>
	:global(div) {
		color: red;
	}
</style>
