# 2026-11-13_gpu-repaint-throttle: снижение потребления GPU (интервал перерисовки)

**Created:** 2026-11-13T10:56:59+03:00

## Scope
- **In scope:**
  - `OverlayHost::paint`: безусловные per-pass перерисовки заменаются на отложенные
    (`request_repaint_after`) — плашка тоста в состоянии Hold не должна гонять цикл на полной
    частоте (`crates/overlay/src/host.rs:471`, `:479-483`).
  - About-viewport и диалог обновления: per-pass `request_repaint_of`
    (`src/gui/app.rs:571`, `:844`) на статичном контенте → тоже отложенная перерисовка.
  - Интервал перерисовки — именованная константа в модуле-владельце (значение 200 мс).
  - About-viewport и диалог обновления троттлятся тем же интервалом (решение пользователя).
  - Примеры тоже троттлятся: `crates/video-capture/examples/capture_demo.rs:1056`
    (непрерывный repaint ради опроса ackov от потока захвата) и
    `crates/icons/examples/icons_gallery.rs:123` (уже троттлен, константа 100 мс) —
    интервал выносится в константу по конвенции модуля.
- **Out of scope:**
  - Событийные `request_repaint()` (закрытие окна, tray-действия, приход результата
    обновления/установки, spawn/redraw, probe-press) — там нужна немедленная реакция.
  - Fade-out тоста остаётся немедленным (шаг 25 мс) — решение пользователя: анимация 250 мс
    не должна превращаться в 1–2 кадра.
  - Архитектура оверлея/тоста, стек рендера (eframe/glow), standalone-бэкенд
    `crates/overlay/src/service.rs` (winit `request_redraw`, не egui-репейнт).
  - Новые зависимости, изменение конфигурации (TOML), интервал как настройка.

## Approach
1. **Deep-исследование egui 0.37 (выполнено, этап 1):** семантика подтверждена по закреплённым
   источникам и scratch-пробу (`research.md`, секция "Verified egui 0.37 repaint API"):
   egui хранит **самый ранний** scheduled repaint (строгий `delay < repaint_delay`);
   отложенный запрос даёт ровно один repaint, немедленный — два; публичного способа прочитать
   остаток задержки нет (`has_requested_repaint[_for]` — только bool, `set_repaint_observer` —
   единственный наблюдатель всех запросов); отложенный запрос **для конкретного viewport**
   существует: `Context::request_repaint_after_for(delay, id)`; root-repaint **не** будит
   Deferred children.
2. **Механика (по фактам этапа 1):**
   - Плашки (Deferred child-viewport): `ctx.request_repaint_after_for(REPAINT_INTERVAL, id)`
     вместо `request_repaint_of(id)`, только при наличии окна (`host.rs:246-250`).
     Отложенный запрос даёт один repaint, поэтому он отправляется каждый проход — steady-state
     получается 5 fps, а не «замирает».
   - Гейт отправки команд по изменению: `Context::send_viewport_cmd_to(id, cmd)` сам ставит
     `repaint_delay = ZERO` для viewport (egui `context.rs:4569-4573`), и строгий `<`
     дедуплицирует отложенный запрос того же прохода — поэтому команды (OuterPosition,
     InnerSize, Visible, ContentProtected) отправляются **только когда значение изменилось**
     (хост хранит последнее отправленное значение), иначе троттл не работает: фактический
     интервал остаётся 0 мс.
   - `ViewportCommand::RequestPaintWhileHidden` для ROOT отправляется только пока root-окно
     скрыто и плашка требует paint; при видимом root он не нужен каждый проход.
   - Эффективный расписание проверяется через публичный
     `FullOutput.viewport_output[id].repaint_delay`, а не только через observer (observer
     фиксирует и дедуплицированные запросы).
   - Fade остаётся немедленным: `drain(Redraw)` (`host.rs:269`) должен будить **конкретный
     viewport** (`request_repaint_of(id)`), потому что root-repaint детей не будит — иначе
     fade на 25 мс перестает анимироваться.
   - Root-цикл при живой плашке: `ctx.request_repaint_after(REPAINT_INTERVAL)` вместо
     `ctx.request_repaint()` (`host.rs:479`); `RequestPaintWhileHidden` для ROOT отправляется
     каждый проход как раньше (`:480-483`). Pending 200 мс не мешает немедленному запросу из
     toast-потока: `ZERO < 200ms` проходит строгий `<` и проход выполняется раньше.
   - `drain(Spawn)` (`host.rs:251`) остаётся немедленным — registration/первый paint не должен
     ждать 200 мс.
   - eframe поднимает invisible/minimized окна до `now + 100ms` — 200 мс безопасен; интервалы
     ниже 100 мс были бы молча бесполезны.
