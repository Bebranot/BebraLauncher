//! Starting the game through `SS14.Loader` (docs/protocol.md, section 8).
//!
//! `build_command` only assembles arguments and environment, so it can be tested without running
//! anything. `spawn` runs it with the client's output going to log files.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::engine::InstalledEngine;
use crate::paths::DataDirs;
use crate::server::BuildInfo;
use crate::{Error, Result};

/// Name used when nobody is signed in (guest play on servers that allow it).
pub const FALLBACK_USERNAME: &str = "JoeGenero";

/// Variables the Loader and the client read. They must not leak from our own environment
/// into a launch that doesn't set them.
const CLEARED_ENV: &[&str] = &[
    "ROBUST_AUTH_TOKEN",
    "ROBUST_AUTH_USERID",
    "ROBUST_AUTH_PUBKEY",
    "ROBUST_AUTH_SERVER",
    "ROBUST_AUTH_SERVERS",
    "SS14_LOADER_CONTENT_DB",
    "SS14_LOADER_CONTENT_VERSION",
    "SS14_LOADER_OVERLAY_ZIP",
    "SS14_LOADER_PATCHES",
    "SS14_DISABLE_SIGNING",
    "SS14_LAUNCHER_PATH",
    "SS14_LOG_CLIENT",
    "DOTNET_gcServer",
    "DOTNET_GCDynamicAdaptationMode",
    "DOTNET_gcConcurrent",
    "DOTNET_TC_QuickJitForLoops",
    "BEBRA_OPTIMIZE",
    "DOTNET_ROLL_FORWARD",
];

/// Values passed to the client so it can prove who the player is. The token is a secret:
/// it never appears in `Debug` output or in [`LaunchCommand::describe`].
#[derive(Clone)]
pub struct AuthEnv {
    pub token: String,
    pub user_id: String,
    /// The server's `auth.public_key` from `/info`.
    pub server_public_key: String,
    pub auth_server_url: String,
}

impl std::fmt::Debug for AuthEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthEnv")
            .field("token", &"<hidden>")
            .field("user_id", &self.user_id)
            .field("auth_server_url", &self.auth_server_url)
            .finish_non_exhaustive()
    }
}

/// Where to connect. Without it the client starts at its main menu.
#[derive(Debug, Clone)]
pub struct ConnectTarget {
    /// `udp://host:port/` game address.
    pub connect_url: String,
    /// The `ss14://` address the player picked, normalized.
    pub server_address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessPriority {
    #[default]
    Normal,
    AboveNormal,
    High,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OptimizationConfig {
    /// Optimization profile: "off", "balanced", "max_fps", "potato", or "custom".
    pub profile: String,
    /// Enable .NET Server GC, concurrent mode, and DATAS.
    pub server_gc: bool,
    /// Optimize lighting by disabling soft shadows and blur.
    pub fast_lighting: bool,
    /// Unlock max FPS and disable VSync.
    pub unlock_fps: bool,
    /// Process priority in Windows (High or AboveNormal).
    pub high_priority: bool,
    /// Scale of the lighting grid resolution (e.g. 0.35 on potato, None for default).
    pub light_resolution: Option<f32>,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            server_gc: true,
            fast_lighting: false,
            unlock_fps: false,
            high_priority: true,
            light_resolution: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub loader_exe: PathBuf,
    /// Folder of the .NET runtime to use (`DOTNET_ROOT`). `None` = the machine's installed one.
    pub dotnet_root: Option<PathBuf>,
    pub engine: InstalledEngine,
    pub signing_key_file: PathBuf,
    pub content_db: PathBuf,
    pub content_version_id: i64,
    /// `(module name, folder)`, for example `("Robust.Client.WebView", ".../modules/...")`.
    pub modules: Vec<(String, PathBuf)>,
    pub username: String,
    pub auth: Option<AuthEnv>,
    pub build: BuildInfo,
    pub connect: Option<ConnectTarget>,
    /// Run the client in the GLES2 fallback renderer.
    pub compat_mode: bool,
    /// Our own executable. The client starts it with `--commands …` to ask for a reconnect
    /// (`SS14_LAUNCHER_PATH`). `None` = the client cannot redial.
    pub launcher_path: Option<PathBuf>,
    /// Overlay zip of text packs (`SS14_LOADER_OVERLAY_ZIP`); see [`crate::packs`].
    pub overlay_zip: Option<PathBuf>,
    /// Additional arguments passed to the engine (e.g. custom cvars).
    pub extra_args: Vec<String>,
    /// Client patch assemblies (.dll) to load (`SS14_LOADER_PATCHES`).
    pub patches: Vec<PathBuf>,
    /// Client performance and optimization configuration.
    pub optimization: OptimizationConfig,
}

#[derive(Clone)]
pub struct LaunchCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub priority: ProcessPriority,
}

const SECRET_ENV: &[&str] = &["ROBUST_AUTH_TOKEN"];

/// Environment variables whose values may be printed by [`LaunchCommand::describe_verbose`].
/// Empty on purpose: these logs are attached to bug reports, so only a deliberate addition
/// should put a value (a path, a token) into them.
const SHOWN_ENV_VALUES: &[&str] = &[];

impl LaunchCommand {
    /// A log-safe one-line description: program, arguments and the names of the environment
    /// variables. Values are never included; use [`describe_verbose`](Self::describe_verbose)
    /// when the exact values are needed for debugging.
    pub fn describe(&self) -> String {
        self.describe_with(|_, _| None)
    }

