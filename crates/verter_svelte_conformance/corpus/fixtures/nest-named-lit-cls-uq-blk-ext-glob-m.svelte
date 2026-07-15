<script>
	let open = $state(true);
</script>

{#if open}
	<div class=a&copy;b>x</div>
{/if}

<style>
	:global(&) {
		color: red;
	}
</style>
