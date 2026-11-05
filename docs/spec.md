# Спецификация проекта: VIRShot

> Актуализировано 2026-11-01 по итогам раундов обсуждения (rounds 1–4).
> Обновлено 2026-11-02 (v4): интеграция с геймпадом вынесена из MVP — триггер
> MVP — глобальный клавиатурный хоткей; слот геймпада зарезервирован (§2, §3).
> Эта формулировка больше не действует: геймпад реализован (v5 ниже).
> Обновлено 2026-11-04 (v5, task `2026-11-03_gamepad-integration`): геймпад-триггер
> на Windows реализован — `crates/gamepad` (hand-written COM shim над
> `gameinput.dll`, zero deps) + поток-помпа в virshot, бинд `pad` в конфиге и
> capture «нажмите кнопку» в UI (§2, §3, §4.1, §4.4). Linux (`gilrs`) остаётся
> вне scope (§2, §9); пример `buttons` проверен на железе пользователя
> (2026-11-04, «все ок»); остаток hardware-verification — virshot с pad-биндом
> на Windows-приёмке (§9).
> Разделы, отмеченные **⏳ (ожидает research)**, закрыты по итогам Stages 6-12:
> eframe multi-viewport оверлей, tray-icon (ksni), global-hotkey, xcap —
> реализованы; геймпад-триггер (Windows) реализован (v5), Linux — вне scope.

## 1. Обзор проекта

**VIRShot** — ультралегковесная утилита для создания скриншотов на Windows
(кроссплатформенная сборка: Windows + Linux), позволяющая делать скриншоты
по глобальному клавиатурному хоткею или кнопке геймпада (геймпад — только
Windows; Linux вне scope, §9). Основная цель — заменить тяжелые комбайны вроде ShareX и
ненадежные ремапперы: стабильный, безопасный и незаметный инструмент.

## 2. Цели и ограничения (Scope)

**Входит в MVP:**

- Глобальные клавиатурные хоткеи (system-wide hook): действие по клавиатурной
  комбо; работает, когда фокус у игры.
- Геймпад-триггер (Windows, реализовано 2026-11-04): отдельный поток-помпа
  `gameinput.dll` (crate `crates/gamepad`, §3) — действие по кнопке геймпада из
  поля `pad`; вместе с клавиатурным биндом образует **OR** через общий debounce
  ~250 мс (§4.1). В UI — capture «нажмите кнопку» в поле `pad` (§4.4): первый
  DOWN пишет `unknown:<N>` / `Guide` / `Share`, hint метки кнопки читается из
  device-info блока. Connect/disconnect — INFO (§4.1, §8). Без эмуляции
  клавиатуры. Linux — вне scope (см. ниже, §9).
- Захват экрана: **весь экран** (монитор с foreground-окном) и **окно**
  (захват содержимого окна независимо от z-порядка).
- Оверлей-уведомление (тост): успех и ошибка; ошибки видны всегда.
- Окно настроек (3 вкладки, кнопка Apply).
- Работа в системном трее (скрытый запуск, контекстное меню).
- Конфиг `config.toml` по конвенциям OS, авто-создание дефолта при первом запуске.
- Шаблон пути файла с токенами: `%Y %y %mo %d %H %M %S %iso %t %n %ext`.
- Headless-подкоманда `virshot shot` для тестов.
- Single-instance guard (resident-приложение; lock-file рядом с used-config,
  stale-recovery по pid; `virshot shot` не блокируется — one-shot CLI для
  скриптов/E2E; разные `--config` — параллельные инстансы).

**Не входит в MVP (на будущее):**

- Геймпад на Linux — вне scope: `gilrs` не добавлен. Hardware-находка: wgi-бэкенд
  `gilrs` гейтит чтения по timestamp контроллера, а generic HID-геймпады отдают
  константный timestamp — нажатия не приходят (проверено на железе пользователя,
  pad vendor=1118 product=2835). На Linux/GameInput-нет-хоста поток не
  запускается, поле `pad` в UI — disabled placeholder; геймпад-триггер работает
  только на Windows. См. §3, §4.1, §9.