3. **Константа:** `REPAINT_INTERVAL: Duration = Duration::from_millis(200)` в модуле-владельце
   по существующей конвенции (`SCREAMING_SNAKE`, module-level, с doc-комментарием —
   precedent `src/gui/app.rs:104 TRAY_POLL_TICK`, `crates/toast/src/rules.rs:22-30`). Одна
   константа на механизм хоста; для child-viewport в `src/gui/app.rs` — своя константа там же
   (один модуль = один файл, без центрального модуля констант).
4. **Что остаётся немедленным:**
   - `OverlayCommand::Spawn` (`host.rs:251`) и fade-тик (toast-поток будит контекст каждые
     25 мс, `crates/toast/src/service.rs:295`, `crates/toast/src/rules.rs:30`) — fade-out
     250 мс обязан остаться плавным (~25 мс шаг), поэтому его не троттлим.
   - `OverlayCommand::Redraw` (`host.rs:269`) будит конкретный viewport немедленно (этап 1
     подтвердил: root-repaint Deferred children не будит).
   - Root-loop при скрытом окне: `request_repaint_after(TRAY_POLL_TICK)`
     (`src/gui/app.rs:954`) и deadline авто-обновления (`:960`) не меняются; троттл 200 мс
     короче 500 мс, так что polling и deadline-проходы остаются.
5. **TDD там, где возможно:** чистая логика (значение константы, поведение fade vs Hold) —
   юнит-тесты; ассерт «scheduled repaint» — через публичный API: `set_repaint_observer`
   (единственный публичный наблюдатель всех запросов) и `has_requested_repaint_for(&id)`.
   Численного «сколько осталось до repaint» нет — тест проверяет факт запроса и его задержку
   через observer, а не внутренности контекста.
6. **Измерение GPU:** в этом sandbox (headless Linux) реально измерить GPU нельзя;
   подтверждение — компиляция, clippy, юнит-тесты + отсутствие непрерывного repaint по коду.
   Это честное ограничение, не заявленный «результат экономии».

## Constraints
- eframe/egui **0.37.0**, `winit 0.30.13`, glow-бэкенд; `App::logic`/`App::ui`, при скрытом
  окне выполняется только `logic` (`src/gui/app.rs:12-24`).
- egui хранит **самый ранний** scheduled repaint → несколько `request_repaint_after`
  аддитивны, троттл не «перебивает» более ранний deadline (`src/gui/app.rs:955-958`).
- Нельзя вызывать `request_repaint_of` для viewport без окна: это обнуляет `repaint_delay`
  и дедуплицирует последующие запросы, окно перестает перерисовываться
  (`crates/overlay/src/host.rs:246-250`).
- `RequestPaintWhileHidden` держит один кадр и должен отправляться каждый проход
  (`crates/overlay/src/host.rs:465-467`, `:480-483`) — троттл меняет частоту проходов,
  но не схему отправки.
- Плашки не интерактивны (`crates/overlay/src/host.rs:347-355`), так что 200 мс не влияет
  на отзывчивость мыши.
