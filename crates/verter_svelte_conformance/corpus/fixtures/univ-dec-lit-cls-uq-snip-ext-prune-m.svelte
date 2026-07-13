{#snippet subject()}
	<div class=a&#32;b>x</div>
{/snippet}

{@render subject()}

<style>
	* {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
