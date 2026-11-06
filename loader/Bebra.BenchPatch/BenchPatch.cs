using System;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Numerics;
using System.Reflection;
using System.Runtime;
using System.Text.Json;
using System.Threading;
using HarmonyLib;

namespace Bebra.BenchPatch;

/// <summary>
/// Developer tool, never shipped to players: measures a client run and writes one JSON report, so
/// the launcher's optimization profiles can be compared with numbers (see <c>scripts/bench/</c>).
/// It is only loaded when passed with <c>bebra launch --patch</c>.
/// </summary>
/// <remarks>
/// Besides frame times, the report records what the running client really uses (CVar values, GC
/// mode, process and thread priority, applied Harmony patches). That is the proof a profile took
/// effect: a launch argument the game ignored shows up here as an old value.
///
/// Settings come from the environment:
/// <list type="bullet">
/// <item><c>BEBRA_BENCH_OUT</c>: report path. Without it the patch does nothing.</item>
/// <item><c>BEBRA_BENCH_WARMUP</c>, <c>BEBRA_BENCH_SECONDS</c>: seconds in game before measuring
/// (the JIT and the caches settle) and seconds measured.</item>
/// <item><c>BEBRA_BENCH_LOBBY</c>: ';'-separated console commands run once in the lobby.</item>
/// <item><c>BEBRA_BENCH_SETUP</c>: the same, run once when the game view opens.</item>
/// <item><c>BEBRA_BENCH_TOUR</c>: ';'-separated commands repeated in a circle, one per
/// <c>BEBRA_BENCH_TOUR_STEP</c> seconds, during warm-up and measuring.</item>
/// <item><c>BEBRA_BENCH_CVARS</c>: ','-separated CVars to report.</item>
/// <item><c>BEBRA_BENCH_TAG</c>: copied to the report.</item>
/// </list>
/// The engine types are resolved by name, as in the other patches.
/// </remarks>
public static class BenchPatch
{
    /// <summary>The Harmony instance, injected by the loader's <c>InitPatchAssembly</c>.</summary>
    public static Harmony? Harmony { get; set; }

    private const int MaxFrames = 1 << 20;
    private static readonly float[] Intervals = new float[MaxFrames];
    private static readonly float[] RenderTimes = new float[MaxFrames];
    private static readonly float[] UpdateTimes = new float[MaxFrames];
    private static readonly float[] TickTimes = new float[MaxFrames];
    private static readonly double MsPerTick = 1000.0 / Stopwatch.Frequency;

    private enum Phase { WaitGame, Calibrate, Warmup, Measure, Done }

    private static Phase _phase = Phase.WaitGame;
    private static bool _installed;
    private static string _out = "";
    private static double _warmup = 90;
    private static double _seconds = 60;
    private static double _tourStep = 5;
    private static string[] _tour = [];
    private static string[] _setup = [];
    private static string[] _lobby = [];
    private static string[] _anchors = [];
    private static string[] _cvarNames = [];
    private static string _tag = "";

    private static int _frames;
    private static long _lastRenderStart;
    private static long _renderStart;
    private static long _updateStart;
    private static long _tickStart;
    private static long _updateAccum;
    private static long _tickAccum;
    private static long _nextPoll;
    private static long _phaseStart;
    private static long _nextTour;
    private static int _tourIndex;
    private static bool _lobbyDone;
    private static bool _setupDone;
    private static double _secondsToGame = -1;
    private static double _secondsToLobby = -1;
    private static string _lastState = "";

    // Grid-local to map coordinates, measured from two warp points: stations load at a random
    // rotation, so "tp x y" means another place every round.
    private static int _calibrationStep;
    private static long _nextCalibration;
    private static Vector2 _firstAnchor;
    private static Vector2 _gridOrigin;
    private static double _gridAngle;
    private static string _gridMap = "";
    private static object? _eye;

    private static int _gen0;
    private static int _gen1;
    private static int _gen2;
    private static TimeSpan _pause;
    private static long _alloc;
    private static TimeSpan _cpu;
    private static long _peakWorkingSet;
    private static long _peakPrivate;
    private static long _peakManaged;

