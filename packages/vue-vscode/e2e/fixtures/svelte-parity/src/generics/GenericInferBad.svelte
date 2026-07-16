<script lang="ts">
  import GenericSelect from "./GenericSelect.svelte";
  import GenericField from "./GenericField.svelte";
  import GenericDefault from "./GenericDefault.svelte";

  const stringOptions = ["a", "b"];
  const numberValue = 1;

  function onSelectString(v: string) {
    void v;
  }
  function onSelectNumber(v: number) {
    void v;
  }
  function formatString(v: string) {
    return v.toUpperCase();
  }
  function onChangeNumber(v: number) {
    void v;
  }
</script>

<!-- BAD: options string[] vs value number -->
<GenericSelect options={stringOptions} value={numberValue} onSelect={onSelectString} />
<!-- BAD: handler number vs T string -->
<GenericSelect options={stringOptions} value="a" onSelect={onSelectNumber} />
<!-- BAD: slot treats string T as number -->
<GenericSelect options={stringOptions} value="a">
  {#snippet selected({ value })}
    <span>{value.toFixed(2)}</span>
  {/snippet}
</GenericSelect>
<!-- BAD: value number vs format string -->
<GenericField value={1} format={formatString} onChange={onChangeNumber} />
<!-- BAD: default T string, value number -->
<GenericDefault value={99} />
