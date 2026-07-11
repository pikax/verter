<script>
	let open = $state(true);
</script>

{#if open}
	<div id="a&amp;b&#38;c">x</div>
{/if}

<style>
	:global(#a\26 b\&c) {
		color: red;
	}
</style>
