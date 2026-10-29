using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Loader;
using HarmonyLib;

namespace SS14.Loader;

internal static class PatchManager
{
    private static readonly object _lock = new();
    private static Harmony? _harmony;
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
    private static Assembly? _currentPatch;

    /// <summary>How many times a deferred entry point / Harmony patch is retried.</summary>
    private const int MaxAttempts = 3;

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

            _harmony = new Harmony("BebraLauncher.ClientPatcher");
        }

        Console.WriteLine("[BebraLoader] Initializing patch manager with {0} patch(es)...", patchPaths.Length);

        foreach (var path in patchPaths)
        {
            if (!File.Exists(path))
            {
                Console.WriteLine("[BebraLoader] Warning: patch file not found: {0}", path);
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

                Console.WriteLine("[BebraLoader] Loaded patch assembly: {0} ({1})", asm.GetName().Name, path);
                EnqueuePatch(asm);
            }
            catch (Exception ex)
            {
                Console.WriteLine("[BebraLoader] Error loading patch {0}: {1}", path, ex);
            }

            // Run the patch outside the lock, right after it is loaded.
            ProcessPendingPatches();
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
                Console.WriteLine("[BebraLoader] Error updating patch {0}: {1}", patch.GetName().Name, ex);
            }
            finally
            {
                lock (_lock)
                {
                    _processingPatches = false;
                    _currentPatch = null;
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

        // 3. Scan patch directories for dependencies
        foreach (var dir in _patchDirectories)
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
                    // Never re-queue the patch whose own code triggered this load: that would
                    // make it (and anything it loads) run again forever.
                    if (patch == _currentPatch)
                        continue;

                    if (!_patchQueue.Contains(patch))
                        _patchQueue.Add(patch);
                }

                relevant = true;
            }
        }

        if (!relevant)
            return;

        Console.WriteLine("[BebraLoader] Game assembly loaded: {0}, updating patches...", name);
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
        if (_harmony == null) return;

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
                var processor = _harmony.CreateClassProcessor(type);
                var methods = processor.Patch();

                // Patch() returning nothing means Harmony considered the class handled (usually
                // no matching target). Retrying it on every AssemblyLoad only burned time.
                _appliedPatchTypes.Add(type);

                if (methods != null && methods.Count > 0)
                {
                    Console.WriteLine("[BebraLoader] Applied Harmony patch {0} ({1} method(s) patched)", type.Name, methods.Count);
                }
                else
                {
                    Console.WriteLine("[BebraLoader] Harmony patch {0} has no methods to patch, marked as processed", type.Name);
                }
            }
            catch (Exception ex)
            {
                // Class targets types in an assembly not loaded yet; retry on the next assembly
                // load, but only a few times: a permanently broken patch must not cost every load.
                var count = _patchAttempts.TryGetValue(type, out var current) ? current + 1 : 1;
                _patchAttempts[type] = count;

                if (count >= MaxAttempts)
                {
                    _appliedPatchTypes.Add(type);
                    Console.WriteLine("[BebraLoader] Harmony patch {0} failed {1} times, giving up: {2}",
                        type.Name, count, ex.Message);
                }
                else
                {
                    Console.WriteLine("[BebraLoader] Harmony patch deferred for {0} ({1}/{2}): {3}",
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
            SetStaticField(type, "Harmony", _harmony);

            // Entry point discovery: supports MarseyEntry, PatchEntry, ModEntry, TranslatorEntry, Entry, Plugin, etc.
            var isCandidateEntry = type.Name.EndsWith("Entry")
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
                            Console.WriteLine("[BebraLoader] Executed entry point on {0}.{1}()", type.FullName, entryMethod.Name);
                        }
                        else if (pars.Length == 1 && pars[0].ParameterType == typeof(Harmony))
                        {
                            entryMethod.Invoke(null, new object?[] { _harmony });
                            _initializedEntryPoints.Add(type);
                            Console.WriteLine("[BebraLoader] Executed entry point on {0}.{1}(Harmony)", type.FullName, entryMethod.Name);
                        }
                        else
                        {
                            // No signature we can call: stop rescanning it on every assembly load.
                            _initializedEntryPoints.Add(type);
                        }
                    }
                    catch (Exception ex)
                    {
                        var count = _entryPointAttempts.TryGetValue(type, out var current) ? current + 1 : 1;
                        _entryPointAttempts[type] = count;

                        if (count >= MaxAttempts)
                        {
                            _initializedEntryPoints.Add(type);
                            Console.WriteLine("[BebraLoader] Entry point {0} failed {1} times, giving up: {2}",
                                type.FullName, count, ex.Message);
                        }
                        else
                        {
                            // Retry when content loads, but not forever.
                            Console.WriteLine("[BebraLoader] Entry point {0} deferred/failed ({1}/{2}): {3}",
                                type.FullName, count, MaxAttempts, ex.Message);
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
                        Console.WriteLine("[BebraLoader] Executed ContentInit on {0}.{1}()", type.FullName, postInitMethod.Name);
                    }
                    catch (Exception ex)
                    {
                        var count = _postInitAttempts.TryGetValue(type, out var current) ? current + 1 : 1;
                        _postInitAttempts[type] = count;

                        if (count >= MaxAttempts)
                        {
                            _executedPostInit.Add(type);
                            Console.WriteLine("[BebraLoader] ContentInit {0}.{1} failed {2} times, giving up: {3}",
                                type.FullName, postInitMethod.Name, count, ex.Message);
                        }
                        else
                        {
                            Console.WriteLine("[BebraLoader] ContentInit note on {0} ({1}/{2}): {3}",
                                type.FullName, count, MaxAttempts, ex.Message);
                        }
                    }
                }
            }
        }
    }

    private static void SetStaticField(Type type, string fieldName, object? value)
    {
        if (value == null) return;
        try
        {
            var field = type.GetField(fieldName, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static);
            if (field != null && field.FieldType.IsAssignableFrom(value.GetType()))
            {
                field.SetValue(null, value);
            }
        }
        catch
        {
            // Ignore type assignment errors
        }
    }
}
