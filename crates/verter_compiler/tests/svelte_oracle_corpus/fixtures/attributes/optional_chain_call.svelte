<script>
	let v = $state('a');
	let w = $state('b');
</script>

<input readonly={v?.startsWith?.('x')} class:active={w?.endsWith?.('y')} />
