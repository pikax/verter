{#snippet subject()}
	<div class="a&#32;b">x</div>
{/snippet}

{@render subject()}

<style>
	.zz {
		& {
			color: red;
		}
	}
</style>
