{#snippet subject()}
	<div id="a&#38;b">x</div>
{/snippet}

{@render subject()}

<style>
	#a\26 b {
		color: red;
	}
</style>
