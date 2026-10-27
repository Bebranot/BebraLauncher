<script lang="ts">
  import AddFavoriteDialog from "./AddFavoriteDialog.svelte";
  import ConfirmConnectDialog from "./ConfirmConnectDialog.svelte";
  import DirectConnectDialog from "./DirectConnectDialog.svelte";
  import EditFavoriteDialog from "./EditFavoriteDialog.svelte";
import LoginDialog from "./LoginDialog.svelte";
  import { app } from "./store.svelte";

  const close = () => (app.dialog = null);
</script>

{#if app.dialog?.kind === "direct"}
  <DirectConnectDialog onclose={close} />
{:else if app.dialog?.kind === "addfav"}
  <AddFavoriteDialog onclose={close} />
{:else if app.dialog?.kind === "editfav"}
  <EditFavoriteDialog onclose={close} address={app.dialog.address} currentName={app.dialog.name} />
{:else if app.dialog?.kind === "confirm_connect"}
  <ConfirmConnectDialog onclose={close} server={app.dialog.server} />
{:else if app.dialog?.kind === "login"}
  <LoginDialog onclose={close} relogin={app.dialog.relogin} />
{/if}
