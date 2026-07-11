<script>
	let open = $state(true);
</script>

{#if open}
	<div id="a&copy;b">x</div>
{/if}

<style>
	:global(#a©b) {
		color: red;
	}
</style>
