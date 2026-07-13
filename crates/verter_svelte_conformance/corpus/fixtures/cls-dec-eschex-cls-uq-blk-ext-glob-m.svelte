<script>
	let open = $state(true);
</script>

{#if open}
	<div class=a&#38;b>x</div>
{/if}

<style>
	:global(.a\26 b) {
		color: red;
	}
</style>