- Один winit `EventLoop` на процесс (`crates/overlay/src/service.rs:3-7`); toast-поток —
  plain `std`, `recv_timeout` (`crates/toast/src/service.rs:157-169`).
- `unsafe` запрещён (для overlay — `deny` + scoped `allow` только в `gl_boot.rs`).
- Новые зависимости — только с явного подтверждения пользователя; здесь их не нужно.
- Ворота: `./scripts/check.sh` (root) после каждой партии правок;
  `./cargo.sh test -p overlay -p toast`; границы этапа — `./scripts/check_full.sh`;
  Windows-гейт — `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`.
- Тесты overlay/toast — inline `#[cfg(test)]` в модулях (`crates/overlay/src/host.rs:710`,
  `crates/toast/src/rules.rs:138`); `tests/unit/` — пустая заглушка (root bin-only).
- Бюджет из `docs/spec.md:873`: RAM < 30 МБ в фоне, CPU < 1% при простое; интервал как
  настройка в spec §4 нет (`docs/spec.md:401-404` описывает fade 250 мс / hold `duration_ms`).

## Open questions
- Закрыто этапом 1: отложенный запрос для конкретного viewport существует —
  `Context::request_repaint_after_for(delay, id)` (egui `context.rs:2222`), поэтому троттл
  плашек, About и dialog делается им, только при наличии окна у viewport.
- Закрыто этапом 1: root-repaint Deferred children **не** будит (egui `viewport.rs:19-33`),
  поэтому `drain(Redraw)` будит конкретный viewport немедленно — fade остаётся плавным.
- `capture_demo`: при 200 мс ackи потока захвата становятся видны позже (не теряются, просто
  рендерятся реже). Если демо должно показывать кадры «в момент прихода», интервал для
  примера подбирается меньше 200 мс — уточняется на этапе примеров по факту кода.
  **Закрыто этапом 4:** по факту кода в демо нет ни одного sub-100 мс Animation
  (нет spinner/progress, единственный repaint-драйвер — `App::ui`, `:1079`), так что
  интервал равен общему 200 мс; формулировка в комментарии честная: ack рендерится
  не «в момент прихода», а максимум через один интервал (≤ 200 мс).

## Changes
- 2026-11-13T10:56:59+03:00 -- v1, initial spec (draft + research). Интервал зафиксирован
  пользователем: 200 мс.
- 2026-11-13T11:19:11+03:00 -- решения пользователя на review: fade-out тоста **не** троттлим
  (оставается шаг 25 мс); About-viewport и диалог обновления троттлим тем же интервалом;
  примеры (`capture_demo`, `icons_gallery`) вынесены **в scope** — интервал в константу.
- 2026-11-13T12:01:51+03:00 -- этап 1 (deep-исследование egui 0.37) закрыл open questions:
  троттл через `request_repaint_after_for(delay, id)`; root-repaint детей не будит, поэтому
  `drain(Redraw)` будит viewport немедленно; тестовый ассерт — `set_repaint_observer` /
  `has_requested_repaint_for`. Механика записана в Approach.
- 2026-11-13T12:37:50+03:00 -- красный тест показал, что `send_viewport_cmd_to` pin’ит
  `repaint_delay = ZERO` каждый проход и дедуплицирует отложенный запрос. Решение пользователя:
  команды viewport отправляются только при изменении значения (гейт по изменению),
  `RequestPaintWhileHidden` — только пока root скрыт; ассерт теста —
  `FullOutput.viewport_output[id].repaint_delay` (эффективное расписание), не observer.
- 2026-11-13T13:51:59+03:00 -- Stage 2 исполнен и принят независимой проверкой: steady-state
  chip + root = 200 мс (`FullOutput`), spawn/redraw/DPI-correction остаётся немедленным,
  команды viewport шлются только при изменении значения. Остаточные риски для Windows-приёмки:
  ROOT `RequestPaintWhileHidden` на реальном хосте почти не отправляется (egui не пишет
  `ViewportInfo.visible`; эффект ограничен — hidden/minimized root всё равно делает logic-only
  проходы, а child с `visible == None` считается видимым); chip показывается ровно один раз
  (`shown`), повторный hide от ОС не восстанавливается. Нужна проверка на Windows-стенде:
  плашка видна при скрытом окне настроек.
