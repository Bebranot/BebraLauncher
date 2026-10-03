<script lang="ts">
  // One check box setting: the box, its name and a line of explanation under it.
  let {
    label,
    hint = "",
    checked,
    onchange,
    disabled = false,
  }: {
    label: string;
    hint?: string;
    checked: boolean;
    onchange: (value: boolean) => void;
    disabled?: boolean;
  } = $props();
</script>

<label class="option" class:disabled>
  <input
    type="checkbox"
    {checked}
    {disabled}
    onchange={(e) => onchange(e.currentTarget.checked)}
  />
  <span class="text">
    {label}
    {#if hint}<small class="sub">{hint}</small>{/if}
  </span>
</label>

<style>
  .option {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    cursor: pointer;
  }
  .option.disabled {
    opacity: 0.55;
    cursor: default;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  small {
    font-size: 13px;
  }
</style>
