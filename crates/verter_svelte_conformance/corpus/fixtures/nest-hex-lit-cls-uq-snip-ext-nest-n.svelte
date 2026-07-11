{#snippet subject()}
	<div class=a&#x20;b>x</div>
{/snippet}

{@render subject()}

<style>
	.zz {
		& {
			color: red;
		}
	}
</style>