    /// Like [`describe`](Self::describe), but prints values for the few variables in the
    /// explicit `SHOWN_ENV_VALUES` allowlist. Everything else, secrets in particular, stays hidden.
    pub fn describe_verbose(&self) -> String {
        self.describe_with(|k, v| {
            let shown = SHOWN_ENV_VALUES.contains(&k) && !SECRET_ENV.contains(&k);
            Some(if shown {
                v.to_string()
            } else {
                "<hidden>".into()
            })
        })
    }

    fn describe_with<F>(&self, value_for: F) -> String
    where
        F: Fn(&str, &str) -> Option<String>,
    {
        let mut out = self.program.display().to_string();
        for (i, a) in self.args.iter().enumerate() {
            out.push_str(&format!(" [{i}] {a}"));
        }
        for (k, v) in &self.env {
            match value_for(k, v) {
                Some(value) => out.push_str(&format!(" {k}={value}")),
                None => out.push_str(&format!(" {k}")),
            }
        }
        if self.priority != ProcessPriority::Normal {
            out.push_str(&format!(" [priority={:?}]", self.priority));
        }
        out
    }
}

impl std::fmt::Debug for LaunchCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.describe())
    }
}

/// `ROBUST_MODULE_<NAME>`: upper case, dots become underscores.
///
/// Underscores in the name are doubled, so the mapping stays one-to-one: `A.B` and `A_B` used to
/// produce the same variable and silently overwrite each other. Names without underscores keep
/// the name the engine expects.
fn module_env_name(module: &str) -> String {
    let mut out = String::with_capacity("ROBUST_MODULE_".len() + module.len() * 2);
    out.push_str("ROBUST_MODULE_");
    for c in module.chars() {
        match c {
            '_' => out.push_str("__"),
            '.' => out.push('_'),
            c => out.push(c.to_ascii_uppercase()),
        }
    }
    out
}

/// Splits a command-line string into arguments, respecting single and double quotes, `\"`, `\\`
/// and empty arguments (`""`). An unclosed quote makes the whole string unusable; see
/// [`try_split_args`] when that matters. This lenient form keeps what it could read (the app calls
/// it with text typed by the player).
pub fn split_args(input: &str) -> Vec<String> {
    split_args_inner(input).0
}

/// Like [`split_args`], but an unclosed quote is an error instead of a silent merge with the rest
/// of the line.
pub fn try_split_args(input: &str) -> Result<Vec<String>> {
    let (args, unclosed) = split_args_inner(input);
    if unclosed {
        return Err(Error::Other("an argument has an unclosed quote".into()));
    }
    Ok(args)
}

