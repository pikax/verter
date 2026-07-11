<script>
	let open = $state(true);
</script>

{#if open}
	<div class="a a&#x26;b">x</div>
{/if}

<style>
	:global(.a\&b) {
		color: red;
	}
</style>
