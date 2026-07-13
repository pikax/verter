{#snippet subject()}
	<div id="a&#38;b">x</div>
{/snippet}

{@render subject()}

<style>
	[data-x="\zz"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
