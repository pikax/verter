{#snippet subject()}
	<div class="a a&#38;b">x</div>
{/snippet}

{@render subject()}

<style>
	#\zz {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
