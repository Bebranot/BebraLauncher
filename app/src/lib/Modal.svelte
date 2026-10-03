<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "./i18n.svelte";
  import Icon from "./Icon.svelte";

  let {
    title,
    onclose,
    children,
  }: { title: string; onclose: () => void; children: Snippet } = $props();
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="box" role="dialog" aria-modal="true" aria-label={title}>
    <header>
      <h2>{title}</h2>
      <button class="close" onclick={onclose} aria-label={t("dialog.close")}>
        <Icon name="close" size={18} />
      </button>
    </header>
    <div class="body">{@render children()}</div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    place-items: center;
    background: var(--overlay);
    backdrop-filter: blur(2px);
    animation: backdrop-in 0.15s ease-out;
  }
  .box {
    width: min(480px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    display: flex;
    flex-direction: column;
    background: var(--popup);
    border: 1px solid var(--line);
    border-radius: 12px;
    box-shadow: var(--shadow);
    animation: pop-in 0.16s ease-out;
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid var(--line);
  }
  .close {
    appearance: none;
    background: none;
    border: 0;
    padding: 5px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    color: var(--sub);
    display: grid;
  }
  .close:hover {
    color: var(--fg);
    background: var(--tint-hover);
  }
  .body {
    padding: 18px;
    overflow: auto;
  }
</style>
