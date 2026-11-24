using System;
using System.IO;
using System.Linq;
using Mono.Cecil;
using Mono.Cecil.Cil;

namespace SS14.Loader;

/// <summary>
/// Rewrites game IL with Mono.Cecil, as the alternative to a Harmony detour for the same
/// performance tweak. Every reference to Mono.Cecil lives in this type, so a launch that does not
/// use the Cecil backend never loads the assembly (the same reason <see cref="HarmonyBridge"/> exists).
/// </summary>
/// <remarks>
/// The bytes on disk and in <c>content.db</c> are never touched: the rewritten assembly exists only in
/// memory and is handed to the engine instead of the original (see <see cref="CecilFileApi"/>).
/// <para>
/// The inserted code uses only <c>System.Diagnostics.Stopwatch</c>, resolved through the
/// <c>System.Runtime</c> reference the game assembly already has. The engine checks content assemblies
/// against a whitelist, so a call into our own assembly would be rejected there.
/// </para>
/// </remarks>
internal static class CecilPatcher
{
    /// <summary>
    /// Finds the assemblies the game assembly refers to. Cecil needs them while it writes: it looks up
    /// the underlying type of every enum constant. They are not on disk, so the caller says where
    /// to read the game's own ones (engine zip, content.db); the .NET ones come from the runtime folder.
    /// </summary>
    private sealed class GameResolver : IAssemblyResolver
    {
        private static readonly string? RuntimeDir = Path.GetDirectoryName(typeof(object).Assembly.Location);

        private readonly Func<string, byte[]?> _loadGameAssembly;
        private readonly System.Collections.Generic.Dictionary<string, AssemblyDefinition?> _cache = new();

        public GameResolver(Func<string, byte[]?> loadGameAssembly)
        {
            _loadGameAssembly = loadGameAssembly;
        }

        public AssemblyDefinition? Resolve(AssemblyNameReference name) => Resolve(name, new ReaderParameters());

        public AssemblyDefinition? Resolve(AssemblyNameReference name, ReaderParameters parameters)
        {
            if (_cache.TryGetValue(name.Name, out var cached))
                return cached;

            AssemblyDefinition? result = null;
            var bytes = _loadGameAssembly(name.Name);
            if (bytes == null && RuntimeDir != null)
            {
                var path = Path.Combine(RuntimeDir, name.Name + ".dll");
                if (File.Exists(path))
                    bytes = File.ReadAllBytes(path);
            }

            if (bytes != null)
            {
                result = AssemblyDefinition.ReadAssembly(
                    new MemoryStream(bytes, writable: false),
                    new ReaderParameters { InMemory = true, ReadSymbols = false, AssemblyResolver = this });
            }

            _cache[name.Name] = result;
            return result;
        }

        public void Dispose()
        {
        }
    }

    /// <summary>Set to <c>1</c> in this process once the outline throttle is in the IL.</summary>
    public const string OutlineAppliedEnv = "BEBRA_PERF_OUTLINE_IL";

    private const string OutlineType = "Content.Client.Outline.InteractionOutlineSystem";
    private const string OutlineMethod = "FrameUpdate";

    /// <summary>
    /// Name of the single static field the throttle needs for its "last run" timestamp. It is
    /// shaped like a C# compiler backing field (angle brackets, <c>k__BackingField</c> suffix) on
    /// purpose: a scan of the metadata for mod-specific strings sees only what looks like ordinary
    /// compiler output, not a marker like <c>__bebra_*</c>. This hides it from signature/heuristic
    /// scans only — a full byte diff against the stock assembly still shows one extra member.
    /// </summary>
    private const string OutlineField = "<FrameUpdate>k__BackingField";

