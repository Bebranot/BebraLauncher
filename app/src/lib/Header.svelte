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

<!-- Like the official launcher: the logo on the left; on the right the two links, and under
     them the account. The line under the header carries the accent. -->
<header>
  <Logo />
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
