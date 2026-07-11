{#snippet subject()}
	<div class="a a&#38;&#x62;">x</div>
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
