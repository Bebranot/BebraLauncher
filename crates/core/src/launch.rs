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
    "SS14_LOADER_CONTENT_DB",
    "SS14_LOADER_CONTENT_VERSION",
    "SS14_LOADER_OVERLAY_ZIP",
    "SS14_DISABLE_SIGNING",
    "SS14_LAUNCHER_PATH",
    "SS14_LOG_CLIENT",
    "DOTNET_gcServer",
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
}

#[derive(Clone)]
pub struct LaunchCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
}

const SECRET_ENV: &[&str] = &["ROBUST_AUTH_TOKEN"];

impl LaunchCommand {
    /// A log-safe one-line description: program, arguments and environment names,
    /// with secret values replaced.
    pub fn describe(&self) -> String {
        let mut out = self.program.display().to_string();
        for (i, a) in self.args.iter().enumerate() {
            out.push_str(&format!(" [{i}] {a}"));
        }
        for (k, v) in &self.env {
            let v = if SECRET_ENV.contains(&k.as_str()) {
                "<hidden>"
            } else {
                v
            };
            out.push_str(&format!(" {k}={v}"));
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
fn module_env_name(module: &str) -> String {
    format!("ROBUST_MODULE_{}", module.to_uppercase().replace('.', "_"))
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

    let mut env: Vec<(String, String)> = Vec::new();
    if let Some(a) = &spec.auth {
        env.push(("ROBUST_AUTH_TOKEN".into(), a.token.clone()));
        env.push(("ROBUST_AUTH_USERID".into(), a.user_id.clone()));
        env.push(("ROBUST_AUTH_PUBKEY".into(), a.server_public_key.clone()));
        env.push(("ROBUST_AUTH_SERVER".into(), a.auth_server_url.clone()));
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
    if let Some(root) = &spec.dotnet_root {
        env.push(("DOTNET_ROOT".into(), root.display().to_string()));
    }

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
    }
}

fn non_empty(s: &str) -> Option<&str> {
    (!s.is_empty()).then_some(s)
}

/// Writes the public key file the Loader wants as an argument (it reads a path, not a string).
pub fn ensure_signing_key_file(dirs: &DataDirs) -> Result<PathBuf> {
    let path = dirs.root().join("signing_key");
    let pem = crate::signature::official_key_pem();
    if std::fs::read_to_string(&path).ok().as_deref() != Some(pem) {
        std::fs::create_dir_all(dirs.root())?;
        std::fs::write(&path, pem)?;
    }
    Ok(path)
}

/// Looks for `SS14.Loader.exe`: `BEBRA_LOADER`, next to our exe, then (when running from a build
/// folder) the `loader/out` of this repository.
pub fn find_loader() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("BEBRA_LOADER") {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(Error::Other(format!(
                "BEBRA_LOADER points to {}, which is not a file",
                p.display()
            )))
        };
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

/// Starts the command. The client's stdout and stderr are written to the given files.
pub fn spawn(cmd: &LaunchCommand, stdout_log: &Path, stderr_log: &Path) -> Result<Child> {
    if let Some(dir) = stdout_log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let out = std::fs::File::create(stdout_log)?;
    let err = std::fs::File::create(stderr_log)?;

    let mut c = Command::new(&cmd.program);
    c.args(&cmd.args)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
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
        s.launcher_path = Some(PathBuf::from(r"C:ppebra-app.exe"));
        let cmd = build_command(&s);
        assert_eq!(
            env_of(&cmd, "SS14_LAUNCHER_PATH").as_deref(),
            Some(r"C:ppebra-app.exe")
        );
        assert!(!cmd.env_remove.contains(&"SS14_LAUNCHER_PATH".to_string()));
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
        for text in [cmd.describe(), format!("{cmd:?}"), format!("{:?}", s.auth)] {
            assert!(!text.contains("SECRET-TOKEN"), "leaked: {text}");
        }
        assert!(cmd.describe().contains("ROBUST_AUTH_TOKEN=<hidden>"));
    }

    #[test]
    fn module_env_names() {
        assert_eq!(
            module_env_name("Robust.Client.WebView"),
            "ROBUST_MODULE_ROBUST_CLIENT_WEBVIEW"
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
        };
        let err = spawn(&cmd, &tmp.path().join("o"), &tmp.path().join("e")).unwrap_err();
        assert!(err.to_string().contains("failed to start"));
    }
}
