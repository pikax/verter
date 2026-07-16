<script>
	let value = $state({ label: 'x' });
	const constant = 5;
	function label(input) {
		return input.label;
	}
</script>

<p>{value.label}</p>
<p>{label(value)}</p>
<div>static {constant}</div>
{#each [value] as item}
	<p>item {item.label}</p>
{/each}

<button onclick={() => (value = { label: 'y' })}>change</button>