- Захват области (`rect`) — слоты в конфиге и UI зарезервированы, без реализации.
- Запись видео / Replay Buffer.
- Встроенный редактор изображений.
- Эксклюзивный полный экран — документированное ограничение (см. §9).
- Автозапуск при логине.
- Пользовательская иконка трея (в MVP — встроенная).
- i18n — UI в MVP только на английском (ключи конфига и CLI — английские;
  документация/README — на русском).
- macOS — вне scope.

## 3. Технический стек

- **Язык:** Rust (latest stable, rust version 1.99.1).
- **UI и Оверлей:** `eframe` / `egui` (multi-viewport для окна настроек и
  оверлея). Реализовано (Stage 8-9, перенесено в крейты в Stage 4): тост живёт
  в workspace-крейтах `crates/toast` (правила: `std`-таймер-поток, latest-wins,
  fade-out) и `crates/overlay` (бэкенд `OverlayHost` поверх `egui::Context`
  multi-viewport + чистая геометрия углов `place`). Плашка — дочерний viewport:
  transparency из ROOT-билдера (GL alpha), click-through (`mouse_passthrough`),
  always-on-top, content-protected, показ после первой отрисовки. `eframe` с
  `default-features=false, features=["glow","default_fonts","x11","wayland"]`
  (accesskit-стэки выключены для бюджета размера). Оба крейты — path-deps,
  новых crates.io зависимостей не добавлено (§4.3).
- **Геймпад (Windows — реализовано, v5):** свой crate `crates/gamepad` —
  hand-written COM shim над экспортами `gameinput.dll` (zero deps — `windows`
  crate не добавлен), подключён как path-dep в
  `[target."cfg(windows)".dependencies]`, так что на Linux зависимость не
  pull-ится вообще. Механика — как в PoC (`hidpad-poc/` остаётся историческим
  артефактом): focus policy перед регистрацией, QI-негосиация v3→v2→v0,
  dispatcher `Dispatch(5000µs)` на потоке-помпе + 50 ms опрос кнопок по
  **STATE-diff** (не timestamp-diff); событие библиотеки — маска состояния,
  DOWN-edges (0→1) считает virshot (`bits & !previous`). System-кнопки
  Guide/Share — через system-button-регистрации. Метки кнопок читаются из
  device-info блока (v3 `controllerInfo`→`controllerButtonLabels`,
  v2 `controllerButtonInfo`, v0 — меток нет): слота `GetButtonLabel` на
  `IGameInputDevice` в vendored header нет — это имя WinRT. unsafe: в virshot
  `#![forbid(unsafe_code)]`, в `crates/gamepad` — crate-root
  `#![deny(unsafe_code)]` с единственным исключением в `gameinput.rs`
  (COM-vtable нельзя воспроизвести без unsafe). Нестандартные кнопки —
  `unknown:<N>` (raw-номер), см. §4.1 и §9. Linux: `gilrs` вне scope
  (hardware finding — §2 и §9), поток не запускается, код под `#[cfg(windows)]`.
- **Захват экрана:** `xcap`.
- **Глобальные клавиатурные хоткеи:** `global-hotkey` (approved 2026-11-03;
  system-wide: PrintScreen работает, когда фокус у игры; регистрация на
  main-тред — pump от winit).
- **Системный трей:** `tray-icon` (ksni) — интегрирован (Stage 10): polling
  глобальных receivers в цикле eframe.
- **Диалог выбора папки:** `rfd` (стандартные native dialog на Windows/Linux) —
  approved пользователем 2026-11-03.
- **Конфигурация:** `serde` + `toml`.
- **Логирование:** `tracing` + `tracing-subscriber` (файловый appender).
- **CLI:** `clap`.

Разработка — в WSL (Linux-сборка компилируется и работает core-функции:
захват/сохранение/конфиг/CLI; GUI и хоткеи на Linux — best-effort по
возможностям окружения). Финальная приемка — вручную на Windows.

## 4. Функциональные требования

