{#snippet subject()}
	<div data-x="a&b">x</div>
{/snippet}

{@render subject()}

<style>
	[data-x="\zz"] {
		color: red;
	}
</style>
