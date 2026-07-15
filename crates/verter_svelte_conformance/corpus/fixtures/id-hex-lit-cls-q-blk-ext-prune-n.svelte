<script>
	let open = $state(true);
</script>

{#if open}
	<div class="a&#x20;b">x</div>
{/if}

<style>
	#zz {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