### 4.1. Модуль ввода (Геймпад + хоткеи)

- Глобальные клавиатурные хоткеи регистрируются system-wide hook'ом (работают
  при фокусе у игры). Ошибка регистрации (конфликт/недоступно) — статус в UI +
  лог, без падения.
- Геймпад-поток (Windows — реализовано): отдельный поток-помпа `gameinput.dll`
  (`crates/gamepad`, §3, §6), plug-and-play; connect/disconnect устройства —
  INFO (§8); при отключении устройства поток не падает; если GameInput
  недоступен при старте — INFO и приложение продолжает работать (ошибки —
  значения, не паника). На Linux/GameInput-нет-хоста поток не запускается
  (INFO на старте), бинды из `config.toml` там не срабатывают.
- Каждое действие («весь экран», «окно», «область» [зарезервировано]) имеет
  **два поля бинда: Основное и Дополнительное**.
  - В MVP поле хранит **клавиатурную комбо** (клавиша + произвольный набор
    модификаторов Ctrl/Alt/Shift, напр. `Alt+PrintScreen`).
  - Поле «кнопка геймпада» (`pad`) — реализовано на Windows (`pad = null` —
    бинд не задан): значение — **одна** кнопка геймпада (комбо из нескольких
    кнопок геймпада **не поддерживается**). Нотация (case-insensitive):
    canonical имя кнопки GameInput → её индекс по порядку vendored header
    (`North`, `East`, `South`, `West`, `LeftShoulder`, `RightShoulder`,
    `LeftTrigger`, `RightTrigger`, `RightThumbStick`, `LeftThumbStick`,
    `LeftStick`, `RightStick`); `unknown:<N>` — raw-номер кнопки (нестандартные
    кнопки — Share, Options, paddle-кнопки — только raw-номер; UI показывает
    hint метки из device-info блока); `Guide` / `Share` — system-кнопки.
    Legacy gilrs-имена (`Select`, `Start`, `Mode`, `C`, `Z`, `DPad*`) в нотации
    не поддерживаются. В capture-режиме бинда «нажмите кнопку» кнопка
    записывается как `unknown:<N>` с номером из события (при нескольких новых
    нажатиях, пойманных одним опросом, — наименьший номер), system-кнопка —
    как `Guide`/`Share`.
  - Поля заполняются независимо; **действие срабатывает по любому заполненному
    полю (OR)**; клавиатурный и геймпад-источник выстреливают через **один
    общий debounce-гейт** на действие — один снимок, а не по одному на источник.
- Бинд в UI: клик по полю → capture-режим «нажмите клавишу» (не dropdown);
  клик по полю `pad` (Windows) → capture-режим «нажмите кнопку»: первый DOWN
  фиксирует бинд и завершает capture, отмена — повторный клик по тому же полю
  (Esc относится только к keyboard-capture). Пока любое бинд-поле находится в
  capture, геймпад-выстрелы подавляются. Список подключённых геймпадов в UI не
  делается (YAGNI).
- **Debounce** одного действия ~250 мс (повторный триггер короче интервала
  игнорируется) — один гейт на оба источника (§4.1 OR-fire).

### 4.2. Модуль захвата экрана

- **Мишень захвата** определяется в момент триггера по foreground-окну;
  собственные окна приложения (настройки/оверлей) исключены из анализа.
- **Весь экран** — монитор, на котором находится foreground-окно; если
  foreground не определён — fallback на primary монитор.
- **Окно** — захват содержимого foreground-окна независимо от перекрытия
  (на Windows — PrintWindow). Если активного окна нет (виден desktop) —
  **ошибка** («no active window») в тост и лог, файл не сохраняется; неудача
  захвата — тоже ошибка в тост и лог.
- Форматы: `png` (дефолт, без потерь), `jpg` (качество, дефолт 90),
  `webp` (качество, дефолт 90; в MVP — lossless VP8L, lossy перенесён:
  требует нативный dep с approval пользователя). Формат — **глобальная настройка**.
