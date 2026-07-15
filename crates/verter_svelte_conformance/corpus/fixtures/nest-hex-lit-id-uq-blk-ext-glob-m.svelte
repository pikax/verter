<script>
	let open = $state(true);
</script>

{#if open}
	<div id=&#x62;>x</div>
{/if}

<style>
	:global(&) {
		color: red;
	}
</style>
