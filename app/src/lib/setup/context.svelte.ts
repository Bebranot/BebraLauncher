// What the wizard learns about this computer, shared between its steps.
import {
  api,
  type HardwareInfo,
  type OfficialImport,
  type OfficialProbe,
} from "../api";

export class SetupContext {
  /** Cores and memory; `null` until the system answers (or when it never does). */
  hardware = $state<HardwareInfo | null>(null);
  /** What the official launcher left behind; `null` until the check is done. */
  probe = $state<OfficialProbe | null>(null);
  /** What the import step actually brought over, once it has run. */
  imported = $state<OfficialImport | null>(null);

  load(): void {
    void api
      .hardwareInfo()
      .then((h) => (this.hardware = h))
      .catch(() => {});
    void api
      .officialProbe()
      .then((p) => (this.probe = p))
      .catch(() => (this.probe = { found: false, favorites: 0, hubs: 0, compat_mode: null }));
  }

  get officialFound(): boolean {
    return this.probe?.found ?? false;
  }
}
