<script lang="ts">
  import GenericSelect from "./GenericSelect.svelte";
  import GenericField from "./GenericField.svelte";
  import GenericDefault from "./GenericDefault.svelte";

  const stringOptions = ["a", "b", "c"];
  let stringValue = $state("a");

  function onSelect(v: string) {
    void v;
  }
  function onChange(v: string) {
    stringValue = v;
  }

  const numberOptions = [1, 2, 3];
  let numberValue = $state(1);
  function onNumSelect(v: number) {
    void v;
  }
  function onNumChange(v: number) {
    numberValue = v;
  }

  const num = 42;
  function formatNum(v: number) {
    return v.toFixed(0);
  }
</script>

<!-- STRING inference: options → value + events + snippets as string -->
<GenericSelect
  options={stringOptions}
  value={stringValue}
  label="pick-str"
  {onSelect}
  {onChange}
>
  {#snippet selected({ value: selStr })}
    <span class="sel-str">{selStr.toUpperCase()}</span>
  {/snippet}
  {#snippet option({ item: optStr, index })}
    <span class="opt-str">{optStr}:{index}</span>
  {/snippet}
</GenericSelect>

<!-- NUMBER inference -->
<GenericSelect
  options={numberOptions}
  value={numberValue}
  label="pick-num"
  onSelect={onNumSelect}
  onChange={onNumChange}
>
  {#snippet selected({ value: selNum })}
    <span class="sel-num">{selNum.toFixed(0)}</span>
  {/snippet}
  {#snippet option({ item: optNum })}
    <span class="opt-num">{optNum.toFixed(0)}</span>
  {/snippet}
</GenericSelect>

<GenericField value={num} format={formatNum} onChange={onNumChange} />
<GenericDefault value="hello" prefix=">" />