    private static object? _console;
    private static MethodInfo? _execute;
    private static object? _states;
    private static PropertyInfo? _currentState;
    private static object? _cfg;
    private static MethodInfo? _getCVar;
    private static int _mainThreadId;
    private static ThreadPriority _mainPriority;

    /// <summary>Called by the loader once Content.Client is loaded.</summary>
    public static void ContentInit()
    {
        if (_installed)
            return;
        var path = Environment.GetEnvironmentVariable("BEBRA_BENCH_OUT");
        if (string.IsNullOrEmpty(path))
            return;

        var harmony = Harmony ?? throw new InvalidOperationException("Harmony was not passed in");
        _out = path;
        _warmup = Number("BEBRA_BENCH_WARMUP", _warmup);
        _seconds = Number("BEBRA_BENCH_SECONDS", _seconds);
        _tourStep = Number("BEBRA_BENCH_TOUR_STEP", _tourStep);
        _tour = List("BEBRA_BENCH_TOUR", ';');
        _setup = List("BEBRA_BENCH_SETUP", ';');
        _lobby = List("BEBRA_BENCH_LOBBY", ';');
        _anchors = List("BEBRA_BENCH_ANCHORS", '|');
        _cvarNames = List("BEBRA_BENCH_CVARS", ',');
        _tag = Environment.GetEnvironmentVariable("BEBRA_BENCH_TAG") ?? "";

        Wrap(harmony, "Robust.Client.GameController:Update", nameof(BeforeUpdate), nameof(AfterUpdate));
        Wrap(harmony, "Robust.Client.GameController:Tick", nameof(BeforeTick), nameof(AfterTick));
        Wrap(harmony, "Robust.Client.Graphics.Clyde.Clyde:Render", nameof(BeforeRender), nameof(AfterRender));
        _installed = true;
    }

    private static void Wrap(Harmony harmony, string target, string before, string after)
    {
        try
        {
            var method = AccessTools.Method(target) ?? throw new InvalidOperationException("not found");
            harmony.Patch(
                method,
                prefix: new HarmonyMethod(typeof(BenchPatch), before),
                postfix: new HarmonyMethod(typeof(BenchPatch), after));
        }
        catch (Exception e)
        {
            Console.WriteLine("[BebraBench] cannot wrap {0}: {1}", target, e);
            throw;
        }
    }

    private static double Number(string name, double fallback) =>
        double.TryParse(
            Environment.GetEnvironmentVariable(name),
            System.Globalization.NumberStyles.Float,
            System.Globalization.CultureInfo.InvariantCulture,
            out var value)
            ? value
            : fallback;

    private static string[] List(string name, char separator) =>
        (Environment.GetEnvironmentVariable(name) ?? "")
        .Split(separator, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);

    private static void BeforeTick() => _tickStart = Stopwatch.GetTimestamp();

    private static void AfterTick() => _tickAccum += Stopwatch.GetTimestamp() - _tickStart;

    private static void BeforeUpdate() => _updateStart = Stopwatch.GetTimestamp();

    private static void AfterUpdate()
    {
        var now = Stopwatch.GetTimestamp();
        _updateAccum += now - _updateStart;
        if (now >= _nextPoll)
        {
            _nextPoll = now + Stopwatch.Frequency / 4;
            Poll(now);
        }
    }

    private static void BeforeRender() => _renderStart = Stopwatch.GetTimestamp();

    private static void AfterRender()
    {
        var now = Stopwatch.GetTimestamp();
        if (_phase == Phase.Measure && _lastRenderStart != 0 && _frames < MaxFrames)
        {
            Intervals[_frames] = (float)((_renderStart - _lastRenderStart) * MsPerTick);
            RenderTimes[_frames] = (float)((now - _renderStart) * MsPerTick);
            UpdateTimes[_frames] = (float)(_updateAccum * MsPerTick);
            TickTimes[_frames] = (float)(_tickAccum * MsPerTick);
            _frames++;
        }
        _lastRenderStart = _renderStart;
        _updateAccum = 0;
        _tickAccum = 0;
    }