/// Returns the arguments and whether a quote was left open.
fn split_args_inner(input: &str) -> (Vec<String>, bool) {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match quote {
            Some(q) => {
                if ch == '\\' && matches!(chars.peek(), Some('"' | '\\' | '\'')) {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                } else if ch == q {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None => match ch {
                '"' | '\'' => {
                    quote = Some(ch);
                    // An empty pair of quotes is still an argument.
                    started = true;
                }
                '\\' if matches!(chars.peek(), Some('"' | '\\' | '\'')) => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                    started = true;
                }
                c if c.is_whitespace() => {
                    if started {
                        args.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                c => {
                    current.push(c);
                    started = true;
                }
            },
        }
    }
    if started {
        args.push(current);
    }
    (args, quote.is_some())
}

/// CVar names the engine accepts: dotted identifiers, e.g. `display.vsync` or `net.predict`.
fn is_valid_cvar_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 50
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// A `name=value` assignment with a valid name and a printable value.
fn is_cvar_assignment(token: &str) -> bool {
    let Some((name, value)) = token.split_once('=') else {
        return false;
    };
    is_valid_cvar_name(name) && value.len() <= 500 && !value.chars().any(char::is_control)
}

/// Keeps only `--cvar name=value` from user-supplied arguments. `--connect-address`,
/// `--username`, `--ss14-address` and anything else are dropped: a settings file or an imported
/// profile must not be able to point the game at another server or rename the player.
fn safe_extra_args(extra: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending_cvar = false;
    for raw in extra {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        if pending_cvar {
            pending_cvar = false;
            if is_cvar_assignment(token) {
                out.push("--cvar".into());
                out.push(token.to_string());
            }
            continue;
        }
        if token == "--cvar" {
            pending_cvar = true;
        } else if let Some(rest) = token.strip_prefix("--cvar ") {
            let rest = rest.trim();
            if is_cvar_assignment(rest) {
                out.push("--cvar".into());
                out.push(rest.to_string());
            }
        }
    }
    out
}

pub fn build_command(spec: &LaunchSpec) -> LaunchCommand {
    // Loader arguments: engine zip, its signature, the public key file; then what the engine gets.
    let mut args: Vec<String> = vec![
        spec.engine.zip_path.display().to_string(),
        spec.engine.sig_hex.clone(),
        spec.signing_key_file.display().to_string(),
        "--username".into(),
        spec.username.clone(),
        "--cvar".into(),
        format!(
            "display.compat={}",
            if spec.compat_mode { "True" } else { "False" }
        ),
        "--cvar".into(),
        "launch.launcher=true".into(),
    ];

    if let Some(c) = &spec.connect {
        // `--launcher` implies `--connect`: no main menu, straight to the server.
        args.extend([
            "--launcher".into(),
            "--connect-address".into(),
            c.connect_url.clone(),
        ]);
        args.extend(["--ss14-address".into(), c.server_address.clone()]);
    }

    // Build info for the client (used when connecting on modern CDNs). Only what the server gave.
    let b = &spec.build;
    for (name, value) in [
        ("download_url", b.download_url()),
        ("manifest_url", b.manifest_url()),
        ("manifest_download_url", b.manifest_download_url()),
        ("version", non_empty(&b.version)),
        ("fork_id", non_empty(&b.fork_id)),
        ("hash", b.hash()),
        ("manifest_hash", b.manifest_hash()),
        ("engine_version", non_empty(&b.engine_version)),
    ] {
        if let Some(v) = value {
            args.push("--cvar".into());
            args.push(format!("build.{name}={v}"));
        }
    }

    // Client optimizations: pass CVars according to profile and settings.
    let opt = &spec.optimization;
    let is_iskander = opt.profile == "iskander";
    if opt.fast_lighting {
        let shadow_lights = if is_iskander { 8 } else { 16 };
        args.extend([
            "--cvar".into(),
            "light.soft_shadows=false".into(),
            "--cvar".into(),
            "light.blur=false".into(),
            "--cvar".into(),
            format!("light.max_shadowcasting_lights={shadow_lights}"),
        ]);
    }
    if opt.unlock_fps {
        args.extend([
            "--cvar".into(),
            "display.vsync=false".into(),
            "--cvar".into(),
            "display.max_fps=0".into(),
        ]);
    }
    if let Some(res) = opt.light_resolution {
        args.extend(["--cvar".into(), format!("light.resolution_scale={res}")]);
    }
    if opt.profile != "off" && opt.profile != "none" {
        args.extend([
            "--cvar".into(),
            "res.prototype_reload_watch=false".into(),
            "--cvar".into(),
            "res.texturepreloadingenabled=true".into(),
            "--cvar".into(),
            "res.texture_preload_cache=true".into(),
            "--cvar".into(),
            "display.gpu_preference=2".into(),
        ]);
        if opt.profile == "potato" {
            args.extend([
                "--cvar".into(),
                "res.check_bad_file_extensions=false".into(),
            ]);
        }
        if is_iskander {
            args.extend([
                "--cvar".into(),
                "light.max_light_count=256".into(),
                "--cvar".into(),
                "light.max_occluder_count=256".into(),
                "--cvar".into(),
                "display.post_shader_render_target_pool_max_size=1048576".into(),
                "--cvar".into(),
                "res.rsi_atlas_size=4096".into(),
                "--cvar".into(),
                "res.check_bad_file_extensions=false".into(),
                "--cvar".into(),
                "res.checkpathcasing=false".into(),
                "--cvar".into(),
                "display.force_sync_windows=false".into(),
                "--cvar".into(),
                "midi.parallelism=1".into(),
            ]);
        }
        if opt.profile == "max_fps" {
            args.extend([
                "--cvar".into(),
                "display.input_buffer_size=16".into(),
                "--cvar".into(),
                "display.force_sync_windows=false".into(),
            ]);
        }
    }

    for extra in safe_extra_args(&spec.extra_args) {
        args.push(extra);
    }

    let mut env: Vec<(String, String)> = Vec::new();
    if let Some(a) = &spec.auth {
        env.push(("ROBUST_AUTH_TOKEN".into(), a.token.clone()));
        env.push(("ROBUST_AUTH_USERID".into(), a.user_id.clone()));
        env.push(("ROBUST_AUTH_PUBKEY".into(), a.server_public_key.clone()));
        env.push(("ROBUST_AUTH_SERVER".into(), a.auth_server_url.clone()));
        env.push((
            "ROBUST_AUTH_SERVERS".into(),
            format!("unset@{}", a.auth_server_url),
        ));
    }
    if let Some(zip) = &spec.overlay_zip {
        env.push(("SS14_LOADER_OVERLAY_ZIP".into(), zip.display().to_string()));
    }
    if !spec.patches.is_empty() {
        let joined = spec
            .patches
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(";");
        env.push(("SS14_LOADER_PATCHES".into(), joined));
    }
    if let Some(path) = &spec.launcher_path {
        env.push(("SS14_LAUNCHER_PATH".into(), path.display().to_string()));
    }
    env.push((
        "SS14_LOADER_CONTENT_DB".into(),
        spec.content_db.display().to_string(),
    ));
    env.push((
        "SS14_LOADER_CONTENT_VERSION".into(),
        spec.content_version_id.to_string(),
    ));
    for (name, dir) in &spec.modules {
        env.push((module_env_name(name), dir.display().to_string()));
    }
    // Performance settings the official launcher uses. No DOTNET_ROLL_FORWARD: the Loader
    // always runs on the runtime it was started with.
    env.push(("DOTNET_TieredPGO".into(), "1".into()));
    env.push(("DOTNET_ReadyToRun".into(), "0".into()));
    env.push(("DOTNET_MULTILEVEL_LOOKUP".into(), "0".into()));
    if opt.server_gc {
        env.push(("DOTNET_gcServer".into(), "1".into()));
        env.push(("DOTNET_GCDynamicAdaptationMode".into(), "1".into()));
        env.push(("DOTNET_gcConcurrent".into(), "1".into()));
        env.push(("DOTNET_TC_QuickJitForLoops".into(), "1".into()));
        env.push(("BEBRA_OPTIMIZE".into(), "1".into()));
    }
    if let Some(root) = &spec.dotnet_root {
        env.push(("DOTNET_ROOT".into(), root.display().to_string()));
    }

    let priority = if opt.high_priority {
        if opt.profile == "max_fps" || opt.profile == "potato" || opt.profile == "iskander" {
            ProcessPriority::High
        } else {
            ProcessPriority::AboveNormal
        }
    } else {
        ProcessPriority::Normal
    };

    let env_remove = CLEARED_ENV
        .iter()
        .filter(|name| !env.iter().any(|(k, _)| k == *name))
        .map(|s| s.to_string())
        .collect();

    LaunchCommand {
        program: spec.loader_exe.clone(),
        args,
        env,
        env_remove,
        priority,
    }
}

fn non_empty(s: &str) -> Option<&str> {
    (!s.is_empty()).then_some(s)
}

/// Writes the public key file the Loader wants as an argument (it reads a path, not a string).
pub fn ensure_signing_key_file(dirs: &DataDirs) -> Result<PathBuf> {
    ensure_signing_key_file_for_engine(dirs, "Robust")
}

pub fn ensure_signing_key_file_for_engine(dirs: &DataDirs, engine: &str) -> Result<PathBuf> {
    let filename = if engine.eq_ignore_ascii_case("multiverse") {
        "signing_key_multiverse"
    } else if engine.eq_ignore_ascii_case("supermatter") {
        "signing_key_supermatter"
    } else {
        "signing_key"
    };
    let path = dirs.root().join(filename);
    let pem = crate::signature::key_pem_for_engine(engine);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(pem) {
        // Write to a temporary file and rename it into place: two launchers starting at once must
        // never let the Loader read a half-written key.
        std::fs::create_dir_all(dirs.root())?;
        crate::fsutil::write_atomic(&path, pem.as_bytes())?;
    }
    Ok(path)
}

/// Checks a loader path taken from the environment: it must be an existing regular file with the
/// expected name. `BEBRA_LOADER` is an explicit developer override, but accepting an arbitrary
/// executable there (a renamed `cmd.exe`, say) would turn a stray environment variable into code
/// execution, so at least the name has to match.
fn validate_loader_path(path: &Path) -> Result<PathBuf> {
    let path = std::fs::canonicalize(path).map_err(|e| {
        Error::Other(format!(
            "BEBRA_LOADER points to {}, which cannot be used: {e}",
            path.display()
        ))
    })?;
    if !path.is_file() {
        return Err(Error::Other(format!(
            "BEBRA_LOADER points to {}, which is not a file",
            path.display()
        )));
    }
    let ok_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("SS14.Loader.exe"));
    if !ok_name {
        return Err(Error::Other(format!(
            "BEBRA_LOADER must point to SS14.Loader.exe, not {}",
            path.display()
        )));
    }
    Ok(path)
}

/// Looks for `SS14.Loader.exe`: `BEBRA_LOADER`, next to our exe, then (when running from a build
/// folder) the `loader/out` of this repository.
pub fn find_loader() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("BEBRA_LOADER") {
        return validate_loader_path(&PathBuf::from(p));
    }

    let exe = std::env::current_exe()?;
    let dir = exe.parent().unwrap_or(Path::new("."));
    let beside = dir.join("loader").join("SS14.Loader.exe");
    if beside.is_file() {
        return Ok(beside);
    }
    for ancestor in dir.ancestors().take(6) {
        let dev = ancestor.join("loader").join("out").join("SS14.Loader.exe");
        if dev.is_file() {
            return Ok(dev);
        }
    }
    Err(Error::Other(
        "SS14.Loader.exe not found: build it (see loader/README.md) or set BEBRA_LOADER".into(),
    ))
}

