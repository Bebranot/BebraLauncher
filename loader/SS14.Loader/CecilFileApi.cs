using System;
using System.Collections.Generic;
using System.Diagnostics.CodeAnalysis;
using System.IO;
using Robust.LoaderApi;

namespace SS14.Loader;

/// <summary>
/// Wraps the content file API and hands the engine a rewritten <c>Content.Client.dll</c> in place of
/// the original. Nothing is written anywhere: the original stays in <c>content.db</c> untouched.
/// </summary>
/// <remarks>
/// The engine may open an assembly more than once (the type checker, then the loader), so the result
/// is cached and every call gets a fresh stream. A failed rewrite is also cached: the game then runs
/// the original code, and <c>Bebra.PerfPatch</c> sees that the marker is missing and falls back to Harmony.
/// </remarks>
internal sealed class CecilFileApi : IFileApi
{
    private const string TargetFile = "Assemblies/Content.Client.dll";
    private const string TargetSymbols = "Assemblies/Content.Client.pdb";

    private readonly IFileApi _inner;
    private readonly IFileApi _engine;
    private readonly object _lock = new();
    private byte[]? _patched;
    private bool _tried;

    /// <param name="inner">The content files (<c>content.db</c>).</param>
    /// <param name="engine">The engine files (Robust.*.dll); Cecil reads them to resolve references.</param>
    public CecilFileApi(IFileApi inner, IFileApi engine)
    {
        _inner = inner;
        _engine = engine;
    }

    /// <summary>The bytes of a game assembly by simple name: engine first, then the content.</summary>
    private byte[]? LoadGameAssembly(string name)
    {
        foreach (var (api, path) in new[] { (_engine, name + ".dll"), (_inner, "Assemblies/" + name + ".dll") })
        {
            if (!api.TryOpen(path, out var stream))
                continue;

            using (stream)
            {
                using var buffer = new MemoryStream();
                stream.CopyTo(buffer);
                return buffer.ToArray();
            }
        }

        return null;
    }

    public IEnumerable<string> AllFiles => _inner.AllFiles;

    public bool TryOpen(string path, [NotNullWhen(true)] out Stream? stream)
    {
        var normalized = path.Replace('\\', '/').TrimStart('/');

        if (string.Equals(normalized, TargetFile, StringComparison.OrdinalIgnoreCase))
        {
            var patched = GetPatched();
            if (patched != null)
            {
                stream = new MemoryStream(patched, writable: false);
                return true;
            }
        }
        else if (string.Equals(normalized, TargetSymbols, StringComparison.OrdinalIgnoreCase) && _patched != null)
        {
            // The symbols describe the original IL; with the rewritten code they would point at the wrong lines.
            stream = null;
            return false;
        }

        return _inner.TryOpen(path, out stream);
    }

    private byte[]? GetPatched()
    {
        lock (_lock)
        {
            if (_tried)
                return _patched;
            _tried = true;

            if (!_inner.TryOpen(TargetFile, out var source))
            {
                PatchLog.Write("[BebraLoader] Cecil: {0} not found in the content", TargetFile);
                return null;
            }

            byte[] original;
            using (source)
            {
                using var buffer = new MemoryStream();
                source.CopyTo(buffer);
                original = buffer.ToArray();
            }

            var result = CecilPatcher.ThrottleOutline(original, LoadGameAssembly, out var error);
            if (result == null)
            {
                PatchLog.Write("[BebraLoader] Cecil: outline throttle not applied: {0}", error);
                return null;
            }

            _patched = result;
            Environment.SetEnvironmentVariable(CecilPatcher.OutlineAppliedEnv, "1");
            PatchLog.Write("[BebraLoader] Cecil: outline throttle written into Content.Client in memory ({0} -> {1} bytes)",
                original.Length, result.Length);
            return result;
        }
    }
}