    private static void Poll(long now)
    {
        try
        {
            ResolveServices();
            var state = _currentState?.GetValue(_states)?.GetType().Name ?? "";
            if (state != _lastState)
            {
                _lastState = state;
                Console.WriteLine("[BebraBench] state: {0} after {1:F1}s", state, SinceStart());
            }

            if (state == "LobbyState")
            {
                if (_secondsToLobby < 0)
                    _secondsToLobby = SinceStart();
                if (!_lobbyDone)
                {
                    _lobbyDone = true;
                    Run(_lobby);
                }
            }

            switch (_phase)
            {
                case Phase.WaitGame:
                    if (state != "GameplayState")
                        break;
                    _secondsToGame = SinceStart();
                    _phase = _anchors.Length == 6 ? Phase.Calibrate : Phase.Warmup;
                    if (_phase == Phase.Warmup)
                        StartWarmup(now);
                    break;

                case Phase.Calibrate:
                    if (now < _nextCalibration)
                        break;
                    _nextCalibration = now + 3 * Stopwatch.Frequency;
                    if (Calibrate())
                        StartWarmup(now);
                    break;

                case Phase.Warmup:
                    Tour(now);
                    if (Elapsed(now) < _warmup)
                        break;
                    _phase = Phase.Measure;
                    _phaseStart = now;
                    BeginMeasure();
                    break;

                case Phase.Measure:
                    Tour(now);
                    Sample();
                    if (Elapsed(now) < _seconds)
                        break;
                    _phase = Phase.Done;
                    Finish(Elapsed(now));
                    break;
            }
        }
        catch (Exception e)
        {
            Console.WriteLine("[BebraBench] {0}", e);
        }
    }

    private static void StartWarmup(long now)
    {
        if (!_setupDone)
        {
            _setupDone = true;
            Run(_setup);
        }
        _phase = Phase.Warmup;
        _phaseStart = now;
        _nextTour = now;
    }

    /// <summary>One step every few seconds: become a ghost, then visit both warp points.</summary>
    private static bool Calibrate()
    {
        switch (_calibrationStep++)
        {
            case 0:
                Execute("ghost");
                return false;
            case 1:
                Execute($"warp \"{_anchors[0]}\"");
                return false;
            case 2:
                _firstAnchor = EyePosition(out _gridMap);
                Execute($"warp \"{_anchors[3]}\"");
                return false;
            default:
                var second = EyePosition(out _);
                var localFirst = new Vector2(Coordinate(_anchors[1]), Coordinate(_anchors[2]));
                var localSecond = new Vector2(Coordinate(_anchors[4]), Coordinate(_anchors[5]));
                var world = second - _firstAnchor;
                var local = localSecond - localFirst;
                _gridAngle = Math.Atan2(world.Y, world.X) - Math.Atan2(local.Y, local.X);
                _gridOrigin = _firstAnchor - Rotate(localFirst, _gridAngle);
                Console.WriteLine("[BebraBench] grid: map {0}, {1:F1} deg, origin {2}, scale check {3:F3}",
                    _gridMap, _gridAngle * 180 / Math.PI, _gridOrigin, world.Length() / local.Length());
                return true;
        }
    }

    private static float Coordinate(string text) => float.Parse(text, CultureInfo.InvariantCulture);

    private static Vector2 Rotate(Vector2 v, double angle) => new(
        (float)(v.X * Math.Cos(angle) - v.Y * Math.Sin(angle)),
        (float)(v.X * Math.Sin(angle) + v.Y * Math.Cos(angle)));

    private static Vector2 EyePosition(out string map)
    {
        var eye = AccessTools.Property(_eye!.GetType(), "CurrentEye").GetValue(_eye)!;
        var coordinates = AccessTools.Property(eye.GetType(), "Position").GetValue(eye)!;
        var type = coordinates.GetType();
        map = AccessTools.Field(type, "MapId").GetValue(coordinates)!.ToString()!;
        return (Vector2)AccessTools.Field(type, "Position").GetValue(coordinates)!;
    }

    private static double Elapsed(long now) => (now - _phaseStart) * (double)MsPerTick / 1000.0;

    private static double SinceStart() =>
        (DateTime.Now - Process.GetCurrentProcess().StartTime).TotalSeconds;

    private static void Tour(long now)
    {
        if (_tour.Length == 0 || now < _nextTour)
            return;
        _nextTour = now + (long)(_tourStep * Stopwatch.Frequency);
        Execute(_tour[_tourIndex++ % _tour.Length]);
    }

