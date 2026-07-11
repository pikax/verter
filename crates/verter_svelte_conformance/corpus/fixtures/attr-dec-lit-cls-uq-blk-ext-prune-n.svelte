<script>
	let open = $state(true);
</script>

{#if open}
	<div class=a&#32;b>x</div>
{/if}

<style>
	[data-x="zz"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
