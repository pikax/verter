<script>
	let s = $state('color:red');
</script>

<button onclick={() => s = 'color:blue'} style={s}></button>
