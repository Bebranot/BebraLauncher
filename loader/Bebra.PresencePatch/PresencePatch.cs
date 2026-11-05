using System;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Reflection;
using HarmonyLib;

namespace Bebra.PresencePatch;

/// <summary>
/// Tells the launcher what only the client knows, for the Discord presence the launcher keeps: the
/// name of the entity the player controls and the Discord images the server asks for. Once a second
/// these are compared with the last ones; a change replaces the file named by
/// <c>BEBRA_PRESENCE_FILE</c>, one value per line in that order. A line is empty while the value is
/// unknown (no entity in the lobby, no server in the main menu). Without the variable the patch
/// does nothing.
/// </summary>
/// <remarks>
/// The engine types are resolved by name, as in <c>Bebra.HwidPatch</c>: a compile-time reference
/// would tie the patch to one build of the game. This is only a convenience, so anything
/// unexpected at run time switches it off instead of disturbing the game.
/// </remarks>
public static class PresencePatch
{
    private const string FileEnv = "BEBRA_PRESENCE_FILE";
    private const string MainIconCVar = "discord.rich_main_icon_id";
    private const string SecondIconCVar = "discord.rich_second_icon_id";

    private static readonly long Interval = Stopwatch.Frequency;

    /// <summary>The Harmony instance, injected by the loader's <c>InitPatchAssembly</c>.</summary>
    public static Harmony? Harmony { get; set; }

    private static bool _patched;
    private static string? _path;
    private static long _lastCheck;
    private static string _written = "";

    // Found on the first frame: the game's objects exist only then. Read from the main thread only.
    private static bool _resolved;
    private static bool _broken;
    private static object? _players;
    private static PropertyInfo? _localEntity;
    private static PropertyInfo? _localSession;
    private static object? _config;
    private static MethodInfo? _getCVar;
    private static object? _entities;
    private static MethodInfo? _tryGetComponent;
    private static Type? _metaData;
    private static PropertyInfo? _entityName;

    /// <summary>Called by the loader once Content.Client is loaded.</summary>
    public static void ContentInit()
    {
        var harmony = Harmony ?? throw new InvalidOperationException("Harmony was not passed in");
        if (_patched)
            return;
        _path = Environment.GetEnvironmentVariable(FileEnv);
        if (string.IsNullOrEmpty(_path))
            return;

        // The loader retries a few times and logs the message when it gives up.
        var update = AccessTools.Method("Robust.Client.GameController:Update")
            ?? throw new InvalidOperationException("not found: GameController.Update");
        harmony.Patch(update, postfix: new HarmonyMethod(typeof(PresencePatch), nameof(AfterUpdate)));
        _patched = true;
    }

    private static void AfterUpdate()
    {
        if (_broken)
            return;
        var now = Stopwatch.GetTimestamp();
        if (now - _lastCheck < Interval)
            return;
        _lastCheck = now;

        try
        {
            if (!_resolved)
                Resolve();
            var text = string.Join("\n", Line(ReadName()), Line(ReadIcon(MainIconCVar)), Line(ReadIcon(SecondIconCVar)));
            if (text != _written && Write(text))
                _written = text;
        }
        catch (Exception)
        {
            // Another build of the game. The launcher's status then simply has no character.
            _broken = true;
        }
    }

    private static void Resolve()
    {
        var ioc = AccessTools.TypeByName("Robust.Shared.IoC.IoCManager")
            ?? throw new InvalidOperationException("IoCManager");
        var resolve = AccessTools.Method(ioc, "Resolve", Type.EmptyTypes)
            ?? throw new InvalidOperationException("IoCManager.Resolve");

        var playersType = AccessTools.TypeByName("Robust.Shared.Player.ISharedPlayerManager")
            ?? throw new InvalidOperationException("ISharedPlayerManager");
        _players = resolve.MakeGenericMethod(playersType).Invoke(null, null);
        _localEntity = AccessTools.Property(playersType, "LocalEntity")
            ?? throw new InvalidOperationException("LocalEntity");

        var entitiesType = AccessTools.TypeByName("Robust.Shared.GameObjects.IEntityManager")
            ?? throw new InvalidOperationException("IEntityManager");
        _entities = resolve.MakeGenericMethod(entitiesType).Invoke(null, null);

        var uid = AccessTools.TypeByName("Robust.Shared.GameObjects.EntityUid")
            ?? throw new InvalidOperationException("EntityUid");
        var component = AccessTools.TypeByName("Robust.Shared.GameObjects.IComponent")
            ?? throw new InvalidOperationException("IComponent");
        _tryGetComponent = AccessTools.Method(
                entitiesType,
                "TryGetComponent",
                new[] { uid, typeof(Type), component.MakeByRefType() })
            ?? throw new InvalidOperationException("TryGetComponent");

        _metaData = AccessTools.TypeByName("Robust.Shared.GameObjects.MetaDataComponent")
            ?? throw new InvalidOperationException("MetaDataComponent");
        _entityName = AccessTools.Property(_metaData, "EntityName")
            ?? throw new InvalidOperationException("EntityName");

        _localSession = AccessTools.Property(playersType, "LocalSession");
        ResolveConfig(resolve);

        _resolved = true;
    }

    /// <summary>The icons are extras: without them the character's name still works.</summary>
    private static void ResolveConfig(MethodInfo resolve)
    {
        var configType = AccessTools.TypeByName("Robust.Shared.Configuration.IConfigurationManager");
        if (configType is null || _localSession is null)
            return;
        _config = resolve.MakeGenericMethod(configType).Invoke(null, null);
        // A generic overload has the same parameters, so the method cannot be looked up by them.
        _getCVar = configType.GetMethods().FirstOrDefault(m =>
            m is { Name: "GetCVar", IsGenericMethod: false }
            && m.GetParameters() is [{ ParameterType: var type }]
            && type == typeof(string));
    }

    /// <summary>
    /// The value the server replicated. Empty without a server: the engine's default would name an
    /// image no server asked for.
    /// </summary>
    private static string ReadIcon(string cvar)
    {
        if (_getCVar is null || _localSession!.GetValue(_players) is null)
            return "";
        try
        {
            return _getCVar.Invoke(_config, new object[] { cvar }) as string ?? "";
        }
        catch (TargetInvocationException)
        {
            // This build has no such variable: no reason to ask every second.
            _getCVar = null;
            return "";
        }
    }

    /// <summary>A value on one line: the line breaks of the file separate the values.</summary>
    private static string Line(string value) =>
        value.Replace('\r', ' ').Replace('\n', ' ').Trim();

    /// <summary>The controlled entity's name; empty when there is none.</summary>
    private static string ReadName()
    {
        var entity = _localEntity!.GetValue(_players);
        if (entity is null)
            return "";

        var args = new[] { entity, _metaData, null };
        if (_tryGetComponent!.Invoke(_entities, args) is not true || args[2] is null)
            return "";
        return (_entityName!.GetValue(args[2]) as string)?.Trim() ?? "";
    }

    /// <summary>Whole file or nothing. A failure (the launcher is reading) is retried next time.</summary>
    private static bool Write(string text)
    {
        var temp = _path + ".tmp";
        try
        {
            File.WriteAllText(temp, text);
            File.Move(temp, _path!, overwrite: true);
            return true;
        }
        catch (IOException)
        {
            return false;
        }
        catch (UnauthorizedAccessException)
        {
            return false;
        }
    }
}
