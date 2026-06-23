<script>
	let x = $state(0);
</script>

<div {...props} style:color="a{x}b"><button onclick={() => x++}>b</button></div>
