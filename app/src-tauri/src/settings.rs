//! User settings: one small JSON file in the launcher's data folder.

use std::io::Write;
use std::path::{Path, PathBuf};

use bebra_core::launch::FALLBACK_USERNAME;
use serde::{Deserialize, Serialize};

/// The shape of `settings.json` this build understands.
///
/// Older files had no `schema_version` at all (version 0); `parse` brings them up to this
/// number before they are read (P2-39).
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// How many favourite servers are kept. The ping call and the list do not need more.
pub const FAVORITES_LIMIT: usize = 200;
/// How many server notes are kept.
pub const SERVER_NOTES_LIMIT: usize = 200;
/// Longest favourite name / server name kept in the history, in characters.
pub const NAME_LIMIT: usize = 120;

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// A server address in one spelling, or `None` if it is not a server address.
///
/// The hub identifies servers by its own spelling of the address, so only the parts that
/// cannot change meaning are normalized: the scheme becomes `ss14`/`ss14s`, the host is
/// lowercased, `http(s)://` becomes `ss14(s)://`, and a missing path stays missing (the hub
/// lists some servers with a trailing slash and some without).
pub fn canonical_address(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let addr = bebra_core::ServerAddr::parse(trimmed).ok()?;
    let mut out = addr.normalized();
    let after_scheme = trimmed
        .find("://")
        .map(|i| &trimmed[i + 3..])
        .unwrap_or(trimmed);
    if !after_scheme.contains('/') && out.ends_with('/') {
        out.pop();
    }
    Some(out)
}

/// Removes control characters and shortens text the UI shows as a name.
fn clean_name(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(NAME_LIMIT)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Single-line text for a note: line breaks are allowed, other controls are not.
fn clean_note(text: &str, limit: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(*c, '\n' | '\r' | '\t'))
        .take(limit)
        .collect::<String>()
        .trim()
        .to_string()
}

/// The canonical form when the text is a server address, the trimmed text otherwise.
fn canonical_or_trimmed(address: &str) -> String {
    canonical_address(address).unwrap_or_else(|| address.trim().to_string())
}

/// Where the schema version of a read file stands relative to this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaSource {
    /// Written by this schema version.
    Current,
    /// Written before the version field existed (or by an older one) and migrated.
    Older,
    /// Written by a newer launcher; unknown fields are kept as they are.
    Newer,
}

/// Reads `settings.json` bytes, migrating an older schema first (P2-39).
pub fn parse(bytes: &[u8]) -> Result<(Settings, SchemaSource), serde_json::Error> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let found = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    let source = match found.cmp(&CURRENT_SCHEMA_VERSION) {
        std::cmp::Ordering::Less => SchemaSource::Older,
        std::cmp::Ordering::Equal => SchemaSource::Current,
        std::cmp::Ordering::Greater => SchemaSource::Newer,
    };
    let value = migrate(found, value);
    let settings: Settings = serde_json::from_value(value)?;
    Ok((settings, source))
}

/// Brings a settings value written by schema `from` up to [`CURRENT_SCHEMA_VERSION`].
///
/// Every step only changes what a newer build reads differently; values that are merely out of
/// range are left to [`Settings::sanitized`]. A file from a newer launcher is returned
/// untouched: those fields are unknown here, and dropping them would lose the user's data.
fn migrate(from: u32, mut value: serde_json::Value) -> serde_json::Value {
    let mut version = from;
    let mut steps = 0;
    // A bound as well: a corrupt version number must not spin here.
    while version < CURRENT_SCHEMA_VERSION && steps < 100 {
        match version {
            // 0 -> 1: the version field itself was added. A version-0 file has the same
            // fields otherwise, so there is nothing to move; the defaults fill the rest.
            0 => {}
            _ => break,
        }
        version += 1;
        steps += 1;
        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "schema_version".to_string(),
                serde_json::Value::from(version),
            );
        }
    }
    value
}

/// One save at a time: the temporary file name is fixed.
static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Settings schema version for migrations and compatibility.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// The name used when playing without an account.
    pub guest_name: String,
    /// Start the game in the GLES2 fallback renderer.
    pub compat_mode: bool,
    /// Whether Discord Rich Presence is enabled in the game client.
    pub discord_rpc: bool,
    /// Minimize the launcher while the game runs, and bring it back when the game closes.
    pub minimize_on_play: bool,
    /// Id of the colour theme; the UI knows the list and falls back to the default.
    pub theme: String,
    /// Gradient background (instead of a flat colour).
    pub theme_gradient: bool,
    /// Decorative striped pattern behind the lists.
    pub theme_decor: bool,
    /// Colours of the `custom` theme.
    pub custom_theme: CustomTheme,
    /// Font family for the interface; empty = the built-in one. Must be installed in Windows.
    pub font: String,
    /// Which columns the server list shows.
    pub columns: Columns,
    /// Text packs to use (file names in the packs folder), the first one wins on conflicts.
    pub enabled_packs: Vec<String>,
    /// Do not follow the game's requests to reconnect (redial).
    pub ignore_redial: bool,
    /// Write more detail to the launcher log (the launch command, with secrets hidden).
    pub verbose_log: bool,
    /// Proxy for the launcher's own requests (`http://`, `https://`, `socks5://`); empty = none.
    pub proxy: String,
    /// `auto` (follow Windows) or the code of an interface language (`ru`, `en`, `uk`, `de`).
    pub language: String,
    /// The GitHub repository whose releases update the launcher (`owner/name`).
    pub update_repo: String,
    /// Look for a new launcher version at start and every few hours.
    pub update_check: bool,
    /// Download a new version by itself and install it when the launcher is closed.
    pub update_auto: bool,
    /// The account chosen last (user id); preselected next time.
    /// An empty string means the player chose to play as a guest; `None`: never chosen.
    pub last_account: Option<String>,
    /// Addresses of favourite servers.
    pub favorites: Vec<String>,
    /// Servers played recently, newest first.
    pub history: Vec<HistoryEntry>,
    /// Server hubs used to populate the server list.
    pub hubs: Vec<HubSetting>,
    /// Additional arguments passed to the game client / engine.
    pub extra_args: String,
    /// Client patches (.dll files in patches folder) enabled by the user.
    pub enabled_patches: Vec<String>,
    /// Client optimization profile: "standard", "balanced", "optimized" or "custom" (older names
    /// are migrated in `sanitized`).
    pub opt_profile: String,
    /// Fast lighting: disable soft shadows and lighting blur.
    pub opt_fast_lighting: bool,
    /// Unlock framerate: disable VSync and set max FPS to uncapped.
    pub opt_unlock_fps: bool,
    /// Run game process with elevated Windows priority.
    pub opt_high_priority: bool,
    /// Enable .NET Server Garbage Collection and DATAS to prevent frame stutters.
    pub opt_server_gc: bool,
    /// Background mode: "space" (default space canvas), "custom" (user image), "none" (solid/theme).
    pub bg_type: String,
    /// Filename of selected custom background in backgrounds folder.
    pub bg_custom_name: Option<String>,
    /// Background darkening tint overlay percentage (0..95, default 40).
    pub bg_dim: u32,
    /// Background blur filter in px (0..30, default 0).
    pub bg_blur: u32,
    /// Game client window mode: "default", "windowed", "fullscreen", "borderless".
    pub client_window_mode: String,
    /// Mute game audio when client is unfocused / minimized.
    pub client_mute_unfocused: bool,
    /// Game client UI scale: "auto", "0.75", "1.0", "1.25", "1.5", "1.75", "2.0".
    pub client_ui_scale: String,
    /// Maximum client FPS limit (0 = uncapped / profile default).
    pub client_max_fps: u32,
    /// Close the launcher completely when the game starts.
    pub close_on_play: bool,
    /// Server list auto-refresh interval in seconds (0 = off, 15, 30, 60, 120).
    pub auto_refresh_interval: u32,
    /// Confirm connect with server details modal before launching.
    pub confirm_connect: bool,
    /// Personal user notes per server address.
    #[serde(default)]
    pub server_notes: std::collections::HashMap<String, String>,
    /// Custom user-assigned names for favourite servers.
    #[serde(default)]
    pub favorite_names: std::collections::HashMap<String, String>,
    /// Scale factor for the launcher's own UI (percent, 75..=200).
    #[serde(default = "default_ui_scale")]
    pub launcher_ui_scale: u32,
    /// Automatically clean old build cache if disk usage exceeds limit.
    #[serde(default = "default_auto_clean_cache")]
    pub auto_clean_cache: bool,
    /// Whether the user has dismissed the Intel 13th/14th Gen CPU degradation advisory.
    #[serde(default)]
    pub dismissed_intel_warning: bool,
    /// Generate a new hardware ID before every game launch.
    #[serde(default = "default_hwid_auto")]
    pub hwid_auto: bool,
    /// The hardware ID to report now (hex, 64 characters). `None` = the machine's own.
    #[serde(default)]
    pub hwid_current: Option<String>,
    /// The value `hwid_current` had before the last change, so it can be restored.
    #[serde(default)]
    pub hwid_previous: Option<String>,
}