    private static void Run(string[] commands)
    {
        foreach (var command in commands)
            Execute(command);
    }

    private static void Execute(string command)
    {
        // "@x y" is a teleport in the station grid's own coordinates.
        if (command.StartsWith('@'))
        {
            var parts = command[1..].Split(' ', StringSplitOptions.RemoveEmptyEntries);
            var target = _gridOrigin + Rotate(new Vector2(Coordinate(parts[0]), Coordinate(parts[1])), _gridAngle);
            // Whole numbers: the command parses floats with the player's culture, which rejects "1.5" in ru-RU.
            command = $"tp {MathF.Round(target.X)} {MathF.Round(target.Y)} {_gridMap}";
        }
        Console.WriteLine("[BebraBench] > {0}", command);
        _execute?.Invoke(_console, [command]);
    }

    private static void ResolveServices()
    {
        if (_console is not null)
            return;
        var ioc = AccessTools.TypeByName("Robust.Shared.IoC.IoCManager");
        var resolve = AccessTools.Method(ioc, "ResolveType", [typeof(Type)]);
        object Service(string name) => resolve.Invoke(null, [AccessTools.TypeByName(name)])!;

        _console = Service("Robust.Client.Console.IClientConsoleHost");
        _execute = AccessTools.Method(
            AccessTools.TypeByName("Robust.Shared.Console.IConsoleHost"), "ExecuteCommand", [typeof(string)]);
        _eye = Service("Robust.Client.Graphics.IEyeManager");
        _states = Service("Robust.Client.State.IStateManager");
        _currentState = AccessTools.Property(AccessTools.TypeByName("Robust.Client.State.IStateManager"), "CurrentState");
        _cfg = Service("Robust.Shared.Configuration.IConfigurationManager");
        _getCVar = AccessTools.TypeByName("Robust.Shared.Configuration.IConfigurationManager")
            .GetMethods()
            .First(m => m.Name == "GetCVar" && !m.IsGenericMethod && m.GetParameters().Length == 1
                && m.GetParameters()[0].ParameterType == typeof(string));

        _mainThreadId = Environment.CurrentManagedThreadId;
        _mainPriority = Thread.CurrentThread.Priority;
    }

    private static void BeginMeasure()
    {
        _frames = 0;
        _lastRenderStart = 0;
        _gen0 = GC.CollectionCount(0);
        _gen1 = GC.CollectionCount(1);
        _gen2 = GC.CollectionCount(2);
        _pause = GC.GetTotalPauseDuration();
        _alloc = GC.GetTotalAllocatedBytes();
        using var process = Process.GetCurrentProcess();
        _cpu = process.TotalProcessorTime;
    }

    private static void Sample()
    {
        using var process = Process.GetCurrentProcess();
        _peakWorkingSet = Math.Max(_peakWorkingSet, process.WorkingSet64);
        _peakPrivate = Math.Max(_peakPrivate, process.PrivateMemorySize64);
        _peakManaged = Math.Max(_peakManaged, GC.GetTotalMemory(false));
    }

