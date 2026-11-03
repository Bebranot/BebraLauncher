using System;
using Content.Client.UserInterface.Controls;
using Content.Client.Viewport;
using HarmonyLib;
using Robust.Shared.Maths;

namespace BebraWare;

/// <summary>
/// Клиентское улучшение отображения: убирает пустые поля вокруг игрового мира.
/// Патч не меняет сетевое состояние, игровую физику и серверную логику.
/// </summary>
[HarmonyPatch(typeof(ScalingViewport), "RegenerateViewport")]
internal static class ScalingViewportPatch
{
    /// <summary>
    /// Перед оригинальным пересозданием вьюпорта подбираем ширину под реальные пропорции окна.
    /// </summary>
    [HarmonyPrefix]
    private static void Prefix(ScalingViewport __instance)
    {
        var controlSize = __instance.PixelSize;
        var viewportSize = __instance.ViewportSize;

        // На старте UI может иметь нулевой размер. В этом случае оригинальный код
        // должен завершить инициализацию сам, поэтому ничего не меняем.
        if (controlSize.X <= 0 || controlSize.Y <= 0 || viewportSize.Y <= 0)
            return;

        var width = (int)MathF.Round(viewportSize.Y * (float)controlSize.X / controlSize.Y);
        if (width <= 0 || width == viewportSize.X)
            return;

        __instance.ViewportSize = new Vector2i(width, viewportSize.Y);
    }

    /// <summary>
    /// После оригинала повторно сохраняем рассчитанную ширину: некоторые версии клиента
    /// пересчитывают размер внутри метода и возвращают фиксированное соотношение сторон.
    /// </summary>
    [HarmonyPostfix]
    private static void Postfix(ScalingViewport __instance)
    {
        var controlSize = __instance.PixelSize;
        var viewportSize = __instance.ViewportSize;
        if (controlSize.X <= 0 || controlSize.Y <= 0 || viewportSize.Y <= 0)
            return;

        var width = (int)MathF.Round(viewportSize.Y * (float)controlSize.X / controlSize.Y);
        if (width > 0 && width != viewportSize.X)
            __instance.ViewportSize = new Vector2i(width, viewportSize.Y);
    }
}

/// <summary>
/// Отключает снап к целому масштабу, который снова создаёт поля на широких окнах.
/// </summary>
[HarmonyPatch(typeof(MainViewport), "CalcSnappingFactor")]
internal static class MainViewportPatch
{
    /// <summary>
    /// Возвращаем null вместо целого множителя: клиент растягивает изображение равномерно
    /// по рассчитанному вьюпорту, без растяжения по одной оси и без обрезки мира.
    /// </summary>
    [HarmonyPrefix]
    private static bool Prefix(ref int? __result)
    {
        __result = null;
        return false;
    }
}
