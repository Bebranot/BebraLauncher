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

    private static Assembly? _robustClient;
    private static Assembly? _robustShared;
    private static Assembly? _contentClient;
    private static Assembly? _contentShared;

    public static void Init(Assembly clientAssembly)
    {
        lock (_lock)
        {
            _robustClient = clientAssembly;
            var envPatches = Environment.GetEnvironmentVariable("SS14_LOADER_PATCHES");
            if (string.IsNullOrWhiteSpace(envPatches))
                return;

            var patchPaths = envPatches.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
            if (patchPaths.Length == 0)
                return;

            // Register assembly resolvers BEFORE loading any patch assemblies.
            // This ensures patches that reference Content.Client, Content.Shared, or local dependencies can resolve them.
            AppDomain.CurrentDomain.AssemblyResolve += OnAssemblyResolve;
            AssemblyLoadContext.Default.Resolving += OnAlResolving;
            AppDomain.CurrentDomain.AssemblyLoad += OnAssemblyLoad;

            Console.WriteLine("[BebraLoader] Initializing patch manager with {0} patch(es)...", patchPaths.Length);
            _harmony = new Harmony("BebraLauncher.ClientPatcher");

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
                    _patchDirectories.Add(dir);
                }

                try
                {
                    var asm = AssemblyLoadContext.Default.LoadFromAssemblyPath(path);
                    _loadedPatches.Add(asm);
                    Console.WriteLine("[BebraLoader] Loaded patch assembly: {0} ({1})", asm.GetName().Name, path);

                    InitPatchAssembly(asm);
                    ApplyPendingPatches(asm);
                }
                catch (Exception ex)
                {
                    Console.WriteLine("[BebraLoader] Error loading patch {0}: {1}", path, ex);
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
        lock (_lock)
        {
            var asm = args.LoadedAssembly;
            var name = asm.GetName().Name;

            if (name == "Robust.Shared") _robustShared = asm;
            else if (name == "Content.Client") _contentClient = asm;
            else if (name == "Content.Shared") _contentShared = asm;

            if (name is "Content.Client" or "Content.Shared" or "Robust.Shared" || (name != null && name.EndsWith(".Client")))
            {
                Console.WriteLine("[BebraLoader] Game assembly loaded: {0}, updating patches...", name);
                foreach (var patch in _loadedPatches)
                {
                    try
                    {
                        InitPatchAssembly(patch);
                        ApplyPendingPatches(patch);
                    }
                    catch (Exception ex)
                    {
                        Console.WriteLine("[BebraLoader] Error updating patch {0} on {1}: {2}", patch.GetName().Name, name, ex);
                    }
                }
            }
        }
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
                if (methods != null && methods.Count > 0)
                {
                    _appliedPatchTypes.Add(type);
                    Console.WriteLine("[BebraLoader] Applied Harmony patch {0} ({1} method(s) patched)", type.Name, methods.Count);
                }
            }
            catch (Exception ex)
            {
                // Class targets types in an assembly not loaded yet; will retry on next assembly load
                Console.WriteLine("[BebraLoader] Harmony patch deferred for {0}: {1}", type.Name, ex.Message);
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
                    }
                    catch (Exception ex)
                    {
                        Console.WriteLine("[BebraLoader] Entry point {0} deferred/failed: {1}", type.FullName, ex.Message);
                        // Do NOT add to _initializedEntryPoints so it can retry when content loads
                    }
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
                        Console.WriteLine("[BebraLoader] ContentInit note on {0}: {1}", type.FullName, ex.Message);
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
