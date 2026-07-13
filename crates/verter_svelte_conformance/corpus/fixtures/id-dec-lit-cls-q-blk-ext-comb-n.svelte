<script>
	let open = $state(true);
</script>

{#if open}
	<div class="wrap">
	<div class="a&#32;b">x</div>
</div>
{/if}

<style>
	.wrap #zz {
		color: red;
	}
</style>
