{#snippet subject()}
	<div data-x=a&copy;b>x</div>
{/snippet}

{@render subject()}

<style>
	[data-x="a©b"] {
		color: red;
	}
</style>