    private static void Finish(double seconds)
    {
        using var process = Process.GetCurrentProcess();
        var count = _frames;
        var report = new Dictionary<string, object?>
        {
            ["tag"] = _tag,
            ["seconds"] = Math.Round(seconds, 2),
            ["frames"] = count,
            ["fps_avg"] = Math.Round(count / seconds, 2),
            ["frame_ms"] = Summary(Intervals, count),
            ["render_ms"] = Summary(RenderTimes, count),
            ["update_ms"] = Summary(UpdateTimes, count),
            ["tick_ms"] = Summary(TickTimes, count),
            ["stutters_over_33ms"] = Over(33.3f, count),
            ["stutters_over_50ms"] = Over(50f, count),
            ["stutters_over_100ms"] = Over(100f, count),
            ["gc_gen0"] = GC.CollectionCount(0) - _gen0,
            ["gc_gen1"] = GC.CollectionCount(1) - _gen1,
            ["gc_gen2"] = GC.CollectionCount(2) - _gen2,
            ["gc_pause_ms"] = Math.Round((GC.GetTotalPauseDuration() - _pause).TotalMilliseconds, 1),
            ["alloc_mb_per_s"] = Math.Round((GC.GetTotalAllocatedBytes() - _alloc) / 1048576.0 / seconds, 2),
            ["cpu_percent_of_one_core"] = Math.Round((process.TotalProcessorTime - _cpu).TotalSeconds / seconds * 100, 1),
            ["working_set_mb"] = _peakWorkingSet / 1048576,
            ["private_mb"] = _peakPrivate / 1048576,
            ["managed_mb"] = _peakManaged / 1048576,
            ["seconds_to_lobby"] = Math.Round(_secondsToLobby, 1),
            ["seconds_to_game"] = Math.Round(_secondsToGame, 1),
            ["runtime"] = RuntimeFacts(process),
            ["cvars"] = CVarValues(),
            ["patched_methods"] = HarmonyLib.Harmony.GetAllPatchedMethods()
                .Select(m => m.DeclaringType?.Name + "." + m.Name)
                .ToArray(),
        };

        var directory = Path.GetDirectoryName(_out);
        if (!string.IsNullOrEmpty(directory))
            Directory.CreateDirectory(directory);
        var options = new JsonSerializerOptions { WriteIndented = true };
        File.WriteAllText(_out, JsonSerializer.Serialize(report, options));
        Console.WriteLine("[BebraBench] report written: {0}", _out);
        Execute("quit");
    }

    private static Dictionary<string, object> RuntimeFacts(Process process) => new()
    {
        ["server_gc"] = GCSettings.IsServerGC,
        ["latency_mode"] = GCSettings.LatencyMode.ToString(),
        ["gc_committed_mb"] = GC.GetGCMemoryInfo().TotalCommittedBytes / 1048576,
        ["process_priority"] = process.PriorityClass.ToString(),
        ["main_thread_priority"] = _mainPriority.ToString(),
        ["main_thread_id"] = _mainThreadId,
        ["logical_cpus_seen"] = Environment.ProcessorCount,
        ["affinity_mask"] = process.ProcessorAffinity.ToInt64(),
        ["threads"] = process.Threads.Count,
        ["env"] = Environment.GetEnvironmentVariables()
            .Cast<DictionaryEntry>()
            .Where(e => e.Key is string k && (k.StartsWith("DOTNET_", StringComparison.OrdinalIgnoreCase)
                || k.StartsWith("BEBRA_PERF", StringComparison.OrdinalIgnoreCase)
                || k == "BEBRA_OPTIMIZE"))
            .ToDictionary(e => (string)e.Key, e => (string?)e.Value ?? ""),
    };

    private static Dictionary<string, string> CVarValues()
    {
        var values = new Dictionary<string, string>();
        foreach (var name in _cvarNames)
        {
            try
            {
                values[name] = Convert.ToString(
                    _getCVar?.Invoke(_cfg, [name]),
                    System.Globalization.CultureInfo.InvariantCulture) ?? "";
            }
            catch (Exception e)
            {
                values[name] = "<" + (e.InnerException ?? e).GetType().Name + ">";
            }
        }
        return values;
    }

    private static int Over(float limit, int count)
    {
        var result = 0;
        for (var i = 0; i < count; i++)
        {
            if (Intervals[i] > limit)
                result++;
        }
        return result;
    }

    private static Dictionary<string, double> Summary(float[] source, int count)
    {
        var sorted = new float[count];
        Array.Copy(source, sorted, count);
        Array.Sort(sorted);
        double At(double q) => count == 0 ? 0 : Math.Round(sorted[Math.Min(count - 1, (int)(q * count))], 2);

        var worst = Math.Max(1, count / 100);
        var worstSum = 0.0;
        for (var i = count - worst; i < count && i >= 0; i++)
            worstSum += sorted[i];

        return new Dictionary<string, double>
        {
            ["avg"] = count == 0 ? 0 : Math.Round(sorted.Sum(v => (double)v) / count, 2),
            ["p50"] = At(0.50),
            ["p90"] = At(0.90),
            ["p95"] = At(0.95),
            ["p99"] = At(0.99),
            ["p999"] = At(0.999),
            ["max"] = count == 0 ? 0 : Math.Round(sorted[count - 1], 2),
            ["worst_1pct_avg"] = count == 0 ? 0 : Math.Round(worstSum / worst, 2),
        };
    }
}
