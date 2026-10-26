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
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 12px;
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
    font-size: 12px;
    font-weight: 500;
    padding: 3px 8px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    user-select: none;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 6px var(--accent);
  }
  .right {
    display: grid;
    gap: 4px;
    min-width: 0;
    justify-items: stretch;
  }
  .links {
    display: flex;
  }
  .links .btn {
    flex: 1;
    min-width: 90px;
  }
</style>