- **Шаблон имени файла** (см. §7): токены `%Y %y %mo %d %H %M %S %iso %t %n %ext`;
  `%iso` = `YYYY-MM-DD_HH-mm-ss`, время — **локальное**.
  - `%t` — в window-шаблоне: заголовок окна, в full-screen шаблоне: литерал `fullscreen`.
  - `%n` — счётчик коллизий в целевой папке (обычно 1), определяется
    сканированием папки на диске при сохранении (без state-файла).
  - `%ext` — расширение выбранного формата (`png`/`jpg`/`webp`).
- **Санитизация:** недопустимые символы в `%t` → `-`, пустой заголовок → `untitled`,
  обрезка до ~60 символов; сегменты шаблона санитизируются.
- **Валидация шаблона:** только относительный путь от `save_path`;
  абсолютные пути и `..` запрещены (ошибка в лог + тост).
- **Коллизии:** перезапись запрещена; при совпадении добавляется суффикс
  `-2`, `-3`… перед расширением (если `%n` в шаблоне — его значение).
- Отсутствующие подпапки шаблона создаются автоматически.
- Сохранение выполняется в отдельном потоке (без микро-фризов в игре).

### 4.3. Оверлей-уведомление (тост)

- Реализация (Stage 4, план overlay/toast): машинерия тоста вынесена в
  workspace-крейты — `crates/toast` (правила тоста на `std`-таймер-потоке:
  `ToastService`, `ToastHandle`, latest-wins) и `crates/overlay` (бэкенд
  хост-цикла `OverlayHost` поверх `egui::Context` multi-viewport + чистая
  геометрия углов `place`). Собственный модуль `src/overlay.rs` и весь
  toast-код в `src/gui.rs` удалены; правила ниже не изменились.
- **Владелец:** `SettingsApp` хранит `overlay::OverlayHost` и
  `toast::ToastService`; оба создаются в `create`-замыкании eframe в `gui::run`
  и живут до конца цикла. Порядок объявления полей — сервис **до** хоста:
  `Drop` сервиса (Quit + join потока, закрытие плашек) выполняется, пока хост
  ещё жив.
- **Передача handle:** `capture::ToastHandoff` — latest-only слот
  `Arc<Mutex<Option<ToastHandle>>>`. `gui::run` заполняет его в `create`;
  потоки-триггеры (хоткеи, геймпад, трей) читают слот на каждый захват.
  Handle передаётся capture **отдельным параметром** `run_capture`/`spawn_capture`,
  а не внутри `CaptureCommand`. Пустой слот (headless `virshot shot`, либо
  захват до/после GUI) — уведомления нет: CLI печатает путь в stdout и пишет
  результат в лог.
