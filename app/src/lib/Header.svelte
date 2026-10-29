<script lang="ts">
  import { api } from "./api";
  import AccountDropdown from "./AccountDropdown.svelte";
  import { t } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import { app } from "./store.svelte";

  // The same community links as the official launcher's header.
  const DISCORD = "https://discord.ss14.io/";
  const WEBSITE = "https://spacestation14.com";
</script>

<!-- Like the official launcher: the logo on the left; on the right the two links, and under
     them the account. The line under the header carries the accent. -->
<header>
  <div class="left">
    <Logo />
    {#if app.runningClients.length > 0}
      <span class="running-badge" title={t("header.runningCount", { count: app.runningClients.length })}>
        <i class="dot"></i>
        {t("header.runningCount", { count: app.runningClients.length })}
      </span>
    {/if}
  </div>
  <div class="right">
    <div class="links">
      <button class="btn open-r" onclick={() => api.openUrl(DISCORD)}>
        <Icon name="chat" size={16} />
        {t("header.discord")}
      </button>
      <button class="btn open-l" onclick={() => api.openUrl(WEBSITE)}>
        <Icon name="web" size={16} />
        {t("header.website")}
      </button>
    </div>
    <AccountDropdown />
  </div>
</header>

<style>
  header {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 12px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--line);
    transition: border-color 0.15s ease;
  }
  :global(html[data-theme="classic"]) header {
    border-bottom: 2px solid var(--accent);
  }
  .left {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .running-badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    font-weight: 500;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    user-select: none;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 6px var(--accent);
    animation: pulse 2s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% {
      opacity: 1;
      transform: scale(1);
    }
    50% {
      opacity: 0.5;
      transform: scale(0.85);
    }
  }
  .right {
    display: grid;
    gap: 4px;
    min-width: 0;
    max-width: 100%;
    justify-items: stretch;
  }
  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 0;
  }
  .links .btn {
    flex: 1 1 auto;
    min-width: 72px;
    font-size: 13px;
  }
</style>