fn default_auto_clean_cache() -> bool {
    true
}

fn default_ui_scale() -> u32 {
    100
}

fn default_hwid_auto() -> bool {
    true
}

/// Configured server hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HubSetting {
    pub url: String,
    pub enabled: bool,
}

impl Default for HubSetting {
    fn default() -> Self {
        Self {
            url: bebra_core::hub::OFFICIAL_HUB_URL.to_string(),
            enabled: true,
        }
    }
}

pub fn default_hubs() -> Vec<HubSetting> {
    vec![HubSetting {
        url: bebra_core::hub::OFFICIAL_HUB_URL.to_string(),
        enabled: true,
    }]
}

/// Colours of the user's own theme, as `#RRGGBB`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomTheme {
    pub background: String,
    pub accent: String,
    pub foreground: String,
    pub popup: String,
    pub gradient_start: String,
    pub gradient_end: String,
}

impl Default for CustomTheme {
    fn default() -> Self {
        Self {
            background: "#25252A".into(),
            accent: "#ADA24B".into(),
            foreground: "#EEEEEE".into(),
            popup: "#202025".into(),
            gradient_start: "#2A2A31".into(),
            gradient_end: "#2F3A36".into(),
        }
    }
}

impl CustomTheme {
    fn sanitized(self) -> Self {
        let d = Self::default();
        let fix = |value: String, fallback: String| {
            if is_hex_color(&value) {
                value.to_uppercase()
            } else {
                fallback
            }
        };
        Self {
            background: fix(self.background, d.background),
            accent: fix(self.accent, d.accent),
            foreground: fix(self.foreground, d.foreground),
            popup: fix(self.popup, d.popup),
            gradient_start: fix(self.gradient_start, d.gradient_start),
            gradient_end: fix(self.gradient_end, d.gradient_end),
        }
    }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Columns of the server list besides the name and the Connect button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Columns {
    pub round_time: bool,
    pub players: bool,
    pub map: bool,
    pub mode: bool,
    pub ping: bool,
}

impl Default for Columns {
    fn default() -> Self {
        Self {
            round_time: true,
            players: true,
            map: false,
            mode: false,
            ping: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub address: String,
    pub name: Option<String>,
    /// RFC 3339.
    pub last_played: String,
    /// Time spent in the game on this server, in seconds.
    #[serde(default)]
    pub play_seconds: u64,
    /// The account used here last time (user id; empty = guest), preselected next time.
    #[serde(default)]
    pub account: Option<String>,
}

/// How many recent servers are remembered.
pub const HISTORY_LIMIT: usize = 20;

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            guest_name: FALLBACK_USERNAME.to_string(),
            compat_mode: false,
            discord_rpc: true,
            minimize_on_play: false,
            theme: "dark".to_string(),
            theme_gradient: false,
            theme_decor: false,
            custom_theme: CustomTheme::default(),
            font: String::new(),
            columns: Columns::default(),
            enabled_packs: Vec::new(),
            ignore_redial: false,
            verbose_log: false,
            proxy: String::new(),
            language: "auto".to_string(),
            update_repo: bebra_core::self_update::DEFAULT_REPO.to_string(),
            update_check: true,
            update_auto: true,
            last_account: None,
            favorites: Vec::new(),
            history: Vec::new(),
            hubs: default_hubs(),
            extra_args: String::new(),
            enabled_patches: Vec::new(),
            opt_profile: "balanced".to_string(),
            opt_fast_lighting: false,
            opt_unlock_fps: false,
            opt_high_priority: true,
            opt_server_gc: true,
            bg_type: "space".to_string(),
            bg_custom_name: None,
            bg_dim: 40,
            bg_blur: 0,
            client_window_mode: "default".to_string(),
            client_mute_unfocused: true,
            client_ui_scale: "auto".to_string(),
            client_max_fps: 0,
            close_on_play: false,
            auto_refresh_interval: 0,
            confirm_connect: false,
            server_notes: std::collections::HashMap::new(),
            favorite_names: std::collections::HashMap::new(),
            launcher_ui_scale: 100,
            auto_clean_cache: true,
            dismissed_intel_warning: false,
            hwid_auto: default_hwid_auto(),
            hwid_current: None,
            hwid_previous: None,
        }
    }
}

impl Settings {
    pub fn path(root: &Path) -> PathBuf {
        root.join("settings.json")
    }

    pub fn backup_path(root: &Path) -> PathBuf {
        root.join("settings.json.bak")
    }

    /// Loads settings with automatic backup recovery and corrupt file quarantine.
    /// If settings.json is corrupted, it attempts to recover from settings.json.bak,
    /// quarantines the corrupt file, and falls back to defaults only if no valid file exists.
    pub fn load(root: &Path) -> Self {
        let path = Self::path(root);
        let bak = Self::backup_path(root);

        if path.exists() {
            match std::fs::read(&path) {
                Ok(bytes) => match parse(&bytes) {
                    Ok((settings, source)) => {
                        if source == SchemaSource::Newer {
                            crate::applog::line(&format!(
                                "settings.json comes from a newer launcher (schema {}); using it as it is",
                                settings.schema_version
                            ));
                        }
                        return Self::cleaned(root, settings, source == SchemaSource::Older);
                    }
                    Err(e) => {
                        crate::applog::line(&format!(
                            "settings.json is corrupted ({e}); quarantining and attempting recovery"
                        ));
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let corrupt_path = root.join(format!("settings.json.corrupt-{ts}"));
                        let _ = std::fs::rename(&path, corrupt_path);
                    }
                },
                Err(e) => {
                    crate::applog::line(&format!("cannot read settings.json ({e})"));
                }
            }
        }

        // Try recovering from backup
        if bak.exists() {
            if let Ok(bytes) = std::fs::read(&bak) {
                if let Ok((settings, _)) = parse(&bytes) {
                    crate::applog::line("settings restored successfully from settings.json.bak");
                    return Self::cleaned(root, settings, true);
                }
            }
        }

        Settings::default()
    }