- **Рисование:** плашка — дочерний egui-viewport, созданный бэкендом:
  прозрачное окно без decorations, always-on-top, **mouse passthrough**
  (клики проходят в игру), **content-protected**. Окно создаётся скрытым
  (`with_visible(false)`, `with_active(false)` — показ без захвата фокуса),
  рисуется пока скрыто (`RequestPaintWhileHidden`) и показывается
  `Visible(true)` только после первой отрисовки — белой «тени» DWM нет.
  `OverlayHost::update(&ctx)` вызывается из `App::logic`, поэтому плашки
  появляются и гаснут и при скрытом окне настроек (на скрытом окне eframe
  выполняет `Context::run_logic`, чей `LogicOutput` содержит команды всех
  viewport'ов). Ограничения платформ те же: Wayland — позиция и always-on-top
  игнорируются (композиционер), passthrough работает; X11 — transparency только
  при создании окна.
- **Click-through на Windows игнорируется** в обоих бэкендах (`OverlayHost`
  и thread-бэкенд): winit 0.30.13 `set_cursor_hittest(false)` добавляет
  `WS_EX_LAYERED` без `SetLayeredWindowAttributes` — окно без layered-атрибутов
  не композитится (плашка не рисуется вовсе), поэтому флаг гасится на Windows
  (`cfg!(windows)`). Плашка не интерактивна — потеря passthrough безвредна.
  На Linux/macOS passthrough работает (X11 — input-shape).
- **Замена:** новый тост заменяет текущий (не стек), ровно одна плашка на
  монитор; spawn/закрытие выполняет бэкенд, app не управляет окнами.
- **Анимация:** fade-in отсутствует (плашка рисуется с полного непрозрачного
  первого кадра), fade-out фиксированный ~250 мс; удержание = `duration_ms`
  (дефолт 2000 мс); `duration_ms = 0` — плашка без авто-скрытия; ошибки
  держатся ~3000 мс независимо от `duration_ms`.
- **Позиция:** 4 угла экрана (`position = TopLeft|TopRight|BottomLeft|BottomRight`
  → `overlay::Corner`), фиксированные отступы 30 px от вертикальной границы и
  60 px от горизонтальной; нижние углы очищаются от панели задач запасом ~48 px.
  Вся математика — `overlay::place` (константы `CORNER_INSET_X`,
  `CORNER_INSET_Y`, `TASKBAR_CLEARANCE`); вариант с `GetMonitorInfo.rcWork`
  (`windows-sys`) отклонён ранее — unsafe FFI.
- **Мониторы:** геометрия берётся один раз при старте GUI на main-потоке —
  `xcap::Monitor::all()` (никогда не в hot-path захвата), переведённая в
  **LOGICAL точки** делением на `ctx.pixels_per_point()` (egui умножает
  позицию/размер builder'а на масштаб), **primary — первым**. `on_all_monitors`
  (дефолт `true`) → `MonitorSel::All` (тост на каждом мониторе, тот же угол);
  `false` → `MonitorSel::Index(0)` (primary). Fan-out корректен: ровно одна
  плашка на каждый монитор. Ошибка перечисления или пустой
  список — WARN/лог, плашки не показываются, приложение живёт; геометрия
  мониторов не обновляется при hotplug (снимок при старте). Debug-пример
  `toast_debug` использует ту же конвенцию: перечисляет **все** мониторы
  (`winit available_monitors()` ÷ `pixels_per_point()` корня) — раньше
  перечислял только primary. Caveat mixed-DPI: все physical-rects делятся на
  один root `pixels_per_point` — при разных DPI мониторов плашка на вторичном
  мониторе может встать неверно (отдельное решение, вне этой задачи).
- **Триггер:** тост срабатывает **только после скриншота** (успех
  захвата-сохранения либо ошибка), не на хоткей-событие. Показ/мониторы —
  INFO-лог для диагностики на Windows.
- **Содержимое (англ.):** успех — `Saved: <имя файла>` (полный путь остаётся в
  лог) с `ToastColor::Success`; ошибка — текст ошибки дословно с
  `ToastColor::Danger`. Гейт прежний: успех показывается только при
  `overlay.enabled = true`, ошибки показываются **даже при
  `overlay.enabled = false`** (чистая функция `capture::should_show`).
- Ключи конфига неизменны: `[overlay] enabled`, `position`, `duration_ms`,
  `on_all_monitors`.
### 4.4. Окно настроек (GUI)

- Обычное окно с рамкой; дефолтный размер ~720×520; позиция/размер не сохраняется.
- Закрытие крестиком — программа прячется в трей (не завершается).
- **Кнопка Apply:** записывает `config.toml` и применяет изменения живьём;
  закрытие окна без Apply отбрасывает несохранённые правки.
- Язык UI — английский.
1) **Вкладка "General":**
   - Корневой путь сохранения (`save_path`, кнопка **Browse** — стандартное
     диалоговое окно выбора папки; crate `rfd` — **new dep, требует approval**).
     Стартовый путь диалога: `save_path`, если существует, иначе
     `%USERPROFILE%\Pictures` (на Linux — `~/Pictures`).
   - Дефолт `save_path` резолвится при старте: `%USERPROFILE%\Pictures\Screenshots`
     (Linux — `~/Pictures/Screenshots`) — вместо хардкода пути.
   - Шаблон full-screen: дефолт `FullScreen/%y-%mo/%y-%mo-%d/%iso.%ext`.
   - Шаблон окна: дефолт `%t/%y-%mo/%y-%mo-%d/%t-%iso.%ext`.
   - Формат файла; `jpeg_quality`; `webp_quality`.
2) **Вкладка "Hotkeys":**
   - 3 колонки: Действие | Основная | Дополнительная.
   - Действия: `Full screen` (PrintScreen), `Window` (Alt+PrintScreen),
     `Rect area` (Ctrl+PrintScreen — слот зарезервирован, неактивен в MVP).
   - Поле бинда = **кнопка с текущим биндом** (текст `Alt+PrintScreen` и т.п.);
     клик → bind-by-press (новая комбинация клавиш заменяет бинд). Селект
     клавиши и чекбоксы модификаторов убраны; нотация в конфиге — без изменений.
     На время capture-press все глобальные хоткеи временно **отключаются**
     (иначе PrintScreen перехватывается зарегистрированным хуком и не доходит до
     окна), после commit/cancel — восстанавливаются из применённого конфига.
     Ограничение: модификатор `Win` недостижим из bind-by-press (winit не
     отдаёт Super) — задаётся правкой config.toml.
   - Ошибка регистрации (конфликт и т.п.) — **значок предупреждения справа от
     поля**, причина — в тултипе; инлайн-текст ошибки убран. Резерв `rect` —
     info-значок с тултипом. Статусы — также в лог.
   - Поле кнопки геймпада (`pad`) — реализовано на Windows (§4.1): клик по нему
     → capture-режим «нажмите кнопку» (кнопка `Press a button…`); первый DOWN
     пишет `unknown:<N>` / `Guide` / `Share` в draft (публикуется только Apply),
     рядом с полем — hint метки кнопки из device-info блока (in-memory, в конфиг
     не сохраняется). Отмена — повторный клик по тому же полю (Esc относится
     только к keyboard-capture); старт keyboard-capture отменяет pad-capture.
     Во время pad-capture геймпад-выстрелы подавлены, клавиатурные хоткеи
     остаются активны (пауза регистраций относится только к keyboard-capture).
     На хосте без GameInput поле — disabled placeholder
     «(gamepad — Windows only)».