/// A client log larger than this is rotated once (`.1`) before a new run appends to it.
pub const MAX_CLIENT_LOG_BYTES: u64 = 8 * 1024 * 1024;

/// Opens a log for appending. Never truncates: two clients launched at once must not erase each
/// other's output. A log that has grown past [`MAX_CLIENT_LOG_BYTES`] is moved aside first.
fn open_log(path: &Path) -> Result<std::fs::File> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() >= MAX_CLIENT_LOG_BYTES) {
        let rotated = PathBuf::from(format!("{}.1", path.display()));
        let _ = std::fs::remove_file(&rotated);
        let _ = std::fs::rename(path, rotated);
    }
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?)
}

/// Starts the command. The client's stdout and stderr are written to the given files.
pub fn spawn(cmd: &LaunchCommand, stdout_log: &Path, stderr_log: &Path) -> Result<Child> {
    if let Some(dir) = stdout_log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let out = open_log(stdout_log)?;
    let err = open_log(stderr_log)?;

    let mut c = Command::new(&cmd.program);
    c.args(&cmd.args)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let flags = match cmd.priority {
            ProcessPriority::Normal => 0,
            ProcessPriority::AboveNormal => 0x00008000,
            ProcessPriority::High => 0x00000080,
        };
        if flags != 0 {
            c.creation_flags(flags);
        }
    }
    for name in &cmd.env_remove {
        c.env_remove(name);
    }
    for (k, v) in &cmd.env {
        c.env(k, v);
    }
    c.spawn()
        .map_err(|e| Error::Other(format!("failed to start {}: {e}", cmd.program.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> InstalledEngine {
        InstalledEngine {
            version: "291.0.0".into(),
            zip_path: PathBuf::from(r"C:\data\engines\291.0.0.zip"),
            sig_hex: "AABB".into(),
            downloaded: false,
        }
    }

    fn build() -> BuildInfo {
        BuildInfo {
            manifest_url: Some("https://x/manifest".into()),
            manifest_download_url: Some("https://x/download".into()),
            download_url: Some("https://x/c.zip".into()),
            manifest_hash: Some("EB".into()),
            hash: Some("8F".into()),
            engine_version: "291.0.0".into(),
            version: "v1".into(),
            fork_id: "fork".into(),
            acz: false,
            engine_type: None,
        }
    }

    fn spec() -> LaunchSpec {
        LaunchSpec {
            loader_exe: PathBuf::from(r"C:\app\loader\SS14.Loader.exe"),
            dotnet_root: None,
            engine: engine(),
            signing_key_file: PathBuf::from(r"C:\data\signing_key"),
            content_db: PathBuf::from(r"C:\data\content.db"),
            content_version_id: 7,
            modules: vec![],
            username: "Player".into(),
            auth: None,
            build: build(),
            connect: Some(ConnectTarget {
                connect_url: "udp://host:1212/".into(),
                server_address: "ss14://host:1212/".into(),
            }),
            compat_mode: false,
            launcher_path: None,
            overlay_zip: None,
            extra_args: vec![],
            patches: vec![],
            optimization: OptimizationConfig {
                profile: "off".into(),
                server_gc: false,
                fast_lighting: false,
                unlock_fps: false,
                high_priority: false,
                light_resolution: None,
            },
        }
    }

    fn env_of(cmd: &LaunchCommand, name: &str) -> Option<String> {
        cmd.env
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }

    #[test]
    fn guest_connect_arguments_match_the_official_launcher() {
        let cmd = build_command(&spec());
        let expected: Vec<String> = [
            r"C:\data\engines\291.0.0.zip",
            "AABB",
            r"C:\data\signing_key",
            "--username",
            "Player",
            "--cvar",
            "display.compat=False",
            "--cvar",
            "launch.launcher=true",
            "--launcher",
            "--connect-address",
            "udp://host:1212/",
            "--ss14-address",
            "ss14://host:1212/",
            "--cvar",
            "build.download_url=https://x/c.zip",
            "--cvar",
            "build.manifest_url=https://x/manifest",
            "--cvar",
            "build.manifest_download_url=https://x/download",
            "--cvar",
            "build.version=v1",
            "--cvar",
            "build.fork_id=fork",
            "--cvar",
            "build.hash=8F",
            "--cvar",
            "build.manifest_hash=EB",
            "--cvar",
            "build.engine_version=291.0.0",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(cmd.args, expected);
        assert_eq!(cmd.program, PathBuf::from(r"C:\app\loader\SS14.Loader.exe"));
    }

    #[test]
    fn without_a_target_the_client_stays_in_its_menu() {
        let mut s = spec();
        s.connect = None;
        let args = build_command(&s).args;
        assert!(!args
            .iter()
            .any(|a| a == "--launcher" || a == "--connect-address" || a == "--ss14-address"));
        assert!(args.contains(&"launch.launcher=true".to_string()));
    }

    #[test]
    fn launcher_path_is_passed_for_redial_only_when_given() {
        let cmd = build_command(&spec());
        assert_eq!(env_of(&cmd, "SS14_LAUNCHER_PATH"), None);
        assert!(cmd.env_remove.contains(&"SS14_LAUNCHER_PATH".to_string()));

        let mut s = spec();
        s.launcher_path = Some(PathBuf::from(r"C:\app\bebra-app.exe"));
        let cmd = build_command(&s);
        assert_eq!(
            env_of(&cmd, "SS14_LAUNCHER_PATH").as_deref(),
            Some(r"C:\app\bebra-app.exe")
        );
        assert!(!cmd.env_remove.contains(&"SS14_LAUNCHER_PATH".to_string()));
    }

    #[test]
    fn the_overlay_is_passed_only_when_given() {
        let cmd = build_command(&spec());
        assert_eq!(env_of(&cmd, "SS14_LOADER_OVERLAY_ZIP"), None);
        assert!(cmd
            .env_remove
            .contains(&"SS14_LOADER_OVERLAY_ZIP".to_string()));

        let mut s = spec();
        s.overlay_zip = Some(PathBuf::from(r"C:\data\overlays\overlay-1.zip"));
        let cmd = build_command(&s);
        assert_eq!(
            env_of(&cmd, "SS14_LOADER_OVERLAY_ZIP").as_deref(),
            Some(r"C:\data\overlays\overlay-1.zip")
        );
        assert!(!cmd
            .env_remove
            .contains(&"SS14_LOADER_OVERLAY_ZIP".to_string()));
    }

    #[test]
    fn compat_mode_flag() {
        let mut s = spec();
        s.compat_mode = true;
        assert!(build_command(&s)
            .args
            .contains(&"display.compat=True".to_string()));
    }

    #[test]
    fn empty_build_fields_are_left_out() {
        let mut s = spec();
        s.build = BuildInfo {
            manifest_url: Some(String::new()),
            engine_version: "291.0.0".into(),
            ..Default::default()
        };
        let args = build_command(&s).args.join(" ");
        assert!(args.contains("build.engine_version=291.0.0"));
        for missing in [
            "manifest_url",
            "download_url",
            "build.version",
            "fork_id",
            "build.hash",
        ] {
            assert!(!args.contains(missing), "{missing} should be omitted");
        }
    }

    #[test]
    fn environment_for_a_guest() {
        let cmd = build_command(&spec());
        assert_eq!(
            env_of(&cmd, "SS14_LOADER_CONTENT_DB").as_deref(),
            Some(r"C:\data\content.db")
        );
        assert_eq!(
            env_of(&cmd, "SS14_LOADER_CONTENT_VERSION").as_deref(),
            Some("7")
        );
        assert_eq!(env_of(&cmd, "DOTNET_TieredPGO").as_deref(), Some("1"));
        assert_eq!(env_of(&cmd, "DOTNET_ReadyToRun").as_deref(), Some("0"));
        assert_eq!(
            env_of(&cmd, "DOTNET_MULTILEVEL_LOOKUP").as_deref(),
            Some("0")
        );
        assert_eq!(env_of(&cmd, "ROBUST_AUTH_TOKEN"), None);
        assert_eq!(env_of(&cmd, "DOTNET_ROOT"), None);
        assert_eq!(
            env_of(&cmd, "DOTNET_ROLL_FORWARD"),
            None,
            "must never be set"
        );

        // Stale credentials from our own environment must not reach the client.
        for name in [
            "ROBUST_AUTH_TOKEN",
            "ROBUST_AUTH_USERID",
            "ROBUST_AUTH_SERVERS",
            "SS14_LOADER_PATCHES",
            "SS14_DISABLE_SIGNING",
            "DOTNET_ROLL_FORWARD",
        ] {
            assert!(
                cmd.env_remove.iter().any(|n| n == name),
                "{name} should be cleared"
            );
        }
    }

    #[test]
    fn environment_with_an_account_and_modules() {
        let mut s = spec();
        s.auth = Some(AuthEnv {
            token: "SECRET-TOKEN".into(),
            user_id: "1234".into(),
            server_public_key: "PUBKEY".into(),
            auth_server_url: "https://auth.example/".into(),
        });
        s.modules = vec![(
            "Robust.Client.WebView".into(),
            PathBuf::from(r"C:\data\modules\wv"),
        )];
        s.dotnet_root = Some(PathBuf::from(r"C:\dotnet"));
        let cmd = build_command(&s);

        assert_eq!(
            env_of(&cmd, "ROBUST_AUTH_TOKEN").as_deref(),
            Some("SECRET-TOKEN")
        );
        assert_eq!(env_of(&cmd, "ROBUST_AUTH_USERID").as_deref(), Some("1234"));
        assert_eq!(
            env_of(&cmd, "ROBUST_AUTH_PUBKEY").as_deref(),
            Some("PUBKEY")
        );
        assert_eq!(
            env_of(&cmd, "ROBUST_AUTH_SERVER").as_deref(),
            Some("https://auth.example/")
        );
        assert_eq!(
            env_of(&cmd, "ROBUST_AUTH_SERVERS").as_deref(),
            Some("unset@https://auth.example/")
        );
        assert!(
            !cmd.env_remove.iter().any(|n| n == "ROBUST_AUTH_SERVERS"),
            "a variable we set is not also removed"
        );
        assert_eq!(
            env_of(&cmd, "ROBUST_MODULE_ROBUST_CLIENT_WEBVIEW").as_deref(),
            Some(r"C:\data\modules\wv")
        );
        assert_eq!(env_of(&cmd, "DOTNET_ROOT").as_deref(), Some(r"C:\dotnet"));
        assert!(
            !cmd.env_remove.iter().any(|n| n == "ROBUST_AUTH_TOKEN"),
            "a variable we set is not also removed"
        );
    }

    #[test]
    fn the_token_never_appears_in_text_output() {
        let mut s = spec();
        s.auth = Some(AuthEnv {
            token: "SECRET-TOKEN".into(),
            user_id: "1234".into(),
            server_public_key: "PUBKEY".into(),
            auth_server_url: "https://auth.example/".into(),
        });
        let cmd = build_command(&s);
        for text in [cmd.describe(), cmd.describe_verbose(), format!("{cmd:?}")] {
            assert!(!text.contains("SECRET-TOKEN"), "leaked: {text}");
            assert!(!text.contains("=PUBKEY"), "leaked: {text}");
            assert!(!text.contains("https://auth.example/"), "leaked: {text}");
        }
        assert!(!format!("{:?}", s.auth).contains("SECRET-TOKEN"));
        // By default only the names are logged; no `=value` pairs at all.
        let plain = cmd.describe();
        assert!(plain.contains("ROBUST_AUTH_TOKEN"));
        assert!(!plain.contains("ROBUST_AUTH_TOKEN="), "{plain}");
        assert!(!plain.contains("SS14_LOADER_CONTENT_DB="), "{plain}");
        assert!(
            cmd.describe_verbose()
                .contains("ROBUST_AUTH_TOKEN=<hidden>"),
            "{}",
            cmd.describe_verbose()
        );
    }

    #[test]
    fn module_env_names() {
        assert_eq!(
            module_env_name("Robust.Client.WebView"),
            "ROBUST_MODULE_ROBUST_CLIENT_WEBVIEW"
        );
        // Dots and underscores must not collapse into the same variable.
        assert_eq!(module_env_name("A.B"), "ROBUST_MODULE_A_B");
        assert_eq!(module_env_name("A_B"), "ROBUST_MODULE_A__B");
        assert_ne!(module_env_name("A.B"), module_env_name("A_B"));
    }

    #[test]
    fn module_names_with_dots_and_underscores_get_distinct_variables() {
        let mut s = spec();
        s.modules = vec![
            ("A.B".into(), PathBuf::from(r"C:\m\dot")),
            ("A_B".into(), PathBuf::from(r"C:\m\underscore")),
        ];
        let cmd = build_command(&s);
        let names: Vec<&str> = cmd.env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names
                .iter()
                .filter(|n| n.starts_with("ROBUST_MODULE_"))
                .count(),
            2
        );
        assert!(names.contains(&"ROBUST_MODULE_A_B"));
        assert!(names.contains(&"ROBUST_MODULE_A__B"));
    }

    #[test]
    fn an_explicit_loader_path_must_be_the_loader() {
        let tmp = tempfile::tempdir().unwrap();
        let good = tmp.path().join("SS14.Loader.exe");
        std::fs::write(&good, b"not a real loader").unwrap();
        assert_eq!(
            validate_loader_path(&good).unwrap(),
            std::fs::canonicalize(&good).unwrap()
        );

        let wrong = tmp.path().join("notepad.exe");
        std::fs::write(&wrong, b"x").unwrap();
        assert!(validate_loader_path(&wrong).is_err());

        assert!(validate_loader_path(&tmp.path().join("missing.exe")).is_err());
        assert!(
            validate_loader_path(tmp.path()).is_err(),
            "a folder is not a file"
        );
    }

    #[test]
    fn signing_key_file_is_written_once() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::at(tmp.path().join("d"));
        let p = ensure_signing_key_file(&dirs).unwrap();
        let first = std::fs::read_to_string(&p).unwrap();
        assert!(first.contains("BEGIN PUBLIC KEY"));
        assert_eq!(ensure_signing_key_file(&dirs).unwrap(), p);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), first);
        // A damaged file is repaired.
        std::fs::write(&p, "junk").unwrap();
        ensure_signing_key_file(&dirs).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), first);
    }

    #[test]
    fn spawn_captures_output_and_clears_env() {
        let tmp = tempfile::tempdir().unwrap();
        let (out, err) = (tmp.path().join("out.log"), tmp.path().join("err.log"));
        let cmd = LaunchCommand {
            program: PathBuf::from("cmd.exe"),
            args: vec![
                "/c".into(),
                "echo hello %MY_TEST_VAR% & echo oops 1>&2 & echo [%ROBUST_AUTH_TOKEN%]".into(),
            ],
            env: vec![("MY_TEST_VAR".into(), "world".into())],
            env_remove: vec!["ROBUST_AUTH_TOKEN".into()],
            priority: ProcessPriority::Normal,
        };
        std::env::set_var("ROBUST_AUTH_TOKEN", "should-not-reach-the-child");
        let status = spawn(&cmd, &out, &err).unwrap().wait().unwrap();
        std::env::remove_var("ROBUST_AUTH_TOKEN");

        assert!(status.success());
        let stdout = std::fs::read_to_string(&out).unwrap();
        assert!(stdout.contains("hello world"), "{stdout}");
        assert!(!stdout.contains("should-not-reach"), "{stdout}");
        assert!(std::fs::read_to_string(&err).unwrap().contains("oops"));
    }

    #[test]
    fn spawn_reports_a_missing_program() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = LaunchCommand {
            program: tmp.path().join("nope.exe"),
            args: vec![],
            env: vec![],
            env_remove: vec![],
            priority: ProcessPriority::Normal,
        };
        let err = spawn(&cmd, &tmp.path().join("o"), &tmp.path().join("e")).unwrap_err();
        assert!(err.to_string().contains("failed to start"));
    }

    #[test]
    fn split_args_handles_whitespace_and_quotes() {
        assert_eq!(
            split_args("--cvar display.vsync=false --cvar gfx.threads=4"),
            vec!["--cvar", "display.vsync=false", "--cvar", "gfx.threads=4"]
        );
        assert_eq!(
            split_args(r#"--cvar "net.predict=false" --cvar 'audio.vol=1.0' plain"#),
            vec![
                "--cvar",
                "net.predict=false",
                "--cvar",
                "audio.vol=1.0",
                "plain"
            ]
        );
        assert_eq!(split_args("   "), Vec::<String>::new());
    }

    #[test]
    fn split_args_keeps_empty_arguments() {
        assert_eq!(split_args(r#"a "" b '' c"#), vec!["a", "", "b", "", "c"]);
        assert_eq!(split_args(r#""""#), vec![""]);
    }

    #[test]
    fn split_args_understands_escapes() {
        // `\"` inside double quotes, `\\` anywhere, `\'` inside single quotes.
        assert_eq!(
            split_args(r#"--cvar "display.name=\"quoted name\"" plain"#),
            vec!["--cvar", r#"display.name="quoted name""#, "plain"]
        );
        assert_eq!(split_args(r"a\\b"), vec![r"a\b"]);
        assert_eq!(split_args(r#""a\\b""#), vec![r"a\b"]);
        // A backslash before an ordinary character is a normal backslash (Windows paths).
        assert_eq!(
            split_args(r"C:\data\loader.exe"),
            vec![r"C:\data\loader.exe"]
        );
        assert_eq!(split_args(r#""C:\data\x""#), vec![r"C:\data\x"]);
    }

    #[test]
    fn an_unclosed_quote_is_an_error_only_when_asked() {
        assert!(try_split_args(r#"--cvar "unclosed"#).is_err());
        assert!(
            try_split_args("it's fine").is_err(),
            "an odd ' opens a quote"
        );
        assert_eq!(
            try_split_args(r#"--cvar "ok" plain"#).unwrap(),
            vec!["--cvar", "ok", "plain"]
        );
        // The lenient form still returns what it read instead of panicking.
        assert_eq!(
            split_args(r#"--cvar "unclosed"#),
            vec!["--cvar", "unclosed"]
        );
    }

    #[test]
    fn client_logs_are_appended_not_truncated() {
        use std::io::Write;

        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("client.stdout.log");
        open_log(&log).unwrap().write_all(b"first run\n").unwrap();
        open_log(&log).unwrap().write_all(b"second run\n").unwrap();
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("first run"), "{text}");
        assert!(text.contains("second run"), "{text}");
    }

    #[test]
    fn a_huge_client_log_is_rotated_before_use() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("client.stdout.log");
        std::fs::write(&log, vec![b'x'; MAX_CLIENT_LOG_BYTES as usize]).unwrap();
        drop(open_log(&log).unwrap());
        let rotated = tmp.path().join("client.stdout.log.1");
        assert_eq!(
            std::fs::metadata(&rotated).unwrap().len(),
            MAX_CLIENT_LOG_BYTES
        );
        assert_eq!(std::fs::metadata(&log).unwrap().len(), 0);
    }

    #[test]
    fn extra_args_appended_to_command() {
        let mut s = spec();
        s.extra_args = vec!["--cvar".into(), "display.vsync=false".into()];
        let cmd = build_command(&s);
        assert!(cmd.args.contains(&"--cvar".to_string()));
        assert!(cmd.args.contains(&"display.vsync=false".to_string()));
    }

    #[test]
    fn extra_args_cannot_override_the_target_or_the_username() {
        let mut s = spec();
        s.extra_args = vec![
            "--connect-address".into(),
            "udp://evil.example:1212/".into(),
            "--username".into(),
            "Other".into(),
            "--ss14-address".into(),
            "ss14://evil.example/".into(),
            "--cvar".into(),
            "display.vsync=false".into(),
            "--cvar".into(),
            "bad name=1".into(),
            "--cvar display.max_fps=60".into(),
        ];
        let cmd = build_command(&s);

        assert!(cmd.args.contains(&"display.vsync=false".to_string()));
        assert!(cmd.args.contains(&"display.max_fps=60".to_string()));
        assert!(!cmd.args.iter().any(|a| a.contains("evil.example")));
        assert!(!cmd.args.iter().any(|a| a == "Other"));
        assert_eq!(
            cmd.args
                .iter()
                .filter(|a| *a == "--connect-address")
                .count(),
            1,
            "only our own address may be given"
        );
        assert_eq!(
            cmd.args.iter().filter(|a| *a == "--username").count(),
            1,
            "only our own username may be given"
        );
        assert_eq!(
            cmd.args.iter().filter(|a| *a == "--ss14-address").count(),
            1
        );
    }

    #[test]
    fn patches_set_loader_env() {
        let mut s = spec();
        s.patches = vec![
            PathBuf::from(r"C:\patches\A.dll"),
            PathBuf::from(r"C:\patches\B.dll"),
        ];
        let cmd = build_command(&s);
        let val = env_of(&cmd, "SS14_LOADER_PATCHES").unwrap();
        assert!(val.contains(r"C:\patches\A.dll;C:\patches\B.dll"));
    }

    #[test]
    fn optimization_balanced_profile_adds_cvars_and_server_gc() {
        let mut s = spec();
        s.optimization = OptimizationConfig {
            profile: "balanced".into(),
            server_gc: true,
            fast_lighting: false,
            unlock_fps: false,
            high_priority: true,
            light_resolution: None,
        };
        let cmd = build_command(&s);
        assert_eq!(cmd.priority, ProcessPriority::AboveNormal);
        assert_eq!(env_of(&cmd, "DOTNET_gcServer").as_deref(), Some("1"));
        assert_eq!(
            env_of(&cmd, "DOTNET_GCDynamicAdaptationMode").as_deref(),
            Some("1")
        );
        assert_eq!(env_of(&cmd, "BEBRA_OPTIMIZE").as_deref(), Some("1"));
        assert!(cmd
            .args
            .contains(&"res.prototype_reload_watch=false".to_string()));
        assert!(cmd
            .args
            .contains(&"res.texturepreloadingenabled=true".to_string()));
        assert!(cmd
            .args
            .contains(&"res.texture_preload_cache=true".to_string()));
    }

    #[test]
    fn optimization_potato_profile_adds_fast_lighting_and_low_res() {
        let mut s = spec();
        s.optimization = OptimizationConfig {
            profile: "potato".into(),
            server_gc: true,
            fast_lighting: true,
            unlock_fps: true,
            high_priority: true,
            light_resolution: Some(0.35),
        };
        let cmd = build_command(&s);
        assert_eq!(cmd.priority, ProcessPriority::High);
        assert!(cmd.args.contains(&"light.soft_shadows=false".to_string()));
        assert!(cmd.args.contains(&"light.blur=false".to_string()));
        assert!(cmd
            .args
            .contains(&"light.resolution_scale=0.35".to_string()));
        assert!(cmd.args.contains(&"display.vsync=false".to_string()));
        assert!(cmd.args.contains(&"display.max_fps=0".to_string()));
        assert!(cmd
            .args
            .contains(&"res.check_bad_file_extensions=false".to_string()));
    }

    #[test]
    fn optimization_max_fps_profile_unlocks_fps_and_high_priority() {
        let mut s = spec();
        s.optimization = OptimizationConfig {
            profile: "max_fps".into(),
            server_gc: true,
            fast_lighting: false,
            unlock_fps: true,
            high_priority: true,
            light_resolution: None,
        };
        let cmd = build_command(&s);
        assert_eq!(cmd.priority, ProcessPriority::High);
        assert!(cmd.args.contains(&"display.vsync=false".to_string()));
        assert!(cmd.args.contains(&"display.max_fps=0".to_string()));
        assert!(cmd
            .args
            .contains(&"display.input_buffer_size=16".to_string()));
    }

    #[test]
    fn optimization_iskander_profile_ultra_optimizations() {
        let mut s = spec();
        s.optimization = OptimizationConfig {
            profile: "iskander".into(),
            server_gc: true,
            fast_lighting: true,
            unlock_fps: true,
            high_priority: true,
            light_resolution: Some(0.2),
        };
        let cmd = build_command(&s);
        assert_eq!(cmd.priority, ProcessPriority::High);
        assert!(cmd.args.contains(&"light.soft_shadows=false".to_string()));
        assert!(cmd.args.contains(&"light.blur=false".to_string()));
        assert!(cmd.args.contains(&"light.resolution_scale=0.2".to_string()));
        assert!(cmd
            .args
            .contains(&"light.max_shadowcasting_lights=8".to_string()));
        assert!(cmd.args.contains(&"light.max_light_count=256".to_string()));
        assert!(cmd
            .args
            .contains(&"light.max_occluder_count=256".to_string()));
        assert!(cmd.args.contains(&"res.rsi_atlas_size=4096".to_string()));
        assert!(cmd.args.contains(&"midi.parallelism=1".to_string()));
        assert!(cmd
            .args
            .contains(&"display.post_shader_render_target_pool_max_size=1048576".to_string()));
        assert_eq!(env_of(&cmd, "DOTNET_gcServer").as_deref(), Some("1"));
        assert_eq!(env_of(&cmd, "BEBRA_OPTIMIZE").as_deref(), Some("1"));
    }
}
