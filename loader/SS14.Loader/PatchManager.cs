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
    private static Harmony? _harmony;
    private static readonly List<Assembly> _loadedPatches = new();
    private static readonly HashSet<Type> _initializedEntryPoints = new();
    private static Assembly? _robustClient;
    private static Assembly? _robustShared;
    private static Assembly? _contentClient;
    private static Assembly? _contentShared;

    public static void Init(Assembly clientAssembly)
    {
        _robustClient = clientAssembly;
        var envPatches = Environment.GetEnvironmentVariable("SS14_LOADER_PATCHES");
        if (string.IsNullOrWhiteSpace(envPatches))
            return;

        var patchPaths = envPatches.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (patchPaths.Length == 0)
            return;

        Console.WriteLine("[BebraLoader] Initializing patch manager with {0} patch(es)...", patchPaths.Length);
        _harmony = new Harmony("BebraLauncher.ClientPatcher");

        AppDomain.CurrentDomain.AssemblyLoad += OnAssemblyLoad;

        foreach (var path in patchPaths)
        {
            if (!File.Exists(path))
            {
                Console.WriteLine("[BebraLoader] Warning: patch file not found: {0}", path);
                continue;
            }

            try
            {
                var asm = AssemblyLoadContext.Default.LoadFromAssemblyPath(path);
                _loadedPatches.Add(asm);
                Console.WriteLine("[BebraLoader] Loaded patch assembly: {0} ({1})", asm.GetName().Name, path);

                InitPatchAssembly(asm);
                TryApplyHarmonyPatches(asm);
            }
            catch (Exception ex)
            {
                Console.WriteLine("[BebraLoader] Error loading patch {0}: {1}", path, ex);
            }
        }
    }

    private static void OnAssemblyLoad(object? sender, AssemblyLoadEventArgs args)
    {
        var asm = args.LoadedAssembly;
        var name = asm.GetName().Name;

        if (name == "Robust.Shared") _robustShared = asm;
        else if (name == "Content.Client") _contentClient = asm;
        else if (name == "Content.Shared") _contentShared = asm;

        if (name is "Content.Client" or "Content.Shared" or "Robust.Shared")
        {
            Console.WriteLine("[BebraLoader] Game assembly loaded: {0}, updating patches...", name);
            foreach (var patch in _loadedPatches)
            {
                try
                {
                    InitPatchAssembly(patch);
                    TryApplyHarmonyPatches(patch);
                }
                catch (Exception ex)
                {
                    Console.WriteLine("[BebraLoader] Error updating patch {0} on {1}: {2}", patch.GetName().Name, name, ex);
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

    private static void TryApplyHarmonyPatches(Assembly patch)
    {
        if (_harmony == null) return;
        try
        {
            _harmony.PatchAll(patch);
            Console.WriteLine("[BebraLoader] Applied Harmony patches from {0}", patch.GetName().Name);
        }
        catch (Exception ex)
        {
            Console.WriteLine("[BebraLoader] Harmony patch attempt on {0}: {1}", patch.GetName().Name, ex.Message);
        }
    }

    private static void InitPatchAssembly(Assembly patch)
    {
        foreach (var type in GetSafeTypes(patch))
        {
            if (type.Name is "MarseyPatch" or "Patch" or "ClientPatch")
            {
                SetStaticField(type, "RobustClient", _robustClient);
                SetStaticField(type, "RobustShared", _robustShared);
                SetStaticField(type, "ContentClient", _contentClient);
                SetStaticField(type, "ContentShared", _contentShared);
            }

            if (type.Name is "MarseyEntry" or "PatchEntry" or "ModEntry")
            {
                if (_initializedEntryPoints.Add(type))
                {
                    var entryMethod = type.GetMethod("Init", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)
                        ?? type.GetMethod("Main", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static);
                    if (entryMethod != null && entryMethod.GetParameters().Length == 0)
                    {
                        try
                        {
                            entryMethod.Invoke(null, null);
                            Console.WriteLine("[BebraLoader] Executed entry point on {0}", type.FullName);
                        }
                        catch (Exception ex)
                        {
                            Console.WriteLine("[BebraLoader] Error invoking entry point on {0}: {1}", type.FullName, ex);
                        }
                    }
                }
            }
        }
    }

    private static void SetStaticField(Type type, string fieldName, object? value)
    {
        if (value == null) return;
        var field = type.GetField(fieldName, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static);
        if (field != null && field.FieldType.IsAssignableFrom(value.GetType()))
        {
            try
            {
                field.SetValue(null, value);
            }
            catch
            {
                // Ignore type assignment errors
            }
        }
    }
}
