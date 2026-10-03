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
    gap: 12px;
    cursor: pointer;
  }
  /* The box is a switch here. */
  input[type="checkbox"] {
    width: 34px;
    height: 20px;
    margin: 1px 0 0;
    border-radius: 999px;
    border: 0;
    background: color-mix(in srgb, var(--fg) 20%, transparent);
    place-content: unset;
    position: relative;
    transition: background 0.15s;
  }
  input[type="checkbox"]:hover {
    background: color-mix(in srgb, var(--fg) 28%, transparent);
  }
  input[type="checkbox"]::after,
  input[type="checkbox"]:checked::after {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    border: 0;
    border-radius: 50%;
    background: var(--sub);
    transform: none;
    transition: transform 0.15s;
  }
  input[type="checkbox"]:checked {
    background: var(--accent);
  }
  input[type="checkbox"]:checked::after {
    background: var(--on-accent);
    transform: translateX(14px);
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
    line-height: 1.35;
  }
</style>
