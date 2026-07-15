{#snippet subject()}
	<div id="a&#x26;b">x</div>
{/snippet}

{@render subject()}

<style>
	#a\&b {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
