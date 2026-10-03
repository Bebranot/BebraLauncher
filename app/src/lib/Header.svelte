<script lang="ts">
  import { api } from "./api";
  import AccountDropdown from "./AccountDropdown.svelte";
  import { t } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";

  // The same community links as the official launcher's header.
  const DISCORD = "https://discord.ss14.io/";
  const WEBSITE = "https://spacestation14.com";
</script>

<header>
  <div class="logo"><Logo /></div>
  <div class="right">
    <div class="links">
      <button class="btn open-both" onclick={() => api.openUrl(DISCORD)}>
        <span class="with-icon"><Icon name="chat" size={16} />{t("header.discord")}</span>
      </button>
      <button class="btn open-l" onclick={() => api.openUrl(WEBSITE)}>
        <span class="with-icon"><Icon name="web" size={16} />{t("header.website")}</span>
      </button>
    </div>
    <AccountDropdown />
  </div>
</header>
<div class="gold-line"></div>

<style>
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 8px 6px 14px;
    background: var(--popup);
  }
  .logo {
    display: flex;
    align-items: center;
  }
  .right {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 3px;
  }
  .links {
    display: flex;
  }
  .with-icon {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
</style>
