using System;
using System.Collections.Generic;
using System.Text;
using HarmonyLib;

namespace Bebra.HwidPatch;

/// <summary>
/// Replaces the hardware ID the engine computes from the machine with the one the launcher
/// generates. Values come from <c>BEBRA_HWID_LEGACY</c> (32 bytes, hex) and
/// <c>BEBRA_HWID_MODERN</c> (the same 32 bytes behind a leading zero byte, hex). Without them the
/// engine's own value is left in place: a random ID made up here would change on every process
/// and could never be rolled back from the launcher.
/// </summary>
/// <remarks>
/// The engine types are resolved by name at patch time, not referenced: the patch must work with
/// whatever Robust.Client the launcher downloaded, and a compile-time reference would tie it to
/// one build. A type that is not loaded yet is not an error: the loader retries the patch when
/// more game assemblies are loaded.
/// </remarks>
public static class HwidPatch
{
    private const string LegacyEnv = "BEBRA_HWID_LEGACY";
    private const string ModernEnv = "BEBRA_HWID_MODERN";

    /// <summary>The Harmony instance, injected by the loader's <c>InitPatchAssembly</c>.</summary>
    public static Harmony? Harmony { get; set; }

    private static readonly HashSet<string> _patchedTypes = new(StringComparer.Ordinal);
    private static byte[]? _legacy;
    private static byte[]? _modern;

    /// <summary>Called by the loader once the engine assemblies are loaded.</summary>
    public static void Init()
    {
        var applied = Patch("Robust.Client.HWId.BasicHWId");
        applied += Patch("Robust.Shared.Network.DummyHWId");

        // Nothing was found: the loader runs this again after the next game assembly loads.
        if (applied == 0)
            throw new InvalidOperationException("no HWId type is loaded yet");
    }

    /// <summary>Patches the HWID getters of one type that are present. Returns how many it added.</summary>
    private static int Patch(string typeName)
    {
        var type = AccessTools.TypeByName(typeName);
        if (type is null)
            return 0;
        if (_patchedTypes.Contains(typeName))
            return 1;

        var harmony = Harmony ?? throw new InvalidOperationException("Harmony was not passed in");
        var applied = 0;

        var legacy = AccessTools.Method(type, "GetLegacy");
        if (legacy is not null)
        {
            harmony.Patch(
                legacy,
                postfix: new HarmonyMethod(typeof(HwidPatch), nameof(AfterGetLegacy)));
            applied++;
        }

        var modern = AccessTools.Method(type, "GetModern");
        if (modern is not null)
        {
            harmony.Patch(
                modern,
                postfix: new HarmonyMethod(typeof(HwidPatch), nameof(AfterGetModern)));
            applied++;
        }

        if (applied > 0)
            _patchedTypes.Add(typeName);
        return applied;
    }

    private static void AfterGetLegacy(ref byte[] __result)
    {
        var value = Legacy();
        if (value is not null)
            __result = value;
    }

    private static void AfterGetModern(ref byte[] __result)
    {
        var value = Modern();
        if (value is not null)
            __result = value;
    }

    private static byte[]? Legacy()
    {
        _legacy ??= ReadValue(LegacyEnv, 32);
        return _legacy;
    }

    private static byte[]? Modern()
    {
        if (_modern is not null)
            return _modern;

        _modern = ReadValue(ModernEnv, 33);
        if (_modern is null)
        {
            // The launcher normally sets both; one variable still has to be enough for a
            // consistent pair.
            var legacy = Legacy();
            if (legacy is not null)
            {
                byte[] modern = [0, .. legacy];
                _modern = modern;
            }
        }
        return _modern;
    }

    /// <summary>Hex of the expected length, or <c>null</c> when the variable is absent or bad.</summary>
    private static byte[]? ReadValue(string name, int expectedBytes)
    {
        var text = Environment.GetEnvironmentVariable(name);
        if (string.IsNullOrWhiteSpace(text))
            return null;
        var clean = new StringBuilder(text.Length);
        foreach (var c in text)
        {
            if (char.IsAsciiHexDigit(c))
                clean.Append(c);
        }
        if (clean.Length != expectedBytes * 2)
            return null;

        var bytes = new byte[expectedBytes];
        for (var i = 0; i < expectedBytes; i++)
            bytes[i] = Convert.ToByte(clean.ToString(i * 2, 2), 16);
        return bytes;
    }
}
