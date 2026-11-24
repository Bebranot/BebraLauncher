using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.Loader;

namespace SS14.Loader;

internal static class PatchManager
{
    private static readonly object _lock = new();
    private static readonly List<Assembly> _loadedPatches = new();
    private static readonly HashSet<Type> _initializedEntryPoints = new();
    private static readonly HashSet<Type> _executedPostInit = new();
    private static readonly HashSet<Type> _appliedPatchTypes = new();
    private static readonly HashSet<string> _patchDirectories = new(StringComparer.OrdinalIgnoreCase);
    private static readonly Dictionary<Type, int> _entryPointAttempts = new();
    private static readonly Dictionary<Type, int> _postInitAttempts = new();
    private static readonly Dictionary<Type, int> _patchAttempts = new();

    // Patch code is executed outside the lock: a patch that loads another assembly or waits for
    // a thread would otherwise block every AssemblyLoad in the game. The queue and the
    // single-runner flag keep that safe.
    private static readonly List<Assembly> _patchQueue = new();
    private static bool _processingPatches;
    private static bool _requeueCurrent;
    private static Assembly? _currentPatch;

    /// <summary>
    /// How many times a deferred entry point / Harmony patch is retried **after the game content is
    /// loaded**. Before that point a failure is almost always "the target type's assembly has not
    /// loaded yet", so it is deferred without counting (see <see cref="ContentReady"/>): a valid
    /// mod whose target loads late is never dropped.
    /// </summary>
    private const int MaxAttempts = 5;

    /// <summary>
    /// True once <c>Content.Client</c> is loaded — the last core game assembly, after which every
    /// type a client patch could target is present. Until then a missing target is expected, not a
    /// failure, so retries are not counted against <see cref="MaxAttempts"/> and nothing is given up.
    /// </summary>
    private static bool ContentReady => _contentClient != null;

    private static Assembly? _robustClient;
    private static Assembly? _robustShared;
    private static Assembly? _contentClient;
    private static Assembly? _contentShared;

