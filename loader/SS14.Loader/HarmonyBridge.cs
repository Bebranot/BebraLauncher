using System;
using System.Reflection;
using HarmonyLib;

namespace SS14.Loader;

/// <summary>
/// Every reference to HarmonyLib lives in this type.
/// </summary>
/// <remarks>
/// Loading the Harmony assembly too early is fatal: <c>Robust.Client.GameController</c>'s static
/// constructor scans <see cref="AppDomain.GetAssemblies"/> and deliberately crashes the process
/// (an <see cref="AccessViolationException"/> that the runtime turns into a fail-fast) when it
/// finds an assembly named <c>0Harmony</c>. That guard is a one-shot static constructor, so
/// <see cref="PatchManager"/> runs it before this type is first touched.
/// <para>
/// Keeping the HarmonyLib types here also means a launch without patches never loads the
/// assembly at all. <see cref="PatchManager"/> must not name a HarmonyLib type in a field or a
/// method signature: the CLR resolves those when the type is loaded, which was exactly what made
/// a patch-less launch die before the client printed anything.
/// </para>
/// </remarks>
internal static class HarmonyBridge
{
    private static Harmony? _harmony;

    public static bool IsReady => _harmony != null;

    /// <summary>The Harmony instance, passed to patch assemblies through their static fields.</summary>
    public static object? Instance => _harmony;

    public static void Create(string id)
    {
        // Harmony's own debug output (and its log file) would advertise the patched client.
        Harmony.DEBUG = false;
        _harmony = new Harmony(id);
    }

    /// <summary>Applies the attributes of one patch class and returns how many methods were patched.</summary>
    public static int ApplyPatches(Type type)
    {
        var methods = _harmony!.CreateClassProcessor(type).Patch();
        return methods?.Count ?? 0;
    }

    /// <summary>Checks for an entry point that takes the Harmony instance as its only parameter.</summary>
    public static bool TakesHarmony(ParameterInfo parameter)
    {
        return parameter.ParameterType == typeof(Harmony);
    }
}
