<script>
	let text = $state('');
	let el = $state();
</script>

<input bind:value={text} />
<div bind:this={el}>{text}</div>