- 2026-11-13T15:42:52+03:00 -- Stage 3 исполнен и принят: About и диалог обновлений
  перерисовываются по константе `CHILD_VIEWPORT_REPAINT_INTERVAL` (200 мс,
  `request_repaint_after_for` с проверкой наличия окна); заметки релиза и события
  discovery/install остаются немедленными. egui сам ставит ZERO на первые два прохода
  свежеоткрытого Deferred-viewport (one-off, не каждый проход) — тесты это учитывают.
  Нюанс: троттл пере-выпускается только в root-проходах, поэтому при скрытом окне
  настроек худший лаг контента child ≈ 500 мс (tray tick) + 200 мс ≈ 700 мс; статичный
  контент это не показывает, текст статуса диалога — может. Нужна Windows-приёмка:
  About/диалог при скрытом окне, open/raise/close child-окон, статус install до
  перезапуска.
- 2026-11-13T15:53:45+03:00 -- этап 4 (примеры) исполнен: `capture_demo` троттлится
  константой `REPAINT_INTERVAL` = 200 мс (`request_repaint_after`, пере-выпуск каждый
  проход — отложенный запрос даёт ровно один repaint), `icons_gallery` остаётся на
  100 мс с `REPAINT_EVERY` как единственным источником интервала в файле: галерея
  ограничена кадрами (`MAX_FRAMES` = 10 paint вне зависимости от интервала), ничего
  не анимирует, и 100 мс — это floor eframe для скрытого окна, то есть минимальный
  полезный интервал; выравнивание на 200 мс растянуло бы ран ~1 с → ~2 с без
  сокращения числа repaintов. Ассерт — inline `#[cfg(test)]` в обоих примерах
  (значение константы, floor, `MAX_FRAMES * REPAINT_EVERY` внутри lifetime),
  прогон `./cargo.sh test --examples -p icons -p video-capture` — 4 теста зелёные.
  Ворота зелёные: `./cargo.sh check --all-targets` (root-пакет — crate-примеры он
  не compile'ит), `./cargo.sh check --all-targets --workspace`,
  `./cargo.sh clippy --all-targets -- -D warnings` и `--workspace`,
  `./cargo.sh fmt --check`, `./scripts/check.sh`. Остаточные риски: статус демо
  отстаёт от acka максимум на 200 мс; при скрытом окне `App::ui` не выполняется
  (опрос ackov там не работает и раньше, поскольку `logic` в демо не реализован).
- 2026-11-13T16:17:42+03:00 -- этап 5 (границы этапа): `./cargo.sh test --workspace` — exit 0
  (gamepad 13, icons 2+1+7, overlay 54, toast 43, video_capture 117, virshot 617, e2e 9;
  3 `#[ignore]` — host-dependent), `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`
  — exit 0, отдельно `--workspace` вариант — 10 `.exe`, включая `gamepad-*.exe` (без
  `--workspace` cargo собирает только root-пакет), `./scripts/check_full.sh` — exit 0,
  файлы не изменены. Ограничение измерений: хост headless Linux без display и GPU,
  реальные GPU numbers не мерились (`scripts/prof.sh` не запускался), GUI-примеры
  не исполнялись, Windows-тесты скомпилированы но не запущены — проверка = код + тесты
  + гейты. Отдельная находка: `clippy --all-targets --workspace --profile test` падает
  на единственной pre-existing ошибке `items_after_test_module` в
  `crates/toast/examples/toast_debug.rs` (файл не в дифе задачи, его коммиты `0307f9d`
  и `50118e5` старше коммитов задачи) — вне скоупа, не правится.