    public static void Init(Assembly clientAssembly)
    {
        string[] patchPaths;
        lock (_lock)
        {
            _robustClient = clientAssembly;
            var envPatches = Environment.GetEnvironmentVariable("SS14_LOADER_PATCHES");
            if (string.IsNullOrWhiteSpace(envPatches))
                return;

            patchPaths = envPatches.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
            if (patchPaths.Length == 0)
                return;

            // Register assembly resolvers BEFORE loading any patch assemblies.
            // This ensures patches that reference Content.Client, Content.Shared, or local dependencies can resolve them.
            AppDomain.CurrentDomain.AssemblyResolve += OnAssemblyResolve;
            AssemblyLoadContext.Default.Resolving += OnAlResolving;
            AppDomain.CurrentDomain.AssemblyLoad += OnAssemblyLoad;
        }

        PatchLog.Write("[BebraLoader] Initializing patch manager with {0} patch(es)...", patchPaths.Length);

        // The engine's GameController static constructor kills the process when 0Harmony is
        // already loaded. It only runs once, so run it now, before Harmony exists, and load
        // Harmony afterwards. HarmonyBridge is the only type that touches HarmonyLib.
        RunEngineHarmonyGuard(clientAssembly);
        HarmonyBridge.Create("BebraLauncher.ClientPatcher");

        foreach (var path in patchPaths)
        {
            if (!File.Exists(path))
            {
                PatchLog.Write("[BebraLoader] Warning: patch file not found: {0}", path);
                continue;
            }

            var dir = Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(dir))
            {
                lock (_lock)
                {
                    _patchDirectories.Add(dir);
                }
            }

            try
            {
                // Loading fires OnAssemblyLoad, which only queues work; nothing runs here yet.
                var asm = AssemblyLoadContext.Default.LoadFromAssemblyPath(path);
                lock (_lock)
                {
                    _loadedPatches.Add(asm);
                }

                PatchLog.Write("[BebraLoader] Loaded patch assembly: {0} ({1})", asm.GetName().Name, path);
                EnqueuePatch(asm);
            }
            catch (Exception ex)
            {
                PatchLog.Write("[BebraLoader] Error loading patch {0}: {1}", path, ex);
            }

            // Run the patch outside the lock, right after it is loaded.
            ProcessPendingPatches();
        }
    }

    /// <summary>
    /// Runs the engine's anti-Harmony static constructor before Harmony is loaded.
    /// </summary>
    /// <remarks>
    /// <c>Robust.Client.GameController..cctor</c> looks through
    /// <see cref="AppDomain.GetAssemblies"/> and, when it finds an assembly named
    /// <c>0Harmony</c>, writes zeroes through an invalid pointer. The resulting
    /// <see cref="AccessViolationException"/> is fatal for the process, so a launcher that loads
    /// Harmony before the game starts can never open the client.
    /// <para>
    /// A static constructor runs at most once, so triggering it here — while no Harmony assembly
    /// has been loaded yet — makes the later load harmless. This is why
    /// <see cref="HarmonyBridge"/> must stay untouched until after this call.
    /// </para>
    /// </remarks>
    private static void RunEngineHarmonyGuard(Assembly clientAssembly)
    {
        var guardType = clientAssembly.GetType("Robust.Client.GameController", throwOnError: false);
        if (guardType == null)
        {
            PatchLog.Write("[BebraLoader] GameController not found; Harmony guard not run");
            return;
        }

        try
        {
            RuntimeHelpers.RunClassConstructor(guardType.TypeHandle);
        }
        catch (Exception ex)
        {
            PatchLog.Write("[BebraLoader] Failed to run the engine's Harmony guard early: {0}", ex.Message);
        }
    }

    private static void EnqueuePatch(Assembly patch)
    {
        lock (_lock)
        {
            if (!_patchQueue.Contains(patch))
                _patchQueue.Add(patch);
        }
    }

    private static void ProcessPendingPatches()
    {
        while (true)
        {
            Assembly patch;
            lock (_lock)
            {
                // A second thread (or a nested call from a patch loading an assembly) must not
                // start a parallel run; the active runner picks the new item up on its next pass.
                if (_processingPatches || _patchQueue.Count == 0)
                    return;

                _processingPatches = true;
                patch = _patchQueue[0];
                _patchQueue.RemoveAt(0);
                _currentPatch = patch;
            }

            try
            {
                InitPatchAssembly(patch);
                ApplyPendingPatches(patch);
            }
            catch (Exception ex)
            {
                PatchLog.Write("[BebraLoader] Error updating patch {0}: {1}", patch.GetName().Name, ex);
            }
            finally
            {
                lock (_lock)
                {
                    _processingPatches = false;
                    _currentPatch = null;

                    if (_requeueCurrent)
                    {
                        _requeueCurrent = false;
                        if (!_patchQueue.Contains(patch))
                            _patchQueue.Add(patch);
                    }
                }
            }
        }
    }

    private static Assembly? OnAlResolving(AssemblyLoadContext context, AssemblyName name)
    {
        return ResolveAssemblyByName(name.Name);
    }

    private static Assembly? OnAssemblyResolve(object? sender, ResolveEventArgs args)
    {
        var requestedName = new AssemblyName(args.Name).Name;
        return ResolveAssemblyByName(requestedName);
    }

    private static Assembly? ResolveAssemblyByName(string? name)
    {
        if (string.IsNullOrEmpty(name))
            return null;

        // 1. Direct references
        if (_robustClient != null && string.Equals(_robustClient.GetName().Name, name, StringComparison.OrdinalIgnoreCase))
            return _robustClient;
        if (_robustShared != null && string.Equals(_robustShared.GetName().Name, name, StringComparison.OrdinalIgnoreCase))
            return _robustShared;
        if (_contentClient != null && string.Equals(_contentClient.GetName().Name, name, StringComparison.OrdinalIgnoreCase))
            return _contentClient;
        if (_contentShared != null && string.Equals(_contentShared.GetName().Name, name, StringComparison.OrdinalIgnoreCase))
            return _contentShared;

        // 2. Scan all assemblies currently loaded in the AppDomain (across all AssemblyLoadContexts)
        foreach (var asm in AppDomain.CurrentDomain.GetAssemblies())
        {
            try
            {
                if (string.Equals(asm.GetName().Name, name, StringComparison.OrdinalIgnoreCase))
                    return asm;
            }
            catch
            {
                // Ignore assemblies whose name cannot be retrieved
            }
        }

        // 3. Scan patch directories for dependencies. Snapshot under the lock: Init adds
        // directories while other threads may already be resolving assemblies.
        string[] patchDirectories;
        lock (_lock)
            patchDirectories = _patchDirectories.ToArray();

        foreach (var dir in patchDirectories)
        {
            var candidate = Path.Combine(dir, name + ".dll");
            if (File.Exists(candidate))
            {
                try
                {
                    return AssemblyLoadContext.Default.LoadFromAssemblyPath(candidate);
                }
                catch
                {
                    // Ignore load errors for candidate
                }
            }
        }

        return null;
    }

    private static void OnAssemblyLoad(object? sender, AssemblyLoadEventArgs args)
    {
        var name = args.LoadedAssembly.GetName().Name;
        var relevant = false;

        lock (_lock)
        {
            var asm = args.LoadedAssembly;

            if (name == "Robust.Shared") _robustShared = asm;
            else if (name == "Content.Client") _contentClient = asm;
            else if (name == "Content.Shared") _contentShared = asm;

            if (name is "Content.Client" or "Content.Shared" or "Robust.Shared" || (name != null && name.EndsWith(".Client")))
            {
                foreach (var patch in _loadedPatches)
                {
                    // Never re-queue the patch whose own code triggered this load right away: that
                    // would make it (and anything it loads) run again forever. But if it just pulled
                    // in one of the core game assemblies itself, it still has to see them (content
                    // injection, ContentInit), so it gets exactly one more pass once it is done.
                    // Only three assemblies qualify and each loads once, so this cannot loop.
                    if (patch == _currentPatch)
                    {
                        if (name is "Content.Client" or "Content.Shared" or "Robust.Shared")
                            _requeueCurrent = true;
                        continue;
                    }

                    if (!_patchQueue.Contains(patch))
                        _patchQueue.Add(patch);
                }

                relevant = true;
            }
        }

        if (!relevant)
            return;

        PatchLog.Write("[BebraLoader] Game assembly loaded: {0}, updating patches...", name);
        // Executed outside the lock: this is where third-party code runs.
        ProcessPendingPatches();
    }

    private static IEnumerable<Type> GetSafeTypes(Assembly assembly)
    {
        try
        {
            return assembly.GetTypes();
        }
        catch (ReflectionTypeLoadException ex)
        {
            return ex.Types.Where(t => t != null)!;
        }
        catch
        {
            return Array.Empty<Type>();
        }
    }

    private static void ApplyPendingPatches(Assembly patch)
    {
        if (!HarmonyBridge.IsReady) return;

        foreach (var type in GetSafeTypes(patch))
        {
            if (_appliedPatchTypes.Contains(type))
                continue;

            // Only attempt Harmony class patching if the type has Harmony attributes
            var hasHarmonyAttr = false;
            try
            {
                hasHarmonyAttr = type.GetCustomAttributes(inherit: true)
                    .Any(a => a.GetType().FullName?.StartsWith("HarmonyLib.Harmony", StringComparison.Ordinal) == true);
            }
            catch
            {
                // If reading attributes fails, dependencies might not be loaded yet
                continue;
            }

            if (!hasHarmonyAttr)
                continue;

            try
            {
                var patched = HarmonyBridge.ApplyPatches(type);

                if (patched > 0)
                {
                    // Done: stop rescanning it on every AssemblyLoad.
                    _appliedPatchTypes.Add(type);
                    PatchLog.Write("[BebraLoader] Applied Harmony patch {0} ({1} method(s) patched)", type.Name, patched);
                }
                else if (ContentReady)
                {
                    // Patch() found nothing to do and every core game assembly is already loaded, so
                    // the target really is absent (a different build or fork). Mark it processed.
                    _appliedPatchTypes.Add(type);
                    PatchLog.Write("[BebraLoader] Harmony patch {0} has no methods to patch, marked as processed", type.Name);
                }
                else
                {
                    // No target yet, but content is still loading (e.g. a [HarmonyPatch] whose
                    // TargetMethod() resolves a type that is not in memory yet). Leave it unmarked so
                    // it gets another pass once more game assemblies load, instead of dropping it now.
                    PatchLog.Write("[BebraLoader] Harmony patch {0} has no target yet, waiting for game content", type.Name);
                }
            }
            catch (Exception ex)
            {
                // A throw usually means the target type's assembly is not loaded yet. While the game
                // content is still loading that is expected, so defer without counting — only once
                // everything is loaded does a repeated failure burn the retry budget and give up.
                if (!ContentReady)
                {
                    PatchLog.Write("[BebraLoader] Harmony patch deferred for {0} (waiting for game content): {1}",
                        type.Name, ex.Message);
                    continue;
                }

                var count = _patchAttempts.TryGetValue(type, out var current) ? current + 1 : 1;
                _patchAttempts[type] = count;

                if (count >= MaxAttempts)
                {
                    _appliedPatchTypes.Add(type);
                    PatchLog.Write("[BebraLoader] Harmony patch {0} failed {1} times, giving up: {2}",
                        type.Name, count, ex.Message);
                }
                else
                {
                    PatchLog.Write("[BebraLoader] Harmony patch deferred for {0} ({1}/{2}): {3}",
                        type.Name, count, MaxAttempts, ex.Message);
                }
            }
        }
    }

    private static void InitPatchAssembly(Assembly patch)
    {
        foreach (var type in GetSafeTypes(patch))
        {
            // Universal static field injection: inject into any type that has these fields
            SetStaticField(type, "RobustClient", _robustClient);
            SetStaticField(type, "RobustShared", _robustShared);
            SetStaticField(type, "ContentClient", _contentClient);
            SetStaticField(type, "ContentShared", _contentShared);
            SetStaticField(type, "Harmony", HarmonyBridge.Instance);

            // A type that declares a static `Harmony` hook (the field/property the loader injects
            // into) is clearly meant to integrate with the loader, whatever its name — treat it as a
            // candidate so a mod with an unconventional class name still gets its entry point run.
            const BindingFlags staticFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static;
            var hasHarmonyHook = type.GetField("Harmony", staticFlags) != null
                || type.GetProperty("Harmony", staticFlags) != null;

            // Entry point discovery: supports MarseyEntry, PatchEntry, ModEntry, TranslatorEntry, Entry, Plugin, etc.
            var isCandidateEntry = hasHarmonyHook
                || type.Name.EndsWith("Entry")
                || type.Name.EndsWith("Plugin")
                || type.Name.EndsWith("Mod")
                || type.Name.EndsWith("Patch")
                || type.Name is "Entry" or "Plugin" or "Main" or "Translator";

            if (isCandidateEntry && !_initializedEntryPoints.Contains(type))
            {
                var entryMethod = type.GetMethod("Init", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)
                    ?? type.GetMethod("Initialize", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)
                    ?? type.GetMethod("Main", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static);

                if (entryMethod != null)
                {
                    var pars = entryMethod.GetParameters();
                    try
                    {
                        if (pars.Length == 0)
                        {
                            entryMethod.Invoke(null, null);
                            _initializedEntryPoints.Add(type);
                            PatchLog.Write("[BebraLoader] Executed entry point on {0}.{1}()", type.FullName, entryMethod.Name);
                        }
                        else if (pars.Length == 1 && HarmonyBridge.TakesHarmony(pars[0]))
                        {
                            entryMethod.Invoke(null, new object?[] { HarmonyBridge.Instance });
                            _initializedEntryPoints.Add(type);
                            PatchLog.Write("[BebraLoader] Executed entry point on {0}.{1}(Harmony)", type.FullName, entryMethod.Name);
                        }
                        else
                        {
                            // No signature we can call: stop rescanning it on every assembly load.
                            _initializedEntryPoints.Add(type);
                        }
                    }
                    catch (Exception ex)
                    {
                        // Before the content is loaded a throw is almost always "my target isn't here
                        // yet", so defer without spending the retry budget; only count failures — and
                        // eventually give up — once every core game assembly is in memory.
                        if (!ContentReady)
                        {
                            PatchLog.Write("[BebraLoader] Entry point {0} deferred (waiting for game content): {1}",
                                type.FullName, Reason(ex));
                        }
                        else
                        {
                            var count = _entryPointAttempts.TryGetValue(type, out var current) ? current + 1 : 1;
                            _entryPointAttempts[type] = count;

                            if (count >= MaxAttempts)
                            {
                                _initializedEntryPoints.Add(type);
                                PatchLog.Write("[BebraLoader] Entry point {0} failed {1} times, giving up: {2}",
                                    type.FullName, count, Reason(ex));
                            }
                            else
                            {
                                PatchLog.Write("[BebraLoader] Entry point {0} deferred/failed ({1}/{2}): {3}",
                                    type.FullName, count, MaxAttempts, Reason(ex));
                            }
                        }
                    }
                }
                else
                {
                    _initializedEntryPoints.Add(type);
                }
            }

            // Post-content hook: invoked when Content.Client is available
            if (_contentClient != null && !_executedPostInit.Contains(type))
            {
                var postInitMethod = type.GetMethod("ContentInit", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)
                    ?? type.GetMethod("PostInit", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static);

                if (postInitMethod != null && postInitMethod.GetParameters().Length == 0)
                {
                    try
                    {
                        postInitMethod.Invoke(null, null);
                        _executedPostInit.Add(type);
                        PatchLog.Write("[BebraLoader] Executed ContentInit on {0}.{1}()", type.FullName, postInitMethod.Name);
                    }
                    catch (Exception ex)
                    {
                        var count = _postInitAttempts.TryGetValue(type, out var current) ? current + 1 : 1;
                        _postInitAttempts[type] = count;

                        if (count >= MaxAttempts)
                        {
                            _executedPostInit.Add(type);
                            PatchLog.Write("[BebraLoader] ContentInit {0}.{1} failed {2} times, giving up: {3}",
                                type.FullName, postInitMethod.Name, count, Reason(ex));
                        }
                        else
                        {
                            PatchLog.Write("[BebraLoader] ContentInit note on {0} ({1}/{2}): {3}",
                                type.FullName, count, MaxAttempts, Reason(ex));
                        }
                    }
                }
            }
        }
    }

    /// <summary>Hands a value to a patch through a static field or property with this name.</summary>
    private static void SetStaticField(Type type, string fieldName, object? value)
    {
        if (value == null) return;
        const BindingFlags flags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static;
        try
        {
            var field = type.GetField(fieldName, flags);
            if (field != null && field.FieldType.IsAssignableFrom(value.GetType()))
            {
                field.SetValue(null, value);
                return;
            }

            // Our own patches declare `public static Harmony? Harmony { get; set; }`.
            var property = type.GetProperty(fieldName, flags);
            if (property is { CanWrite: true } && property.PropertyType.IsAssignableFrom(value.GetType()))
            {
                property.SetValue(null, value);
            }
        }
        catch
        {
            // Ignore type assignment errors
        }
    }

    /// <summary>What really went wrong: a reflected call wraps it in TargetInvocationException.</summary>
    private static string Reason(Exception ex) => (ex as TargetInvocationException)?.InnerException?.Message ?? ex.Message;
}