3) **Вкладка "Overlay":**
   - `enabled`, угол экрана, `duration_ms`, `on_all_monitors`.

### 4.5. Системный трей

- Встроенная иконка (простой camera glyph; кастомная — позже).
- Левый клик — открыть окно настроек.
- Правый клик — меню: `Take screenshot` (захват всего экрана), `Settings`, `Exit`.
- Выход — только из меню трея.
- Интеграция `tray-icon` с event loop: решено (Stage 10) — polling глобальных
  receivers в UI-цикле eframe (`try_recv` каждый кадр, +repaint каждые 500 мс);
  ksni бэкенд требует D-Bus session, без него — typed-статус, приложение живёт.

## 5. Нефункциональные требования

- **Размер бинарника:** < 10 МБ (release: `strip`, `lto`, `opt-level="z"`).
- **RAM:** < 30 МБ в фоне (без захвата). **CPU:** < 1% при простое.
- **Безопасность:** никаких сетевых запросов; полная приватность.
- **Отказоустойчивость:** любая ошибка (нет доступа к папке, ошибка регистрации хоткея,
  неудачный захват) логируется и показывается тостом — без аварийного завершения.

## 6. Архитектура и потоки (Threading)

Модель (итог — Stage 6-10):

1. **Main Thread (UI + трей + регистрация хоткеев):** цикл `eframe` (окно
   настроек + оверлей: `overlay::OverlayHost` и `toast::ToastService` владеет
   `SettingsApp`, хост обновляется в logic-pass — плашки живут и при скрытом
   окне), polling событий трея каждый кадр, регистрация hotkeys (WM_HOTKEY
   thread-affinity, pump — winit). В `create`-замыкании handle сервиса
   кладётся в `capture::ToastHandoff` (§4.3).
