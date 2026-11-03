# Loader

`SS14.Loader` starts the game client: it checks the engine signature, loads `Robust.Client.dll`
from the engine zip and hands it the game files from `content.db`. It has to be a .NET program,
so it can't be part of the Rust core.

## What is upstream and what is ours

Most sources come from the official launcher
(https://github.com/space-wizards/SS14.Launcher, commit `437fe66`, MIT, see `LICENSE-SS14.Launcher.txt`):

- `SS14.Loader/*.cs`, `Robust.LoaderApi/` (MIT, `Robust.LoaderApi/LICENSE.txt`)
- `Shared/*.cs` are helper files from `SS14.Launcher/` (five on disk; the csproj compiles only
  `ContentCompressionScheme.cs`, `SqliteBlobStream.cs`, `ZStdException.cs`)
- project files (`*.csproj`, `Directory.*.props`) are ours: no `Launcher.props`, no trimming step.

The differences we know of (they were checked against upstream `437fe66` during the code review of
2026-10-29) are listed below; this list is the contract. If you find another difference, add it here
before a release — code that changes behaviour without a line here is a bug:

| File | Difference | Why |
|---|---|---|
| `SS14.Loader/PatchManager.cs` | Our file, absent upstream (~490 lines): reads `SS14_LOADER_PATCHES`, loads the .dll files with `AssemblyLoadContext.Default`, wires assembly resolvers, applies Harmony patches, calls patch entry points. Patch code runs outside the internal lock (work is queued, one runner at a time), every type is marked processed after a successful `Patch()` even when it patched 0 methods, and failed entry points/Harmony patches are retried at most 3 times | Lets the launcher install client mods (see the "Моды" tab). This is execution of unsigned third-party code — README/PRODUCT must not claim otherwise. Running third-party code while holding the load lock could deadlock every `AssemblyLoad` in the game |
| `SS14.Loader/Program.cs` | `PatchManager.Init(clientAssembly)` is called right after `Robust.Client.dll` is opened, before the Loader entry point runs | Patches have to be applied before the game initialises |
| `SS14.Loader/HarmonyBridge.cs` | Our file: the only type that names a HarmonyLib type. `PatchManager` keeps Harmony in an `object?` and goes through this bridge (`Create`, `ApplyPatches`, `TakesHarmony`) | `Robust.Client.GameController`'s static constructor kills the process when an assembly named `0Harmony` is already loaded. A `Harmony`-typed field in `PatchManager` made the CLR load `0Harmony.dll` on a patch-less launch, so every game start died with `AccessViolationException` before the first log line. With the types isolated, a launch without patches never loads Harmony at all |
| `SS14.Loader/PatchManager.cs` (Harmony guard) | Before `HarmonyBridge.Create` the loader runs `RuntimeHelpers.RunClassConstructor` on `Robust.Client.GameController`, so the engine's one-shot anti-Harmony constructor sees no Harmony assembly | That constructor deliberately corrupts memory when it finds `0Harmony`; running it once while Harmony is not loaded makes the later load harmless and keeps client patches usable |
| `SS14.Loader/PatchLog.cs` | Our file: all patch messages go through it and it writes nothing unless `BEBRA_PATCH_LOG=1`/`true` is set; `HarmonyBridge` also forces `Harmony.DEBUG = false` | A patched client must not advertise itself in the client's local logs. The switch is for debugging a broken patch, not for normal play |
| `SS14.Loader/SS14.Loader.csproj` (Harmony version) | `Lib.Harmony` is 2.4.3, which ships a `net10.0` build; 2.3.5 throws `PlatformNotSupportedException: CoreCLR version 10.x is not supported` from `PatchFunctions.UpdateWrapper` | The client runs on .NET 10, so older Harmony builds cannot apply a patch at all |
| `SS14.Loader/Program.cs` | `BEBRA_OPTIMIZE` block: `GCSettings.LatencyMode = SustainedLowLatency` and `ThreadPriority.AboveNormal` when the variable is `1`/`true`, failures are only logged | Opt-in runtime tuning, requested by the launcher |
| `SS14.Loader/Program.cs` | The engine signature is verified over one in-memory copy of the zip and that same buffer is parsed (the file is not reopened by path); `Program` takes a `Stream` and is `IDisposable`; content DB and overlay are closed in a `finally` even when the overlay cannot be opened; `Main` has a top-level `try/catch` returning exit code 4; the `SS14_DISABLE_SIGNING` opt-out is removed entirely | Closes the TOCTOU window between check and read, stops leaked file handles, and makes an unsigned engine impossible in any configuration (the old `#if !RELEASE` guard never compiled out because `RELEASE` was not defined) |
| `SS14.Loader/ContentDbFileApi.cs` | Deflate path rejects a blob whose real length differs from the declared size; zstd path fails on a truncated blob or when the declared size is too small (`into.Length == 0`) instead of spinning, with an iteration cap; `sqlite3_open_v2` result is checked before the handle is used and `busy_timeout(5000)` is set; `RunningClient` INSERT/DELETE check `SQLITE_DONE`; `Dispose` is idempotent, tolerates a missing master connection, frees each `ZSTD_DCtx`, and `SS14_LOADER_CONTENT_POOL_SIZE` is parsed with `TryParse` and clamped to 1..8 | A short deflate blob exposed uninitialized heap bytes to the client; a truncated zstd blob burned 100% CPU forever; a failed `RunningClient` write made the launcher's cull treat a live client as dead |
| `SS14.Loader/SS14.Loader.csproj` | `Lib.Harmony` package reference (all package versions are pinned in `Directory.Packages.props`) | Needed by `PatchManager` |
| `Bebra.HwidPatch/` | Our project, separate from the Loader: a patch assembly that postfixes `GetLegacy`/`GetModern` of `Robust.Client.HWId.BasicHWId` and `Robust.Shared.Network.DummyHWId` and returns the value from `BEBRA_HWID_LEGACY`/`BEBRA_HWID_MODERN`. The types are found by name at patch time, patch application is idempotent, and a type that is not loaded yet makes the loader retry instead of failing the launch | Lets the launcher show a generated hardware ID without the player installing a mod by hand. The engine types still differ between builds, so a compile-time reference would break on the next engine update |
| `Directory.Build.props` | `LangVersion` fixed to `13.0` instead of `latest`, `RestorePackagesWithLockFile=true`, `RuntimeIdentifiers=win-x64`; `Directory.Packages.props` keeps the exact versions | The same sources must compile on any SDK; `packages.lock.json` plus `--locked-mode` in CI makes a dependency change visible; the RID entry lets the locked restore also serve the self-contained publish |

The project files are not vendored at all: they are ours, and editing them needs no entry in the
table above.

## Rules for vendored code

1. Any edit to a file that exists upstream is written into the table above in the same change:
   file, what changed, why. "Copied unchanged" is a claim that must stay true.
2. New files under `loader/` are ours; name them so this is obvious and describe them above.
3. When syncing with a newer upstream commit, update the commit hash here and re-check every row
   of the table. Prefer moving our code into separate files over editing upstream ones.
4. If a difference is only in the project files, say so there instead of hiding it in a source
   file: `loader/Directory.Packages.props` is the single place for package versions.

Build (needs the .NET 10 SDK):

```
dotnet restore loader/SS14.Loader/SS14.Loader.csproj --locked-mode
dotnet build loader/SS14.Loader/SS14.Loader.csproj -c Release -warnaserror --no-restore
# development build for the launcher (needs the .NET 10 runtime installed):
dotnet publish loader/SS14.Loader/SS14.Loader.csproj -c Release -r win-x64 --self-contained false -o loader/out
# the installer version (self-contained, no .pdb) is produced by scripts/build-release.ps1
```

The HWID patch is built into `loader/out` as well: the launcher looks for
`Bebra.HwidPatch.dll` next to `SS14.Loader.exe`, so a dev build needs both.

```
dotnet restore loader/Bebra.HwidPatch/Bebra.HwidPatch.csproj --locked-mode
dotnet publish loader/Bebra.HwidPatch/Bebra.HwidPatch.csproj -c Release --no-restore -o loader/out
```

`packages.lock.json` in `SS14.Loader/` and `Robust.LoaderApi/` is committed: after an intended
package change run `dotnet restore --force-evaluate` and commit the updated lock files. CI and the
release script restore with `--locked-mode` and fail when the lock file does not match.

`THIRD-PARTY-NOTICES.txt` lists the licences of everything shipped in the installer's `loader`
folder; `scripts/build-release.ps1` copies it into `loader/out-sc`. Update it when a package or a
native library is added.

Do not change behaviour in copied C# files without documenting it as described above.
