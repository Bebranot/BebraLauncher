using System;
using System.Collections.Generic;
using System.Diagnostics;
using HarmonyLib;

namespace Bebra.PerfPatch;

/// <summary>
/// Client-side work the game does every frame although its result changes far less often. Each
/// patch is switched on by its own variable, so the launcher's options decide what runs:
/// <list type="bullet">
/// <item><c>BEBRA_PERF_OUTLINE=1</c>: the hover outline (<c>InteractionOutlineSystem</c>) looks
/// for the entity under the cursor at most ~33 times a second instead of every frame.</item>
/// <item><c>BEBRA_PERF_STATUS_ICONS=1</c>: the status icon list of an entity
/// (<c>StatusIconSystem.GetStatusIcons</c>, one event per visible entity per frame) is reused for
/// 100 ms.</item>
/// </list>
/// Neither changes what the game shows, only how fresh it is: an outline or an icon can appear up
/// to a few frames late.
/// </summary>
/// <remarks>
/// The content types are resolved by name, as in <c>Bebra.HwidPatch</c>: a compile-time reference
/// would tie the patch to one build of the game. A method that is missing (another fork, a newer
/// build) is skipped and logged by the loader, the game then runs unpatched.
/// </remarks>
public static class PerfPatch
{
    private const string OutlineEnv = "BEBRA_PERF_OUTLINE";
    private const string StatusIconsEnv = "BEBRA_PERF_STATUS_ICONS";

    /// <summary>Shortest time between two outline updates.</summary>
    private static readonly long OutlineInterval = Stopwatch.Frequency * 30 / 1000;
    /// <summary>How long a status icon list is reused.</summary>
    private static readonly long IconsLifetime = Stopwatch.Frequency / 10;
    /// <summary>How often entries of entities no longer drawn are dropped.</summary>
    private static readonly long IconsSweep = Stopwatch.Frequency * 2;

    /// <summary>The Harmony instance, injected by the loader's <c>InitPatchAssembly</c>.</summary>
    public static Harmony? Harmony { get; set; }

    private static bool _outlineDone;
    private static bool _iconsDone;
    private static long _lastOutline;

    // Only the main thread draws, but nothing checks that: a lock costs nothing uncontended.
    private static readonly object IconsLock = new();
    private static readonly Dictionary<object, (long Stamp, object List)> Icons = new();
    private static long _lastSweep;

    /// <summary>Called by the loader once Content.Client is loaded.</summary>
    public static void ContentInit()
    {
        var harmony = Harmony ?? throw new InvalidOperationException("Harmony was not passed in");

        if (!_outlineDone && Enabled(OutlineEnv))
        {
            var method = AccessTools.Method(
                "Content.Client.Outline.InteractionOutlineSystem:FrameUpdate",
                new[] { typeof(float) });
            if (method is not null)
            {
                harmony.Patch(method, prefix: new HarmonyMethod(typeof(PerfPatch), nameof(BeforeOutline)));
                _outlineDone = true;
            }
        }

        if (!_iconsDone && Enabled(StatusIconsEnv))
        {
            var method = AccessTools.Method("Content.Client.StatusIcon.StatusIconSystem:GetStatusIcons");
            if (method is not null)
            {
                harmony.Patch(
                    method,
                    prefix: new HarmonyMethod(typeof(PerfPatch), nameof(BeforeIcons)),
                    postfix: new HarmonyMethod(typeof(PerfPatch), nameof(AfterIcons)));
                _iconsDone = true;
            }
        }

        var missing = new List<string>();
        if (!_outlineDone && Enabled(OutlineEnv))
            missing.Add("InteractionOutlineSystem.FrameUpdate");
        if (!_iconsDone && Enabled(StatusIconsEnv))
            missing.Add("StatusIconSystem.GetStatusIcons");
        // The loader retries a few times and logs the message when it gives up.
        if (missing.Count > 0)
            throw new InvalidOperationException("not found: " + string.Join(", ", missing));
    }

    private static bool Enabled(string name) => Environment.GetEnvironmentVariable(name) == "1";

    private static bool BeforeOutline()
    {
        var now = Stopwatch.GetTimestamp();
        if (now - _lastOutline < OutlineInterval)
            return false;
        _lastOutline = now;
        return true;
    }

    private static bool BeforeIcons(object[] __args, ref object __result, out bool __state)
    {
        var now = Stopwatch.GetTimestamp();
        lock (IconsLock)
        {
            if (now - _lastSweep > IconsSweep)
            {
                _lastSweep = now;
                var stale = new List<object>();
                foreach (var (key, entry) in Icons)
                {
                    if (now - entry.Stamp > IconsLifetime)
                        stale.Add(key);
                }
                foreach (var key in stale)
                    Icons.Remove(key);
            }

            if (Icons.TryGetValue(__args[0], out var cached) && now - cached.Stamp <= IconsLifetime)
            {
                __result = cached.List;
                __state = false;
                return false;
            }
        }
        // Store what the original returns.
        __state = true;
        return true;
    }

    private static void AfterIcons(object[] __args, object __result, bool __state)
    {
        if (!__state || __result is null)
            return;
        lock (IconsLock)
            Icons[__args[0]] = (Stopwatch.GetTimestamp(), __result);
    }
}
