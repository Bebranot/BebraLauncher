# Loader

`SS14.Loader` starts the game client: it checks the engine signature, loads `Robust.Client.dll`
from the engine zip and hands it the game files from `content.db`. It has to be a .NET program,
so it can't be part of the Rust core.

The sources here are copied unchanged from the official launcher
(https://github.com/space-wizards/SS14.Launcher, commit `437fe66`, MIT, see `LICENSE-SS14.Launcher.txt`):

- `SS14.Loader/*.cs`, `Robust.LoaderApi/` (MIT, `Robust.LoaderApi/LICENSE.txt`)
- `Shared/*.cs` are four helper files the Loader uses from `SS14.Launcher/`

Only the project files (`*.csproj`, `Directory.*.props`) are ours: no `Launcher.props`, no trimming step.

Build (needs the .NET 10 SDK):

```
dotnet publish loader/SS14.Loader/SS14.Loader.csproj -c Release -r win-x64 --self-contained false -o loader/out
```

Don't edit the copied C# files to change behaviour. If the Loader has to change, say so and why in the
commit message so it can be compared with upstream later.
