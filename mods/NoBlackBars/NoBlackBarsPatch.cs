using System;
using Content.Client.UserInterface.Controls;
using Content.Client.Viewport;
using HarmonyLib;
using Robust.Shared.Maths;

namespace NoBlackBars;

/// <summary>
/// Расширяет внутренний вьюпорт под пропорции окна, чтобы не оставалось чёрных полос.
/// </summary>
/// <remarks>
/// <see cref="ScalingViewport"/> рисует мир в прямоугольник фиксированных пропорций
/// (в SS220 это 21×15 тайлов). На 16:9 такая картинка уже окна, поэтому по бокам
/// остаются поля. Патч перед созданием вьюпорта подбирает его ширину из высоты и
/// пропорций контрола: видно больше мира по бокам, а масштаб остаётся равномерным,
/// без растяжения и обрезки.
/// </remarks>
[HarmonyPatch(typeof(ScalingViewport), "RegenerateViewport")]
internal static class ScalingViewportRegeneratePatch
{
    [HarmonyPrefix]
    private static void Prefix(ScalingViewport __instance)
    {
        var controlSize = __instance.PixelSize;
        var viewportSize = __instance.ViewportSize;

        // На старте контрол может быть ещё нулевого размера: оставляем как есть,
        // оригинал создаст вьюпорт с текущими значениями.
        if (controlSize.X <= 0 || controlSize.Y <= 0 || viewportSize.Y <= 0)
            return;

        // Высота вьюпорта задаёт вертикальный обзор (15 тайлов), ширину подгоняем
        // под окно. Округление до пикселя даёт расхождение меньше пикселя.
        var width = (int) MathF.Round(viewportSize.Y * (float) controlSize.X / controlSize.Y);
        if (width <= 0 || width == viewportSize.X)
            return;

        __instance.ViewportSize = new Vector2i(width, viewportSize.Y);
    }
}

/// <summary>
/// Отключает снап вьюпорта к целому масштабу.
/// </summary>
/// <remarks>
/// <see cref="MainViewport.CalcSnappingFactor"/> подбирает целый множитель и включает
/// режим <c>FixedStretchSize</c>: картинка рисуется в размер «тайлы × множитель» и
/// центрируется, из-за чего поля возвращаются даже при подобранной ширине вьюпорта.
/// Возвращаем <c>null</c>, чтобы всегда работал режим растяжения по размеру контрола.
/// </remarks>
[HarmonyPatch(typeof(MainViewport), "CalcSnappingFactor")]
internal static class MainViewportCalcSnappingFactorPatch
{
    [HarmonyPrefix]
    private static bool Prefix(ref int? __result)
    {
        __result = null;
        return false;
    }
}