2. **Input Thread (Hotkeys):** слушатель глобального receiver событий хоткеев →
   debounce → команда (owned `CaptureCommand`) + handle из `ToastHandoff`
   (читается на каждый выстрел) в capture-поток.
   **Gamepad Pump Thread (Windows):** поток-помпа `gameinput.dll`
   (`crates/gamepad`) — события → diff масок состояний (0→1 edges) →
   сопоставление с `pad`-биндом → **тот же общий debounce-гейт** (~250 мс на
   действие, §4.1) → та же команда + тот же handle в capture-поток. Связь с
   остальными модулями — mailbox-ы без callback в GUI: settings-mailbox (живой
   конфиг после Apply — re-bind выстреливает без перезапуска) и pad-capture
   mailbox (§4.4). Поток резидентен на весь процесс (stop-флаг не выставляется —
   parity с hotkeys-слушателем). На не-Windows хостах поток не запускается.
3. **Capture Thread:** принимает команду, захватывает (monitor/window), сохраняет
   файл; уведомление — через `ToastHandle` (отдельный параметр, не поле
   команды); при `None` (headless CLI, либо захват без GUI) — только лог и
   stdout.
4. **Toast Timer Thread (`crates/toast`):** таймер-поток крейта — latest-wins,
   удержание/fade-out, закрытие плашек через sink (`OverlayHostHandle`);
   `Quit` + join при drop сервиса (поле app объявлено раньше хоста).

Клавиатурные хоткеи: регистрация на main-тред (thread-affinity WM_HOTKEY на
Windows; X11 pump — свой поток крейта на Linux), слушатель событий — отдельный
поток (blocking receiver), debounce 250 мс per-action.

## 7. Формат конфигурации (config.toml)

Путь (портабельный режим): Windows — `config.toml` рядом с `virshot.exe`
(и `virshot.log` рядом); если каталог exe недоступен для записи — fallback
`%APPDATA%\virshot\`. Linux — `~/.config/virshot/config.toml` (XDG).
Переопределяется `--config`. При первом запуске создаётся дефолтный конфиг.
Неспарсенный/битый конфиг → ошибка в лог + работа на дефолтах.

Схема (финальная; нотация клавиш — коды `Code` без регистра, модификаторы
`alt/ctrl/shift/win`, `pad` — бинд кнопки геймпада, активен на Windows, §4.1):

```toml
[general]
save_path       = "%USERPROFILE%\\Pictures\\Screenshots"  # резолвится при старте
file_full_screen = "FullScreen/%y-%mo/%y-%mo-%d/%iso.%ext"
file_window      = "%t/%y-%mo/%y-%mo-%d/%t-%iso.%ext"

format      = "png"        # png | jpg | webp
jpeg_quality = 90
webp_quality = 90

[controls.full_screen]
key  = "PrintScreen"
mods = []          # ["Alt"], ["Ctrl","Shift"] ...
pad  = null        # canonical имя GameInput (напр. "South"), "unknown:<N>" (нестандартная кнопка) или "Guide"/"Share"

[controls.window]
key  = "PrintScreen"
mods = ["Alt"]
pad  = "unknown:16"  # пример нестандартной кнопки (номер проверяется на железе)

[controls.rect]          # зарезервировано, неактивно в MVP
key  = "PrintScreen"
mods = ["Ctrl"]
pad  = null