    /// Cleans what was read and writes the clean version back.
    ///
    /// A file can be valid JSON and still contain values the UI cannot survive
    /// (`{"launcher_ui_scale": 0}` makes the window empty). Such a file is only reachable by a
    /// hand edit or an old version, but it is read at start, before the reset button exists, so
    /// the same cleaning as in `set_settings` has to happen here (P1-19). An already clean file
    /// is left untouched, so a normal start does not rewrite it.
    fn cleaned(root: &Path, settings: Settings, migrated: bool) -> Self {
        let clean = settings.clone().sanitized();
        let same = match (serde_json::to_vec(&settings), serde_json::to_vec(&clean)) {
            (Ok(before), Ok(after)) => before == after,
            _ => false,
        };
        if migrated || !same {
            if let Err(e) = clean.save(root) {
                crate::applog::line(&format!("cannot save cleaned settings.json ({e})"));
            }
        }
        clean
    }

    /// Saves settings atomically, keeping a backup of the previous valid settings.json.
    ///
    /// The temporary file is written and flushed to disk before the rename, so a power loss
    /// leaves either the old file or the complete new one — never a truncated one (P2-43).
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(root)?;
        let path = Self::path(root);
        let bak = Self::backup_path(root);
        let tmp = path.with_extension("json.tmp");
        // Two saves at once would both use the same temporary name.
        let _guard = crate::lock_or_recover(&SAVE_LOCK);

        let bytes = serde_json::to_vec_pretty(self)?;
        {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }

        if path.exists() {
            // Keep the previous valid version as a backup. A failed copy must not keep the new
            // settings from being saved, but it is worth a line in the log.
            if let Err(e) = std::fs::copy(&path, &bak) {
                crate::applog::line(&format!("cannot back up settings.json ({e})"));
            }
        }

