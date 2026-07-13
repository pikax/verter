{#snippet subject()}
	<div id="a&#38;b&#38;c">x</div>
{/snippet}

{@render subject()}

<style>
	#a\26 b\&c {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
