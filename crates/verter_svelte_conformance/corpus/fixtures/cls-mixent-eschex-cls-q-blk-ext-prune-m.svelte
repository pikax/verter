<script>
	let open = $state(true);
</script>

{#if open}
	<div class="a a&#38;&#x62;">x</div>
{/if}

<style>
	.a\26 b {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
