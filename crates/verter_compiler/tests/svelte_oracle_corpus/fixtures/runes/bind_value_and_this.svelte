<script>
	let v = $state('');
	let el;
</script>

<input bind:value={v} /><div bind:this={el}></div>{v}
