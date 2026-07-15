{#snippet subject()}
	<div data-x=b>x</div>
{/snippet}

{@render subject()}

<style>
	[data-x="b"] {
		color: red;
	}
</style>
