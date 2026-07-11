{#snippet subject()}
	<div class="a&#x20;b">x</div>
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