        if let Err(e) = std::fs::rename(&tmp, &path) {
            crate::applog::line(&format!("cannot replace settings.json ({e})"));
            return Err(e);
        }
        Ok(())
    }

    /// Moves the server to the top of the history.
    pub fn remember_played(&mut self, address: &str, name: Option<String>, when: String) {
        let address = canonical_or_trimmed(address);
        let name = name.map(|n| clean_name(&n)).filter(|n| !n.is_empty());
        let (play_seconds, account) = self
            .history
            .iter()
            .find(|h| h.address == address)
            .map_or((0, None), |h| (h.play_seconds, h.account.clone()));
        self.history.retain(|h| h.address != address);
        self.history.insert(
            0,
            HistoryEntry {
                address,
                name,
                last_played: when,
                play_seconds,
                account,
            },
        );
        self.history.truncate(HISTORY_LIMIT);
    }

    /// Notes which account was used on a server that is in the history.
    pub fn remember_account(&mut self, address: &str, account: &str) {
        let address = canonical_or_trimmed(address);
        if let Some(h) = self.history.iter_mut().find(|h| h.address == address) {
            h.account = Some(account.to_string());
        }
    }

    /// Adds time played to a server that is in the history (a server that dropped off the
    /// list in the meantime is simply not counted).
    pub fn add_play_time(&mut self, address: &str, seconds: u64) {
        let address = canonical_or_trimmed(address);
        if let Some(h) = self.history.iter_mut().find(|h| h.address == address) {
            h.play_seconds = h.play_seconds.saturating_add(seconds);
        }
    }

    /// Adds or removes a favourite. Returns whether it is a favourite now.
    pub fn toggle_favorite(&mut self, address: &str) -> bool {
        let address = canonical_or_trimmed(address);
        if let Some(i) = self.favorites.iter().position(|a| *a == address) {
            self.favorites.remove(i);
            self.favorite_names.remove(&address);
            false
        } else if self.favorites.len() < FAVORITES_LIMIT {
            self.favorites.push(address);
            true
        } else {
            crate::applog::line(&format!(
                "favorites: the list is full ({FAVORITES_LIMIT}), {address} not added"
            ));
            false
        }
    }

    /// Adds a favourite unless it is already there or the list is full. Returns whether it is
    /// in the list afterwards.
    pub fn add_favorite(&mut self, address: &str) -> bool {
        let address = canonical_or_trimmed(address);
        if self.favorites.contains(&address) {
            return true;
        }
        if self.favorites.len() >= FAVORITES_LIMIT {
            crate::applog::line(&format!(
                "favorites: the list is full ({FAVORITES_LIMIT}), {address} not added"
            ));
            return false;
        }
        self.favorites.push(address);
        true
    }

    /// Moves a favourite server up (delta < 0) or down (delta > 0) in the list.
    pub fn move_favorite(&mut self, address: &str, delta: i32) {
        let address = canonical_or_trimmed(address);
        if let Some(i) = self.favorites.iter().position(|a| *a == address) {
            let len = self.favorites.len();
            if len <= 1 {
                return;
            }
            let target = (i as i32 + delta).clamp(0, (len - 1) as i32) as usize;
            if target != i {
                let item = self.favorites.remove(i);
                self.favorites.insert(target, item);
            }
        }
    }

    /// Renames or changes the address of an existing favourite.
    pub fn edit_favorite(
        &mut self,
        old_address: &str,
        new_address: &str,
        new_name: Option<String>,
    ) {
        let old_address = canonical_or_trimmed(old_address);
        let new_address = canonical_or_trimmed(new_address);
        if let Some(i) = self.favorites.iter().position(|a| *a == old_address) {
            // The old alias goes away in every case: the address it belonged to is gone.
            let old_name = self.favorite_names.remove(&old_address);
            let saved_name = match new_name {
                Some(name) => Some(name),
                None => old_name,
            };
            self.favorites[i] = new_address.clone();
            match saved_name {
                Some(name) => self.set_favorite_name(&new_address, Some(name)),
                None => {
                    self.favorite_names.remove(&new_address);
                }
            }
            // One address can only be a favourite once.
            let mut seen = std::collections::HashSet::new();
            self.favorites.retain(|a| seen.insert(a.clone()));
        }
    }

    /// Sets or clears a custom alias for a favourite server.
    pub fn set_favorite_name(&mut self, address: &str, name: Option<String>) {
        let address = canonical_or_trimmed(address);
        if let Some(n) = name {
            let trimmed = clean_name(&n);
            if !trimmed.is_empty() {
                self.favorite_names.insert(address, trimmed);
                return;
            }
        }
        self.favorite_names.remove(&address);
    }

    /// Sets or removes a personal note for a server address.
    pub fn set_server_note(&mut self, address: &str, note: &str) {
        let address = canonical_or_trimmed(address);
        let final_text = clean_note(note, 500);
        if final_text.is_empty() {
            self.server_notes.remove(&address);
        } else {
            self.server_notes.insert(address, final_text);
        }
    }

    /// Cleans up values typed by the user.
    pub fn sanitized(mut self) -> Self {
        let name: String = self
            .guest_name
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .chars()
            .take(32)
            .collect();
        self.guest_name = if name.is_empty() {
            FALLBACK_USERNAME.to_string()
        } else {
            name
        };

        self.custom_theme = self.custom_theme.sanitized();

        // A font name ends up in a CSS declaration: allow only what a family name needs.
        self.font = self
            .font
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
            .collect::<String>()
            .trim()
            .chars()
            .take(64)
            .collect();

        // A proxy that cannot be used is dropped rather than kept to fail on every start.
        self.proxy = self.proxy.trim().to_string();
        if !bebra_core::http::is_valid_proxy(&self.proxy) {
            self.proxy.clear();
        }

        // The repository ends up in a URL: it must look like `owner/name`.
        self.update_repo = self.update_repo.trim().to_string();
        if !bebra_core::self_update::valid_repo(&self.update_repo) {
            self.update_repo = bebra_core::self_update::DEFAULT_REPO.to_string();
        }

        // Pack ids are plain file names, each at most once.
        let mut seen = std::collections::HashSet::new();
        self.enabled_packs.retain(|id| {
            !id.is_empty()
                && id.len() <= 200
                && !id.contains(['/', '\\'])
                && seen.insert(id.clone())
        });

        // Patch ids are plain file names, each at most once.
        let mut seen_patches = std::collections::HashSet::new();
        self.enabled_patches.retain(|id| {
            !id.is_empty()
                && id.len() <= 200
                && !id.contains(['/', '\\'])
                && seen_patches.insert(id.clone())
        });

        // Hubs: normalize URLs, filter out invalid/duplicate URLs.
        let mut seen_hubs = std::collections::HashSet::new();
        let mut clean_hubs = Vec::new();
        for h in self.hubs {
            if let Some(norm) = bebra_core::hub::normalize_hub_url(&h.url) {
                if seen_hubs.insert(norm.clone()) {
                    clean_hubs.push(HubSetting {
                        url: norm,
                        enabled: h.enabled,
                    });
                }
            }
        }
        if clean_hubs.is_empty() {
            clean_hubs = default_hubs();
        }
        self.hubs = clean_hubs;

        // Extra args: clean control characters, limit length.
        self.extra_args = self
            .extra_args
            .chars()
            .filter(|c| !c.is_control() || *c == ' ' || *c == '\t')
            .take(1000)
            .collect::<String>()
            .trim()
            .to_string();

        // Profiles of older versions map onto the current ones, so the options tab always has an
        // entry to show: "potato" and "iskander" became "optimized", "off"/"none" became
        // "standard", and "max_fps" (uncapped FPS without the lighting cuts) lives on as the
        // same switches under "custom".
        match self.opt_profile.as_str() {
            "standard" | "balanced" | "optimized" | "custom" => {}
            "off" | "none" => self.opt_profile = "standard".to_string(),
            "potato" | "iskander" => self.opt_profile = "optimized".to_string(),
            "max_fps" => {
                self.opt_profile = "custom".to_string();
                self.opt_server_gc = true;
                self.opt_fast_lighting = false;
                self.opt_unlock_fps = true;
                self.opt_high_priority = true;
            }
            _ => self.opt_profile = "balanced".to_string(),
        }

        match self.bg_type.as_str() {
            "space" | "custom" | "none" => {}
            _ => self.bg_type = "space".to_string(),
        }

        self.bg_custom_name = self.bg_custom_name.and_then(|name| {
            let clean: String = name
                .chars()
                .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
                .take(200)
                .collect();
            let trimmed = clean.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

        self.bg_dim = self.bg_dim.min(95);
        self.bg_blur = self.bg_blur.min(30);

        match self.client_window_mode.as_str() {
            "default" | "windowed" | "fullscreen" | "borderless" => {}
            _ => self.client_window_mode = "default".to_string(),
        }

        match self.client_ui_scale.as_str() {
            "auto" | "0.75" | "1.0" | "1.25" | "1.5" | "1.75" | "2.0" => {}
            _ => self.client_ui_scale = "auto".to_string(),
        }

        self.client_max_fps = self.client_max_fps.min(360);

        match self.auto_refresh_interval {
            0 | 15 | 30 | 60 | 120 | 300 => {}
            _ => self.auto_refresh_interval = 0,
        }

        self.launcher_ui_scale = self.launcher_ui_scale.clamp(75, 200);

        // A stored HWID must be exactly 32 bytes of hex; anything else is dropped rather than
        // passed to the client patch.
        let clean_hwid = |v: Option<String>| {
            v.map(|s| s.trim().to_string())
                .filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
                .map(|s| s.to_ascii_lowercase())
        };
        self.hwid_current = clean_hwid(self.hwid_current);
        self.hwid_previous = clean_hwid(self.hwid_previous);

        // Favourites are server addresses: one spelling each, no duplicates, and no more than
        // the list is ever asked to ping (P2-37).
        let mut seen_favorites = std::collections::HashSet::new();
        self.favorites = self
            .favorites
            .iter()
            .filter_map(|a| canonical_address(a))
            .filter(|a| seen_favorites.insert(a.clone()))
            .take(FAVORITES_LIMIT)
            .collect();

        let mut seen_history = std::collections::HashSet::new();
        let mut clean_history = Vec::new();
        for mut h in std::mem::take(&mut self.history) {
            let Some(addr) = canonical_address(&h.address) else {
                continue;
            };
            if !seen_history.insert(addr.clone()) {
                continue;
            }
            h.address = addr;
            h.name = h.name.map(|n| clean_name(&n)).filter(|n| !n.is_empty());
            clean_history.push(h);
            if clean_history.len() >= HISTORY_LIMIT {
                break;
            }
        }
        self.history = clean_history;

        let mut clean_notes = std::collections::HashMap::new();
        for (addr, note) in std::mem::take(&mut self.server_notes) {
            if clean_notes.len() >= SERVER_NOTES_LIMIT {
                break;
            }
            let Some(addr) = canonical_address(&addr) else {
                continue;
            };
            let trimmed = clean_note(&note, 500);
            if !trimmed.is_empty() {
                clean_notes.insert(addr, trimmed);
            }
        }
        self.server_notes = clean_notes;

        let mut clean_fav_names = std::collections::HashMap::new();
        for (addr, name) in std::mem::take(&mut self.favorite_names) {
            if clean_fav_names.len() >= FAVORITES_LIMIT {
                break;
            }
            let Some(addr) = canonical_address(&addr) else {
                continue;
            };
            let trimmed = clean_name(&name);
            if !trimmed.is_empty() {
                clean_fav_names.insert(addr, trimmed);
            }
        }
        self.favorite_names = clean_fav_names;

        self
    }

    /// Resolves the optimization profile into a concrete runtime configuration.
    pub fn optimization_config(&self) -> bebra_core::launch::OptimizationConfig {
        match self.opt_profile.as_str() {
            "standard" => bebra_core::launch::OptimizationConfig {
                profile: "standard".into(),
                server_gc: false,
                fast_lighting: false,
                unlock_fps: false,
                high_priority: false,
                light_resolution: None,
            },
            "optimized" => bebra_core::launch::OptimizationConfig {
                profile: "optimized".into(),
                server_gc: true,
                fast_lighting: true,
                // The profile caps the frame rate at 60 itself (`build_command`).
                unlock_fps: false,
                high_priority: true,
                light_resolution: Some(0.35),
            },
            "custom" => bebra_core::launch::OptimizationConfig {
                profile: "custom".into(),
                server_gc: self.opt_server_gc,
                fast_lighting: self.opt_fast_lighting,
                unlock_fps: self.opt_unlock_fps,
                high_priority: self.opt_high_priority,
                light_resolution: if self.opt_fast_lighting {
                    Some(0.35)
                } else {
                    None
                },
            },
            _ => bebra_core::launch::OptimizationConfig {
                profile: "balanced".into(),
                server_gc: true,
                fast_lighting: false,
                unlock_fps: false,
                high_priority: true,
                light_resolution: None,
            },
        }
    }

    /// Moves a favourite to the top of the list.
    pub fn raise_favorite(&mut self, address: &str) {
        let address = canonical_or_trimmed(address);
        if let Some(i) = self.favorites.iter().position(|a| *a == address) {
            let item = self.favorites.remove(i);
            self.favorites.insert(0, item);
        }
    }

    /// Defaults, keeping the things the player cannot retype: favourites and their names,
    /// notes, history and the account chosen last (P1-21).
    pub fn reset_keeping_user_data(&self) -> Self {
        Settings {
            last_account: self.last_account.clone(),
            favorites: self.favorites.clone(),
            favorite_names: self.favorite_names.clone(),
            history: self.history.clone(),
            server_notes: self.server_notes.clone(),
            ..Settings::default()
        }
    }

    /// Settings coming from the UI, merged with the current ones (P1-20).
    ///
    /// The UI works on a snapshot and sends the whole object back. Some fields are also written
    /// by the launcher itself: `enabled_patches` (`install_patch`/`delete_patch`),
    /// `bg_custom_name` (`delete_background`), `dismissed_intel_warning`
    /// (`dismiss_intel_warning`). For those, a value equal to `seen` — the settings the UI was
    /// last given — means the UI did not touch it, and the current value wins; the UI's copy may
    /// be older than what the launcher wrote. A different value is a deliberate edit by the UI
    /// and is taken as is.
    ///
    /// Favourites, their names, notes, history and the last account are always the launcher's:
    /// they are changed by their own commands and the UI only shows them. `schema_version` is
    /// always ours too: a UI must not be able to roll a migration back.
    pub fn merged_from_ui(&self, seen: &Settings, incoming: Settings) -> Self {
        /// Keep the current value unless the UI changed it since it was last told about it.
        fn untouched<T: PartialEq>(current: &T, seen: &T, incoming: &T) -> bool {
            incoming == seen && current != seen
        }
        fn pick<T: PartialEq + Clone>(current: &T, seen: &T, incoming: &T) -> T {
            if untouched(current, seen, incoming) {
                current.clone()
            } else {
                incoming.clone()
            }
        }

        Settings {
            schema_version: CURRENT_SCHEMA_VERSION,
            last_account: self.last_account.clone(),
            favorites: self.favorites.clone(),
            favorite_names: self.favorite_names.clone(),
            history: self.history.clone(),
            server_notes: self.server_notes.clone(),
            // Changed by their own commands (`start_play`, `set_hwid_auto`,
            // `regenerate_hwid`, `restore_hwid`) and possibly while the UI still holds an older
            // snapshot: an unrelated save must not roll them back.
            hwid_auto: self.hwid_auto,
            hwid_current: self.hwid_current.clone(),
            hwid_previous: self.hwid_previous.clone(),
            dismissed_intel_warning: pick(
                &self.dismissed_intel_warning,
                &seen.dismissed_intel_warning,
                &incoming.dismissed_intel_warning,
            ),
            enabled_patches: pick(
                &self.enabled_patches,
                &seen.enabled_patches,
                &incoming.enabled_patches,
            ),
            bg_custom_name: pick(
                &self.bg_custom_name,
                &seen.bg_custom_name,
                &incoming.bg_custom_name,
            ),
            ..incoming
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = tempdir();
        assert_eq!(Settings::load(&dir).guest_name, FALLBACK_USERNAME);
        assert!(!Settings::load(&dir).minimize_on_play);
        assert!(Settings::load(&dir).auto_clean_cache);

        let s = Settings {
            guest_name: "Bebra".into(),
            compat_mode: true,
            last_account: Some("x".into()),
            ..Settings::default()
        };
        s.save(&dir).unwrap();
        let back = Settings::load(&dir);
        assert_eq!(back.guest_name, "Bebra");
        assert!(back.compat_mode);
        assert_eq!(back.last_account.as_deref(), Some("x"));
    }

    #[test]
    fn broken_file_gives_defaults_and_partial_file_fills_in() {
        let dir = tempdir();
        std::fs::write(Settings::path(&dir), b"{ not json").unwrap();
        assert_eq!(Settings::load(&dir).guest_name, FALLBACK_USERNAME);

        std::fs::write(Settings::path(&dir), br#"{"compat_mode": true}"#).unwrap();
        let s = Settings::load(&dir);
        assert!(s.compat_mode);
        assert_eq!(s.guest_name, FALLBACK_USERNAME);
    }

    #[test]
    fn corrupt_settings_restores_from_backup_and_quarantines() {
        let dir = tempdir();
        let s = Settings {
            guest_name: "SavedPlayer".into(),
            compat_mode: true,
            ..Settings::default()
        };
        // First save creates settings.json
        s.save(&dir).unwrap();
        // Second save creates settings.json.bak
        let s2 = Settings {
            guest_name: "UpdatedPlayer".into(),
            compat_mode: true,
            ..Settings::default()
        };
        s2.save(&dir).unwrap();

        assert!(Settings::backup_path(&dir).exists());

        // Corrupt main settings.json
        std::fs::write(Settings::path(&dir), b"CORRUPTED_JSON_DATA_GARBAGE").unwrap();

        // Load should recover from backup
        let loaded = Settings::load(&dir);
        assert_eq!(loaded.guest_name, "SavedPlayer");

        // The corrupt file should have been moved/quarantined
        let files = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(files
            .iter()
            .any(|f| f.starts_with("settings.json.corrupt-")));
    }

    #[test]
    fn guest_name_is_cleaned() {
        let s = Settings {
            guest_name: "  \u{7}  ".into(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.guest_name, FALLBACK_USERNAME);

        let s = Settings {
            guest_name: format!(" {} ", "a".repeat(100)),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.guest_name.len(), 32);
    }

    #[test]
    fn history_is_newest_first_without_duplicates_and_limited() {
        let mut s = Settings::default();
        s.remember_played("ss14://a.test", None, "1".into());
        s.remember_played("ss14://b.test", Some("B".into()), "2".into());
        s.remember_played("ss14://a.test", Some("A".into()), "3".into());
        let order: Vec<&str> = s.history.iter().map(|h| h.address.as_str()).collect();
        assert_eq!(order, ["ss14://a.test", "ss14://b.test"]);
        assert_eq!(s.history[0].name.as_deref(), Some("A"));

        for i in 0..50 {
            s.remember_played(&format!("ss14://s{i}.test"), None, i.to_string());
        }
        assert_eq!(s.history.len(), HISTORY_LIMIT);
        assert_eq!(s.history[0].address, "ss14://s49.test");
    }

    #[test]
    fn play_time_accumulates_and_survives_replaying() {
        let mut s = Settings::default();
        s.remember_played("ss14://a.test", None, "1".into());
        s.add_play_time("ss14://a.test", 60);
        s.add_play_time("ss14://a.test", 30);
        s.add_play_time("ss14://unknown.test", 999);
        s.remember_played("ss14://b.test", None, "2".into());
        s.remember_played("ss14://a.test", None, "3".into());
        assert_eq!(s.history[0].address, "ss14://a.test");
        assert_eq!(s.history[0].play_seconds, 90);
        assert_eq!(s.history[1].play_seconds, 0);
    }

    #[test]
    fn the_account_is_remembered_per_server() {
        let mut s = Settings::default();
        s.remember_played("ss14://a.test", None, "1".into());
        s.remember_account("ss14://a.test", "user-1");
        s.remember_account("ss14://unknown.test", "x");
        s.remember_played("ss14://b.test", None, "2".into());
        s.remember_account("ss14://b.test", "");
        s.remember_played("ss14://a.test", None, "3".into());
        let a = s
            .history
            .iter()
            .find(|h| h.address == "ss14://a.test")
            .unwrap();
        let b = s
            .history
            .iter()
            .find(|h| h.address == "ss14://b.test")
            .unwrap();
        assert_eq!(
            a.account.as_deref(),
            Some("user-1"),
            "survives playing again"
        );
        assert_eq!(b.account.as_deref(), Some(""), "guest is a choice too");
        assert_eq!(s.history.len(), 2);
    }

    #[test]
    fn colours_and_fonts_are_cleaned() {
        let s = Settings {
            font: "Comic Sans MS; } body { display:none".into(),
            custom_theme: CustomTheme {
                background: "#abcdef".into(),
                accent: "red".into(),
                foreground: "#12345".into(),
                popup: "url(javascript:1)".into(),
                gradient_start: "#00FF00".into(),
                gradient_end: "".into(),
            },
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.font, "Comic Sans MS  body  displaynone");
        assert_eq!(s.custom_theme.background, "#ABCDEF");
        assert_eq!(s.custom_theme.accent, CustomTheme::default().accent);
        assert_eq!(s.custom_theme.foreground, CustomTheme::default().foreground);
        assert_eq!(s.custom_theme.popup, CustomTheme::default().popup);
        assert_eq!(s.custom_theme.gradient_start, "#00FF00");
        assert_eq!(
            s.custom_theme.gradient_end,
            CustomTheme::default().gradient_end
        );
    }

    #[test]
    fn unusable_proxies_are_dropped() {
        let ok = Settings {
            proxy: " socks5://127.0.0.1:1080 ".into(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(ok.proxy, "socks5://127.0.0.1:1080");
        let bad = Settings {
            proxy: "not a proxy".into(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(bad.proxy, "");
    }

    #[test]
    fn hwid_values_must_be_32_bytes_of_hex() {
        let s = Settings {
            hwid_current: Some("not a hardware id".into()),
            hwid_previous: Some("AB".repeat(32)),
            ..Settings::default()
        }
        .sanitized();
        assert!(s.hwid_current.is_none(), "an unusable value is dropped");
        assert_eq!(s.hwid_previous.as_deref(), Some("ab".repeat(32).as_str()));
    }

    #[test]
    fn pack_ids_are_plain_unique_names() {
        let s = Settings {
            enabled_packs: vec![
                "a.zip".into(),
                "../evil.zip".into(),
                "dir/x.zip".into(),
                "a.zip".into(),
                "".into(),
                "b.zip".into(),
            ],
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.enabled_packs, ["a.zip", "b.zip"]);
    }

    #[test]
    fn raising_a_favorite_moves_it_to_the_top() {
        let mut s = Settings::default();
        for a in ["ss14://a.test", "ss14://b.test", "ss14://c.test"] {
            s.toggle_favorite(a);
        }
        s.raise_favorite("ss14://c.test");
        s.raise_favorite("ss14://missing.test");
        assert_eq!(
            s.favorites,
            ["ss14://c.test", "ss14://a.test", "ss14://b.test"]
        );
    }

    #[test]
    fn old_settings_files_still_load() {
        // A file from before these options existed.
        let json = r#"{"guest_name":"X","theme":"ocean","favorites":["a"]}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.guest_name, "X");
        assert!(!s.theme_gradient && s.columns.players && !s.columns.ping);
        assert_eq!(s.favorites, ["a"]);
        assert_eq!(s.hubs.len(), 1);
        assert_eq!(s.extra_args, "");
        assert!(s.enabled_patches.is_empty());
        assert!(s.discord_rpc);
    }

    /// A file written before the schema field existed is version 0: it is migrated, marked as
    /// the current version and written back, and its values survive (P2-39).
    #[test]
    fn a_version_zero_file_is_migrated_and_kept() {
        let dir = tempdir();
        let old = br#"{"guest_name":"Old","theme":"light","favorites":["ss14s://a.test"]}"#;
        let (settings, source) = parse(old).unwrap();
        assert_eq!(source, SchemaSource::Older);
        assert_eq!(settings.schema_version, CURRENT_SCHEMA_VERSION);

        std::fs::write(Settings::path(&dir), old).unwrap();
        let loaded = Settings::load(&dir);
        assert_eq!(loaded.guest_name, "Old");
        assert_eq!(loaded.theme, "light");
        assert_eq!(loaded.favorites, ["ss14s://a.test"]);

        // The migrated file is on disk, so the next start does not migrate again.
        let (_, source) = parse(&std::fs::read(Settings::path(&dir)).unwrap()).unwrap();
        assert_eq!(source, SchemaSource::Current);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file from a newer launcher is read as it is and not rewritten (unknown fields would be
    /// lost otherwise).
    #[test]
    fn a_newer_file_is_not_downgraded() {
        let dir = tempdir();
        let json = br#"{"schema_version":99,"guest_name":"New","future_option":true}"#;
        let (settings, source) = parse(json).unwrap();
        assert_eq!(source, SchemaSource::Newer);
        assert_eq!(settings.guest_name, "New");

        std::fs::write(Settings::path(&dir), json).unwrap();
        let loaded = Settings::load(&dir);
        assert_eq!(loaded.schema_version, 99);
        let on_disk = std::fs::read(Settings::path(&dir)).unwrap();
        assert!(
            String::from_utf8_lossy(&on_disk).contains("future_option"),
            "the file is left alone"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Favourites, history, names and notes are cleaned on load: one spelling of an address,
    /// no duplicates, no control characters, and no more entries than the UI handles (P2-37).
    #[test]
    fn favorites_history_and_notes_are_validated() {
        let mut s = Settings {
            favorites: vec![
                " http://a.test:1212 ".into(),
                "ss14://a.test:1212".into(), // the same server, other spelling
                "ss14s://b.test/".into(),
                "not an address \u{7}".into(),
                "ss14://nul".into(),
            ],
            history: vec![
                HistoryEntry {
                    address: "ss14://a.test:1212".into(),
                    name: Some(format!("{}{}", "\u{7}", "x".repeat(400))),
                    last_played: "1".into(),
                    play_seconds: 5,
                    account: None,
                },
                HistoryEntry {
                    address: "  ss14://a.test:1212  ".into(),
                    name: None,
                    last_played: "2".into(),
                    play_seconds: 9,
                    account: None,
                },
            ],
            favorite_names: [
                (
                    " ss14://a.test:1212 ".to_string(),
                    "  Имя \u{7} ".to_string(),
                ),
                ("not a server".to_string(), "nope".to_string()),
            ]
            .into_iter()
            .collect(),
            server_notes: [
                ("ss14://a.test:1212".to_string(), "note".to_string()),
                ("  ".to_string(), "orphan".to_string()),
            ]
            .into_iter()
            .collect(),
            ..Settings::default()
        };
        for i in 0..(FAVORITES_LIMIT + 10) {
            s.favorites.push(format!("ss14://s{i}.test"));
        }

        let s = s.sanitized();
        assert_eq!(s.favorites[0], "ss14://a.test:1212");
        assert_eq!(s.favorites[1], "ss14s://b.test/");
        assert!(s.favorites.len() <= FAVORITES_LIMIT);
        assert_eq!(
            s.favorites
                .iter()
                .filter(|a| *a == "ss14://a.test:1212")
                .count(),
            1,
            "one server is one favourite"
        );

        assert_eq!(s.history.len(), 1, "the same server is remembered once");
        assert_eq!(s.history[0].address, "ss14://a.test:1212");
        let name = s.history[0].name.as_deref().unwrap();
        assert_eq!(name.chars().count(), NAME_LIMIT);
        assert!(!name.chars().any(char::is_control));

        assert_eq!(
            s.favorite_names
                .get("ss14://a.test:1212")
                .map(String::as_str),
            Some("Имя")
        );
        assert_eq!(s.favorite_names.len(), 1, "a non-address key is dropped");
        assert_eq!(
            s.server_notes.get("ss14://a.test:1212").map(String::as_str),
            Some("note")
        );
        assert_eq!(s.server_notes.len(), 1);
    }

    #[test]
    fn hubs_and_extra_args_are_sanitized() {
        let s = Settings {
            hubs: vec![
                HubSetting {
                    url: "https://hub.corvax.fun".into(),
                    enabled: true,
                },
                HubSetting {
                    url: "https://hub.corvax.fun/".into(), // duplicate
                    enabled: false,
                },
                HubSetting {
                    url: "invalid-url".into(),
                    enabled: true,
                },
            ],
            extra_args: "  --cvar display.vsync=false \0 \r\n ".into(),
            ..Settings::default()
        }
        .sanitized();

        assert_eq!(s.hubs.len(), 1);
        assert_eq!(s.hubs[0].url, "https://hub.corvax.fun/");
        assert!(s.hubs[0].enabled);
        assert_eq!(s.extra_args, "--cvar display.vsync=false");

        // Empty hubs list gets restored to default
        let empty_hubs = Settings {
            hubs: vec![],
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(empty_hubs.hubs.len(), 1);
        assert_eq!(empty_hubs.hubs[0].url, bebra_core::hub::OFFICIAL_HUB_URL);
    }

    #[test]
    fn favorites_toggle() {
        let mut s = Settings::default();
        assert!(s.toggle_favorite("ss14://a.test"));
        assert!(s.toggle_favorite("ss14://b.test"));
        assert!(!s.toggle_favorite("ss14://a.test"));
        assert_eq!(s.favorites, ["ss14://b.test"]);
    }

    #[test]
    fn optimization_profiles_and_config() {
        let mut s = Settings::default();
        assert_eq!(s.opt_profile, "balanced");
        let cfg = s.optimization_config();
        assert_eq!(cfg.profile, "balanced");
        assert!(cfg.server_gc);
        assert!(!cfg.fast_lighting);
        assert!(!cfg.unlock_fps);
        assert!(cfg.high_priority);

        s.opt_profile = "optimized".into();
        let cfg_opt = s.optimization_config();
        assert_eq!(cfg_opt.profile, "optimized");
        assert!(cfg_opt.fast_lighting);
        assert!(!cfg_opt.unlock_fps, "the profile caps at 60 on its own");
        assert_eq!(cfg_opt.light_resolution, Some(0.35));

        s.opt_profile = "standard".into();
        let cfg_std = s.optimization_config();
        assert_eq!(cfg_std.profile, "standard");
        assert!(!cfg_std.server_gc);
        assert!(!cfg_std.fast_lighting);

        s.opt_profile = "custom".into();
        s.opt_server_gc = false;
        s.opt_fast_lighting = true;
        s.opt_unlock_fps = true;
        s.opt_high_priority = false;
        let cfg_custom = s.optimization_config();
        assert_eq!(cfg_custom.profile, "custom");
        assert!(!cfg_custom.server_gc);
        assert!(cfg_custom.fast_lighting);
        assert!(cfg_custom.unlock_fps);
        assert!(!cfg_custom.high_priority);

        s.opt_profile = "invalid_name".into();
        let san = s.sanitized();
        assert_eq!(san.opt_profile, "balanced");
    }

    #[test]
    fn profiles_of_older_versions_are_migrated() {
        let migrate = |profile: &str| {
            Settings {
                opt_profile: profile.into(),
                ..Settings::default()
            }
            .sanitized()
        };
        assert_eq!(migrate("potato").opt_profile, "optimized");
        assert_eq!(migrate("iskander").opt_profile, "optimized");
        assert_eq!(migrate("off").opt_profile, "standard");
        assert_eq!(migrate("none").opt_profile, "standard");

        // "max_fps" keeps its behaviour as the same switches under "custom".
        let fps = migrate("max_fps");
        assert_eq!(fps.opt_profile, "custom");
        let cfg = fps.optimization_config();
        assert!(cfg.server_gc && cfg.unlock_fps && cfg.high_priority);
        assert!(!cfg.fast_lighting);
    }

    #[test]
    fn background_and_client_settings_sanitized() {
        let s = Settings {
            bg_type: "unknown_mode".into(),
            bg_custom_name: Some("  ../bad/path\\name.png \0 ".into()),
            bg_dim: 150,
            bg_blur: 50,
            client_window_mode: "invalid".into(),
            client_ui_scale: "3.5".into(),
            client_max_fps: 1000,
            auto_refresh_interval: 999,
            ..Settings::default()
        }
        .sanitized();

        assert_eq!(s.bg_type, "space");
        assert_eq!(s.bg_custom_name.as_deref(), Some("..badpathname.png"));
        assert_eq!(s.bg_dim, 95);
        assert_eq!(s.bg_blur, 30);
        assert_eq!(s.client_window_mode, "default");
        assert_eq!(s.client_ui_scale, "auto");
        assert_eq!(s.client_max_fps, 360);
        assert_eq!(s.auto_refresh_interval, 0);
    }

    #[test]
    fn server_notes_sanitization_and_management() {
        let mut s = Settings::default();
        s.set_server_note("ss14s://corvax.example", "  Главный инженер на смене  ");
        assert_eq!(
            s.server_notes
                .get("ss14s://corvax.example")
                .map(String::as_str),
            Some("Главный инженер на смене")
        );

        // Blank note removes the entry
        s.set_server_note("ss14s://corvax.example", "   ");
        assert!(!s.server_notes.contains_key("ss14s://corvax.example"));

        // Sanitization trims, drops empty/invalid addresses and limits length
        s.server_notes.insert(
            "  ss14s://test.org  ".into(),
            "  Заметка \0 с нулём  ".into(),
        );
        s.server_notes.insert("   ".into(), "валидный текст".into());
        let san = s.sanitized();
        assert_eq!(
            san.server_notes.get("ss14s://test.org").map(String::as_str),
            Some("Заметка  с нулём")
        );
        assert!(!san.server_notes.contains_key("   "));
    }

    #[test]
    fn moving_and_editing_favorites() {
        let mut s = Settings::default();
        s.toggle_favorite("ss14://a.test");
        s.toggle_favorite("ss14://b.test");
        s.toggle_favorite("ss14://c.test");
        assert_eq!(
            s.favorites,
            ["ss14://a.test", "ss14://b.test", "ss14://c.test"]
        );

        s.set_favorite_name("ss14://a.test", Some("Alpha".into()));
        assert_eq!(
            s.favorite_names.get("ss14://a.test").map(String::as_str),
            Some("Alpha")
        );

        // Move b down
        s.move_favorite("ss14://b.test", 1);
        assert_eq!(
            s.favorites,
            ["ss14://a.test", "ss14://c.test", "ss14://b.test"]
        );

        // Move b up twice
        s.move_favorite("ss14://b.test", -2);
        assert_eq!(
            s.favorites,
            ["ss14://b.test", "ss14://a.test", "ss14://c.test"]
        );

        // Edit favorite a -> a2 with new name
        s.edit_favorite("ss14://a.test", "ss14://a2.test", Some("Alpha 2".into()));
        assert_eq!(
            s.favorites,
            ["ss14://b.test", "ss14://a2.test", "ss14://c.test"]
        );
        assert_eq!(
            s.favorite_names.get("ss14://a2.test").map(String::as_str),
            Some("Alpha 2")
        );
        assert!(!s.favorite_names.contains_key("ss14://a.test"));

        // Sanitization of favorite names and launcher UI scale
        s.favorite_names
            .insert("  ss14s://fav.org  ".into(), "  Имя \0 с нулём  ".into());
        s.favorite_names.insert("   ".into(), "имя".into());
        s.launcher_ui_scale = 300;
        let san = s.sanitized();
        assert_eq!(
            san.favorite_names
                .get("ss14s://fav.org")
                .map(String::as_str),
            Some("Имя  с нулём")
        );
        assert!(!san.favorite_names.contains_key("   "));
        assert_eq!(san.launcher_ui_scale, 200);
    }

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("bebra-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// A file that is valid JSON but holds values the UI cannot survive is cleaned at load,
    /// before the UI ever sees it, and the clean version is written back (P1-19).
    #[test]
    fn loading_cleans_the_file_and_keeps_the_clean_version() {
        let dir = tempdir();
        std::fs::write(
            Settings::path(&dir),
            br#"{"launcher_ui_scale":0,"bg_dim":500,"bg_blur":99,"guest_name":"  \u0007 bad  ","client_max_fps":100000,"theme":"dark"}"#,
        )
        .unwrap();

        let s = Settings::load(&dir);
        assert_eq!(s.launcher_ui_scale, 75, "the window stays usable");
        assert_eq!(s.bg_dim, 95);
        assert_eq!(s.bg_blur, 30);
        assert_eq!(s.guest_name, "bad");
        assert_eq!(s.client_max_fps, 360);
        assert_eq!(s.theme, "dark", "valid values are kept");

        let on_disk: Settings =
            serde_json::from_slice(&std::fs::read(Settings::path(&dir)).unwrap()).unwrap();
        assert_eq!(on_disk.launcher_ui_scale, 75);
        assert_eq!(on_disk.bg_dim, 95);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A clean file is not rewritten on every start.
    #[test]
    fn a_clean_settings_file_is_left_alone() {
        let dir = tempdir();
        Settings::default().save(&dir).unwrap();
        assert!(!Settings::backup_path(&dir).exists());

        let loaded = Settings::load(&dir);
        assert_eq!(loaded.guest_name, FALLBACK_USERNAME);
        assert!(
            !Settings::backup_path(&dir).exists(),
            "no save happened, so no backup of an unchanged file was made"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The backup-restored file is cleaned too, not returned as it was saved long ago.
    #[test]
    fn a_restored_backup_is_cleaned() {
        let dir = tempdir();
        std::fs::write(
            Settings::backup_path(&dir),
            br#"{"launcher_ui_scale":5,"guest_name":"Player"}"#,
        )
        .unwrap();
        let s = Settings::load(&dir);
        assert_eq!(s.launcher_ui_scale, 75);
        assert_eq!(s.guest_name, "Player");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A reset keeps what the player cannot retype: favourite names and server notes were lost
    /// by `..Settings::default()` before (P1-21).
    #[test]
    fn reset_keeps_user_data() {
        let mut current = Settings {
            theme: "light".into(),
            extra_args: "--cvar x=1".into(),
            enabled_patches: vec!["p.dll".into()],
            ..Settings::default()
        };
        current.toggle_favorite("ss14://a:1212");
        current.set_favorite_name("ss14://a:1212", Some("Свой сервер".into()));
        current.set_server_note("ss14://a:1212", "Тут играем по вечерам");
        current.remember_played("ss14://a:1212", Some("A".into()), "1".into());
        current.last_account = Some("user-1".into());

        let fresh = current.reset_keeping_user_data();
        assert_eq!(fresh.theme, "dark", "options go back to defaults");
        assert_eq!(fresh.extra_args, "");
        assert!(fresh.enabled_patches.is_empty());
        assert_eq!(fresh.favorites, ["ss14://a:1212"]);
        assert_eq!(
            fresh
                .favorite_names
                .get("ss14://a:1212")
                .map(String::as_str),
            Some("Свой сервер")
        );
        assert_eq!(
            fresh.server_notes.get("ss14://a:1212").map(String::as_str),
            Some("Тут играем по вечерам")
        );
        assert_eq!(fresh.history.len(), 1);
        assert_eq!(fresh.last_account.as_deref(), Some("user-1"));
    }

    /// The UI saves a snapshot. Fields the launcher wrote meanwhile must come from the current
    /// settings; a field the UI itself changed is taken from the request (P1-20).
    #[test]
    fn an_old_ui_snapshot_does_not_wipe_backend_fields() {
        // What the UI was last given.
        let seen = Settings {
            favorites: vec!["ss14://a:1212".into()],
            enabled_patches: vec!["a.dll".into()],
            bg_custom_name: Some("old.png".into()),
            ..Settings::default()
        };
        // What the launcher has meanwhile: a patch was installed, the warning dismissed and a
        // new HWID generated before the last launch.
        let current = Settings {
            enabled_patches: vec!["a.dll".into(), "b.dll".into()],
            dismissed_intel_warning: true,
            hwid_current: Some("ab".repeat(32)),
            hwid_previous: Some("cd".repeat(32)),
            ..seen.clone()
        };

        // The UI changes only the theme and sends its old copy of everything else.
        let stale = Settings {
            theme: "light".into(),
            schema_version: 999,
            ..seen.clone()
        };
        let merged = current.merged_from_ui(&seen, stale);
        assert_eq!(merged.theme, "light", "the UI's own fields are taken");
        assert!(merged.dismissed_intel_warning);
        assert_eq!(merged.enabled_patches, ["a.dll", "b.dll"]);
        assert_eq!(merged.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(
            merged.hwid_current.as_deref(),
            Some("ab".repeat(32).as_str()),
            "the UI's stale copy must not undo the HWID generated before a launch"
        );
        assert_eq!(
            merged.hwid_previous.as_deref(),
            Some("cd".repeat(32).as_str())
        );

        // The UI turns a patch off and picks another background itself: those values differ
        // from what it was given, so they win over the current ones.
        let edited = Settings {
            enabled_patches: Vec::new(),
            bg_custom_name: Some("other.png".into()),
            ..seen.clone()
        };
        let merged = current.merged_from_ui(&seen, edited);
        assert!(merged.enabled_patches.is_empty());
        assert_eq!(merged.bg_custom_name.as_deref(), Some("other.png"));
    }

    /// The launcher's own data is never taken from the UI's copy.
    #[test]
    fn the_ui_cannot_change_launcher_owned_data() {
        let mut current = Settings {
            favorites: vec!["ss14://a:1212".into()],
            favorite_names: [("ss14://a:1212".to_string(), "A".to_string())]
                .into_iter()
                .collect(),
            server_notes: [("ss14://a:1212".to_string(), "note".to_string())]
                .into_iter()
                .collect(),
            last_account: Some("user-1".into()),
            ..Settings::default()
        };
        current.remember_played("ss14://a:1212", Some("A".into()), "1".into());

        let incoming = Settings {
            favorites: Vec::new(),
            last_account: None,
            ..Settings::default()
        };
        let merged = current.merged_from_ui(&Settings::default(), incoming);
        assert_eq!(merged.favorites, ["ss14://a:1212"]);
        assert!(merged.favorite_names.contains_key("ss14://a:1212"));
        assert!(merged.server_notes.contains_key("ss14://a:1212"));
        assert_eq!(merged.history.len(), 1);
        assert_eq!(merged.last_account.as_deref(), Some("user-1"));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedHub {
    pub url: String,
    pub name: String,
}

pub fn recommended_hubs() -> Vec<RecommendedHub> {
    vec![
        RecommendedHub {
            url: "https://hub.spacestation14.com/".to_string(),
            name: "Wizards' Den".to_string(),
        },
        RecommendedHub {
            url: "https://hub.singularity14.co.uk/".to_string(),
            name: "Singularity".to_string(),
        },
        RecommendedHub {
            url: "https://hub.simplestation.org/".to_string(),
            name: "SimpleStation".to_string(),
        },
        RecommendedHub {
            url: "https://cdn.spacestationmultiverse.com/hub/".to_string(),
            name: "Multiverse".to_string(),
        },
    ]
}
