<script>
	let active = $state(false);
	let color = $state('red');
	let id = $state('main');
</script>

<div {id} class:active style:color>
	dynamic attrs
</div>
