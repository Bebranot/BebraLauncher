using System;

namespace SS14.Loader;

/// <summary>
/// Output for the client patch subsystem.
/// </summary>
/// <remarks>
/// Silent by default: a patched client must not advertise itself in any local output. Set
/// <c>BEBRA_PATCH_LOG=1</c> (or <c>true</c>) only while debugging a patch.
/// </remarks>
internal static class PatchLog
{
    private static readonly bool Enabled = IsEnabled();

    public static void Write(string message)
    {
        if (Enabled)
            Console.WriteLine(message);
    }

    public static void Write(string format, params object?[] args)
    {
        if (Enabled)
            Console.WriteLine(format, args);
    }

    private static bool IsEnabled()
    {
        var value = Environment.GetEnvironmentVariable("BEBRA_PATCH_LOG");
        return value == "1" || string.Equals(value, "true", StringComparison.OrdinalIgnoreCase);
    }
}
