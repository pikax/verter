<script>
	let open = $state(true);
</script>

{#if open}
	<div class=a&#38;&#x62;>x</div>
{/if}

<style>
	:global(.a\&b) {
		color: red;
	}
</style>