[overlay]
enabled         = true
position        = "BottomRight"   # TopLeft | TopRight | BottomLeft | BottomRight
duration_ms     = 2000
on_all_monitors = true
```

## 8. CLI

Флаги: `--config`, `--log`, `--log-level=DEBUG|INFO|ERROR`, `--version`, `--help`.

Подкоманда (headless, для тестов захвата):

```
virshot shot [--window] [--out <path>] [--format png|jpg|webp]
```

- Без GUI/трея/оверлея; конфиг — источник дефолтов; `--out`/`--format` переопределяют.
- Печатает сохранённый путь в stdout; exit 0 — успех, 1 — ошибка.

Логи: по умолчанию `virshot.log` рядом с конфигом; `--log` переопределяет;
`--log-level` дефолт `INFO`; без ротации. Успешные сохранения — INFO;
connect/disconnect геймпада — INFO (Windows); выстрел с геймпада и фиксация
pad-бинда в UI — INFO.

## 9. Ограничения (документируем в README)

- **Exclusive fullscreen не поддерживается:** захват может быть чёрным, оверлей
  не виден. Рекомендация — Borderless Windowed. Workaround не делаем.
- **Linux — best-effort:** core (захват/конфиг/CLI) работает; GUI работает на
  X11/Wayland; глобальные хоткеи — только X11 (Wayland не поддерживается
  крейтом — typed-ошибка при старте); трей — X11/D-Bus session (SNI); тост на
  Wayland без позиции/always-on-top (компоунтер игнорирует).
- **Win11:** PrintScreen может перехватываться Snipping Tool — в приложении
  обработки нет; приемка: отключить настройку ОС или перепривязать хоткей в UI.
- **Нестандартные кнопки геймпада (Share, Options, paddle-кнопки и т.п.):**
  gameinput-бэкенд отдаёт кнопки как raw-номер (`unknown:<N>`), метка читается
  из device-info блока (не через `GetButtonLabel` — такого слота на
  `IGameInputDevice` в vendored header нет); номер стабилен только в пределах
  одной модели геймпада. Геймпады, которые WGI классифицирует как Gamepad
  (Xbox Series), отдают только канонические кнопки — Share на них может не
  приходить. Xbox Game Bar перехватывает Share (побочный запуск Game Bar).
  **Hardware-verification (2026-11-04): пример `buttons.exe` проверен на железе
  пользователя — все ок** (устройство, button bits, Guide/Share приходят).
  Остаток приемки: virshot с pad-биндом на Windows (выстрел по кнопке,
  capture в UI).
- **Геймпад на Linux — вне scope (аппаратная находка):** `gilrs` не добавлен —
  wgi-бэкенд `gilrs` гейтит чтения по timestamp контроллера, generic
  HID-геймпады отдают константный timestamp, поэтому нажатия не приходят
  (проверено на железе пользователя: pad vendor=1118 product=2835). На Linux
  геймпад-триггер не работает, поле `pad` в UI — disabled placeholder.
- **Windows — mouse passthrough не поддерживается:** `set_cursor_hittest(false)`
  (winit 0.30.13) добавляет `WS_EX_LAYERED` без `SetLayeredWindowAttributes`,
  окно без layered-атрибутов не композитится, поэтому click-through на Windows
  игнорируется в обоих бэкендах оверлея (см. §4.3). Плашка не интерактивна —
  потеря passthrough безвредна; клики в игру на Windows не проходят.
- **Transparency на Windows — hint, не гарантия:** известная проблема
  прозрачных child-viewports на glow (egui#8423); после снятия `WS_EX_LAYERED`
  (gate click-through выше) прозрачные плашки композитируются DWM.
  `ContentProtected` на ре-композитированном окне — **Needs Windows-verification**
  (приемка на железе пользователя).

### 9.x Сборка Windows (доработки v3)

- subsystem `WINDOWS` (без чёрного окна консоли при запуске):
  `link-args=-Wl,--subsystem,windows` для mingw в `.cargo/config.toml`.
  CLI `virshot shot` пишет stdout только при запуске из уже открытой консоли
  (cmd/PowerShell); двойной клик — без консоли, молча.

## 10. Тестирование и поставка

- **Юнит-тесты:** шаблон пути, санитизация, валидация, парсинг/дефолты конфига.
- **Интеграция (WSL):** headless `virshot shot` — реальный захват/запись файлов.
- **Приемка на Windows:** вручную, чеклист выдаётся в чате (не в репозитории).
- **Поставка MVP:** `virshot.exe` прямо из билда
  (`target/x86_64-pc-windows-gnu/release/virshot.exe`) + README.md; архив (zip)
  не делается — пользователь запускает из билда; без инсталлятора и автообновления.
