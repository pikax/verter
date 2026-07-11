<script>
	let open = $state(true);
</script>

{#if open}
	<div class="a b">x</div>
{/if}

<style>
	:global(div) {
		color: red;
	}
</style>
