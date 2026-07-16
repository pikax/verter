<script>
  // @ts-check
  /** @type {{ label: string, count: number }} */
  let jsValue = { label: "javascript", count: 2 };
  /** @param {{ label: string }} value */
  function renderJs(value) {
    return value.label.toUpperCase();
  }
  let jsRendered = renderJs(jsValue);
</script>

<p>{jsRendered} {jsValue.label}</p>
