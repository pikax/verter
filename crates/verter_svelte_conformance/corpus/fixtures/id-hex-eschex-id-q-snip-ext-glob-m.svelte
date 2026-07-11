{#snippet subject()}
	<div id="a&#x26;b">x</div>
{/snippet}

{@render subject()}

<style>
	:global(#a\26 b) {
		color: red;
	}
</style>