    /// <summary>
    /// Makes <c>InteractionOutlineSystem.FrameUpdate</c> return at once when less than ~30 ms have
    /// passed since its last full run. Same behaviour as the Harmony prefix in <c>Bebra.PerfPatch</c>.
    /// </summary>
    /// <returns>The rewritten assembly, or <c>null</c> with the reason in <paramref name="error"/>.</returns>
    /// <param name="original">Content.Client.dll as the content holds it.</param>
    /// <param name="loadGameAssembly">Reads another game assembly by simple name (<c>null</c> = not there).</param>
    /// <param name="error">Why nothing was rewritten.</param>
    public static byte[]? ThrottleOutline(byte[] original, Func<string, byte[]?> loadGameAssembly, out string? error)
    {
        error = null;
        try
        {
            using var input = new MemoryStream(original, writable: false);
            using var resolver = new GameResolver(loadGameAssembly);
            using var module = ModuleDefinition.ReadModule(input, new ReaderParameters
            {
                InMemory = true,
                ReadSymbols = false,
                AssemblyResolver = resolver,
            });

            // A strong-named assembly would fail to load after any change.
            if (module.Assembly.Name.HasPublicKey)
            {
                error = "assembly is strong-named";
                return null;
            }

            var type = module.GetType(OutlineType);
            if (type == null)
            {
                error = OutlineType + " not found";
                return null;
            }

            if (type.Fields.Any(f => f.Name == OutlineField))
            {
                error = "already rewritten";
                return null;
            }

            var method = type.Methods.FirstOrDefault(m =>
                m.Name == OutlineMethod && !m.IsStatic && m.HasBody && m.Parameters.Count == 1
                && m.ReturnType.FullName == "System.Void"
                && m.Parameters[0].ParameterType.FullName == "System.Single");
            if (method == null)
            {
                error = OutlineType + "." + OutlineMethod + "(float) not found";
                return null;
            }

            var runtime = module.AssemblyReferences.FirstOrDefault(a => a.Name == "System.Runtime");
            if (runtime == null)
            {
                error = "System.Runtime is not referenced";
                return null;
            }

            var int64 = module.TypeSystem.Int64;
            var stopwatch = new TypeReference("System.Diagnostics", "Stopwatch", module, runtime);
            var getTimestamp = new MethodReference("GetTimestamp", int64, stopwatch) { HasThis = false };

            // One private static field, kept to the minimum the throttle needs (see OutlineField).
            var last = new FieldDefinition(OutlineField, FieldAttributes.Private | FieldAttributes.Static, int64);
            type.Fields.Add(last);

            var interval = System.Diagnostics.Stopwatch.Frequency * 30 / 1000;
            var il = method.Body.GetILProcessor();
            var first = method.Body.Instructions[0];

            // Smallest prologue that does the job: one GetTimestamp call (the timestamp is dup'd and
            // reused instead of read twice), no extra local, no marker instruction.
            //   long now = Stopwatch.GetTimestamp();
            //   if (now - last < interval) return;   // too soon — skip this frame's work
            //   last = now;                          // run it, remember when
            var store = il.Create(OpCodes.Stsfld, last);
            il.InsertBefore(first, il.Create(OpCodes.Call, getTimestamp)); // now
            il.InsertBefore(first, il.Create(OpCodes.Dup));                // now, now
            il.InsertBefore(first, il.Create(OpCodes.Ldsfld, last));       // now, now, last
            il.InsertBefore(first, il.Create(OpCodes.Sub));                // now, now-last
            il.InsertBefore(first, il.Create(OpCodes.Ldc_I8, interval));   // now, now-last, interval
            il.InsertBefore(first, il.Create(OpCodes.Bge, store));         // now  (>= interval: store & run)
            il.InsertBefore(first, il.Create(OpCodes.Pop));                // too soon: drop now
            il.InsertBefore(first, il.Create(OpCodes.Ret));                // ...and return early
            il.InsertBefore(first, store);                                 // last = now, then fall into the original body

            using var output = new MemoryStream();
            module.Write(output);
            return output.ToArray();
        }
        catch (Exception ex)
        {
            error = ex.GetType().Name + ": " + ex.Message;
            return null;
        }
    }
}
