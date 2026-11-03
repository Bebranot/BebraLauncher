# BebraWare

Клиентский Harmony-мод для Space Station 14 (SS220/ADT-форк).

## Что делает

Два визуальных патча, без изменения серверной логики:

- `ScalingViewport.RegenerateViewport` подбирает ширину игрового вьюпорта по пропорциям окна;
- `MainViewport.CalcSnappingFactor` отключает целочисленный снап масштаба.

На широких мониторах пропадают лишние чёрные поля без растяжения по одной оси.

Классы и методы: `Content.Client.Viewport.ScalingViewport.RegenerateViewport` и
`Content.Client.UserInterface.Controls.MainViewport.CalcSnappingFactor`.

## Безопасность

В исходнике только Harmony-атрибуты и операции с размерами UI: нет сети, файловой системы, реестра,
процессов, нативных библиотек, перехвата ввода, скриншотов, токенов или обхода античита.
Старый `CerberusWareV3.dll` из соседней папки не используется: в нём aim assist/ESP, обход античита,
`MinHook` и работа с чувствительными данными.

## Сборка

Нужны: .NET SDK 10, собранный клиент игры и `0Harmony.dll` версии 2.4.3 (`Lib.Harmony`) в кэше NuGet
(`%USERPROFILE%\.nuget\packages\lib.harmony\2.4.3`) или путь в MSBuild-свойстве `HarmonyDir`.

```powershell
.\build.ps1                                   # GameDir по умолчанию: C:\LocalDutyAVS\bin\Content.Client
.\build.ps1 -GameDir "D:\SS14\bin\Content.Client"
```

Скрипт выполняет `dotnet build -c Release`. Игровые DLL и `0Harmony.dll` подключаются только для
компиляции (`Private=false`) и в выходную папку не копируются.

## Установка

```powershell
.\build.ps1 -Install
```

Копирует `BebraWare.dll` в `%LOCALAPPDATA%\BebraLauncher\patches` и обновляет `manifest.json`
(SHA-256 и размер). Затем включите `BebraWare` на вкладке «Моды» лаунчера. Правила загрузки патчей:
[loader/README.md](../../loader/README.md).

## Ограничения

- Патч завязан на имена методов текущего клиента. Если их переименуют, Harmony не применит патч.
- Мод меняет только отображение клиента: нет доступа к серверным данным, правила игры не меняются.
- Запуск игры этим проектом не выполняется.
