<script>
	let x = $state(0);
</script>

<input {...props} defaultValue={x} /><button onclick={() => x++}>b</button>
