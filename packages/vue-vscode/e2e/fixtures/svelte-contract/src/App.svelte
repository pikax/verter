<script lang="ts">
  import DirectChild from "./components/DirectChild.svelte";

  interface ContractValue {
    label: string;
    count: number;
  }

  let typedValue: ContractValue = { label: "typed", count: 1 };
  function renderTyped(): string {
    return `${typedValue.label}:${typedValue.count}`;
  }
</script>

<main>
  <button onclick={renderTyped}>{typedValue.label}</button>
  <DirectChild contractProp={typedValue.label} />
</main>
