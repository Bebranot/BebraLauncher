<script lang="ts">
  import { t } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./store.svelte";

  const running = $derived(app.play.phase === "running" ? app.play : null);
</script>

{#if running || app.notice}
  <div class="toast" role="status">
    {#if running}
      <span class="good">●</span>
      <span>{t("game.running", { name: running.playsAs })}</span>
    {:else if app.notice}
      <span>{app.notice}</span>
      <button class="x" aria-label={t("game.dismiss")} onclick={() => (app.notice = null)}>
        <Icon name="close" size={16} />
      </button>
    {/if}
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    left: 10px;
    bottom: 14px;
    z-index: 25;
    max-width: min(560px, calc(100vw - 20px));
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 14px;
    background: var(--popup);
    border: 1px solid var(--line);
    border-radius: 10px;
    box-shadow: var(--shadow);
    animation: pop-in 0.18s ease-out;
  }
  .x {
    appearance: none;
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
    color: var(--sub);
    display: grid;
  }
  .x:hover {
    color: var(--fg);
  }
</style>
