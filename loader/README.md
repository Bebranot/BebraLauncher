# Loader

`SS14.Loader` starts the game client: it checks the engine signature, loads `Robust.Client.dll`
from the engine zip and hands it the game files from `content.db`. It has to be a .NET program,
so it can't be part of the Rust core.

## What is upstream and what is ours

Most sources come from the official launcher
(https://github.com/space-wizards/SS14.Launcher, commit `437fe66`, MIT, see `LICENSE-SS14.Launcher.txt`):

- `SS14.Loader/*.cs`, `Robust.LoaderApi/` (MIT, `Robust.LoaderApi/LICENSE.txt`)
- `Shared/*.cs` are four helper files the Loader uses from `SS14.Launcher/`
- project files (`*.csproj`, `Directory.*.props`) are ours: no `Launcher.props`, no trimming step.

The differences we know of (they were checked against upstream `437fe66` during the code review of
2026-10-29) are listed below; this list is the contract. If you find another difference, add it here
before a release — code that changes behaviour without a line here is a bug:

| File | Difference | Why |
|---|---|---|
| `SS14.Loader/PatchManager.cs` | Our file, absent upstream (~317 lines): reads `SS14_LOADER_PATCHES`, loads the .dll files with `AssemblyLoadContext.Default`, wires assembly resolvers, applies Harmony patches, calls patch entry points | Lets the launcher install client mods (see the "Моды" tab). This is execution of unsigned third-party code — README/PRODUCT must not claim otherwise |
| `SS14.Loader/Program.cs` | `PatchManager.Init(clientAssembly)` is called right after `Robust.Client.dll` is opened, before the Loader entry point runs | Patches have to be applied before the game initialises |
| `SS14.Loader/Program.cs` | `BEBRA_OPTIMIZE` block: `GCSettings.LatencyMode = SustainedLowLatency` and `ThreadPriority.AboveNormal` when the variable is `1`/`true`, failures are only logged | Opt-in runtime tuning, requested by the launcher |
| `SS14.Loader/SS14.Loader.csproj` | `Lib.Harmony` package reference (all package versions are pinned in `Directory.Packages.props`) | Needed by `PatchManager` |

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
dotnet publish loader/SS14.Loader/SS14.Loader.csproj -c Release -r win-x64 --self-contained false -o loader/out
```

Do not change behaviour in copied C# files without documenting it as described above.
