# 2026-11-04_overlay-toast: overlay + toast libraries

**Created:** 2026-11-04T12:09:08+03:00

## Scope

**In scope:**
1. `crates/overlay/` — низкоуровневая библиотека вывода произвольных данных (текст, картинки,
   произвольная egui-отрисовка) в оверлей-окно в произвольном месте экрана; окно защищено от экранного
   захвата (content-protected) и клик-сквозная; кроссплатформенная (Windows + Linux).
2. `crates/overlay/examples/` — консольное приложение (из терминала), запускающее отрисовку в оверлей.
3. `crates/toast/` — библиотека уведомлений поверх `crates/overlay/`, API:
   - экран показа (конкретный монитор) / все экраны;
   - текст;
   - картинка (опционально);
   - цвет: `default | success | danger | warning`;
   - положение: угол экрана (4 угла);
   - время автоскрытия, ms (0 = висит вечно, скрывается только событием извне).
4. `crates/toast/examples/` — отладочное окно: 4 кнопки (4 положения), тип уведомления,
   автоскрытие, экран показа, выбор картинки.
5. Интеграция `crates/toast` в `virshot`: уведомления только **shot success | fail** (заменяют
   текущую реализацию тоста в `src/overlay.rs` + `src/gui.rs`).
6. `crates/overlay` и `crates/toast` — участники cargo workspace.

**Out of scope:** другие типы уведомлений virshot (кроме shot success/fail); анимация появления;
изменение поведения захвата скриншотов; Windows-допилки — тестируются пользователем после.

## Approach

- **Синхронное ядро на выделенном OS-потоке + неблокирующий фасад.** (Решение по
  `.tmp/overlay-async.md` и TLDR пользователя, 2026-11-04.) Каждый crate (`overlay`, `toast`)
  владеет выделенным потоком (`std::thread::spawn`) с блокирующим `winit`-циклом
  (`EventLoop::with_user_event().run_app`). Внутренняя реализация синхронная: крейты **не**
  предоставляют `async fn` и не требуют tokio внутри себя. Публичный API — неблокирующий фасад
  поверх `std::sync::mpsc` (unbounded): `show/dismiss/update` ставят команду в очередь и
  возвращаются немедленно, безопасны из sync- и async-контекстов и из любого треда.
  Несколько продюсеров — `Sender` обёрнут в `Arc<Mutex<Sender>>` (минимальный lock только при
  постановке команды). Хост не управляет event loop и таймерами.
- **Оверлей = выделенный поток + winit + glutin + egui_glow (standalone-бэкенд).** `crates/overlay`:
  на потоке — winit event loop, GL-поверхность через `glutin` + `glutin-winit`, отрисовка
  (egui + glow-контекст); окно создаётся winit-атрибутами напрямую:
  `with_transparent(true)`, `with_decorations(false)`, `with_always_on_top`,
  `with_content_protected(true)` (параметр), клик-сквозность —
  `Window::set_cursor_hittest(false)` (параметр), позиция/размер —
  `with_position/with_inner_size`. Контент-команды: текст, картинка (bytes), painter-callback
  (`Fn(&mut egui::Ui) + Send`). Публичный facade: `OverlayHandle` (Send+Sync),
  spawn_window/redraw/hide/quit_all. **Constraint (verified, winit 0.30.13
  event_loop.rs:118):** ровно один `EventLoop` на процесс (global `EVENT_LOOP_CREATED`) —
  оверлей-поток владеет им только в standalone-процессах (примеры). В хост-приложениях (virshot,
  где eframe уже владеет loop'ом) оверлей использует **host-бэкенд**: `OverlayHost` —
  egui multi-viewport поверх `egui::Context` хоста (`ViewportBuilder` transparent/passthrough +
  `ViewportCommand::ContentProtected`), рисуется в logic-pass хоста, тот же mpsc facade.
  N окон мультиплексируются `EventLoopProxy` на единственном loop'е.
- **Тост = свои правила + свой поток, поверх оверлея.** Таймер-поток тоста — **обычный
  std-поток** (не winit — второй EventLoop в процессе невозможен): цикл
  `recv_timeout(до следующего фазового перехода)` качает фазы `Hold → FadeOut → Gone`
  (fade 250ms, error-hold 3000ms, `duration_ms`, 0 = sticky до `dismiss` — на sticky
  поток блокируется на `recv()` без тиков), позиционирование по углам (insets 30/60px,
  нижние +48px), палитра default/success/danger/warning, выбор монитора `Index(n) | All`.
  Тост толкает контент в **sink** (`OverlayHandle` для standalone или `OverlayHost` для
  хоста) — окна создаёт оверлей, таймеры качает тост.
  API: `ToastHandle { show(Toast), dismiss(id), update(Toast) }`,
  `Toast { monitor, text, image: Option<Vec<u8>>, color, corner, duration_ms }`.
- **Картинка:** API принимает bytes; helper `load(path) -> Vec<u8>`; декод на оверлей-потоке
  через `image` 0.25 (png/jpeg/webp уже в lock) → `egui::TextureHandle`.
- **Примеры:** `crates/overlay/examples/overlay_demo.rs` — консольное приложение (поток-бэкенд:
  оверлей владеет единственным loop'ом процесса), циклически показывает текст/примитивы в
  разных углах. Отладочное окно `crates/toast/examples/toast_debug.rs` — eframe-окно
  (хост-бэкенд: loop у eframe, `OverlayHost` рисуется в его logic-pass) с кнопками 4 углов,
  выбором цвета/экрана/картинки(bytes)/duration (0 = sticky); кнопки «показать», «скрыть».
- **Интеграция в virshot:** `main.rs` спавнит `ToastService` (host-бэкенд: `OverlayHost` +
  таймер-поток) один раз, `ToastHandle` (Send) передаётся в capture-треды;
  `src/capture.rs` (`run_capture`, :166) вызывает `show` для Success/Error (неблокирующий
  вызов, заменяет mailbox `ToastMailbox`); `src/gui.rs` в logic-pass вызывает
  `OverlayHost.update(ctx)` (drain команд + отрисовка viewports; wake уже есть —
  `request_repaint_after(TRAY_POLL_TICK)`, gui.rs:1607); вся тост-механика `src/gui.rs`
  удаляется (deferred viewports, helpers `toast_window_size/toast_position/toast_placements/
  toast_show_commands/update_toast`); `src/overlay.rs` удаляется; гейттинг конфига `[overlay]`
  (`enabled` — ошибки показываются даже когда выключен) и отображение
  `position/duration_ms/on_all_monitors` в параметры тоста остаются в virshot без изменения
  формата конфига; `docs/spec.md` §4.3 обновляется (content-protected, клик-сквозность,
  тост-окна рисует OverlayHost на loop'е eframe).
- **Workspace:** в root `Cargo.toml` `[workspace] members = [".", "crates/overlay",
  "crates/toast"]`, `exclude = ["crates/gamepad", "hidpad-poc"]`. Факт Stage 1: cargo
  автоматически добавляет path-dep участников — `crates/gamepad` стал участником workspace
  (exclude не вето; gamepad компилируется/clippy-зелён на обоих таргетах, standalone-запуск
  сохранён); `hidpad-poc` получил пустой `[workspace]` в своём манифесте — standalone-сборка
  восстановлена. AGENTS.md обновляется на Stage 5.

## Constraints

- Locked stack: egui/eframe/egui-winit/egui_glow **0.37.0**, winit **0.30.13**, glow 0.17 —
  все уже в Cargo.lock (через eframe glow); **новые crates.io-зависимости не нужны** — только
  path-deps и прямые декларации зависимостей тех же версий (egui, eframe для примеров, image для тоста).
  Скелет из `.tmp/overlay.md` (egui 0.31/glow 0.16) выравнивается под lock 0.37/0.17.
- Unsafe policy (решение по факту Stage 2): `#![forbid(unsafe_code)]` несовместим с glutin
  (context/surface creation — только `unsafe fn`, E0453 на вложенный allow). `crates/overlay`:
  `#![deny(unsafe_code)]` + модульный `#![allow(unsafe_code)]`, ограниченный `gl_boot.rs`
  (прецедент — hand-written shim в `crates/gamepad`). `crates/toast` — unsafe нет вовсе.
  Ограничение фиксируется в AGENTS.md на Stage 5.
- Кроссплатформенность: Windows — content-protected реально работает (winit windows impl);
  Linux X11/Wayland — winit не предоставляет аналог защиты от захвата (likely no-op) —
  оверлей рисуется и клик-сквозится, но исключение из скриншота на Linux проверяется фактом
  (x11-скриншоты захватывают всё); честно фиксируется в docs.
  Wayland: position/always-on-top игнорируются композитором, passthrough работает
  (docs/spec.md §4.3).
- X11: прозрачность окна возможна только при создании (уже учтено в текущем builder).
- **Два event loop в одном процессе** (eframe main + overlay/toast потоки) — на Linux это
  отдельные X/Wayland соединения, на Windows pump'ы независимы; проверяется фактом на Linux
  (Stage 5), на Windows — пользователем. hotkey manager остаётся на main-треде (WM_HOTKEY
  аффинити не затрагивается).
- macOS out of scope (event loop на отдельном потоке допустим на Windows/Linux).
- Gates: `./scripts/check.sh` (Linux), перед коммитом `./scripts/check_full.sh`;
  windows-gnu — compile-only `test --no-run`; scratch — `./.tmp/`, кэш — `./.cache/` через `./cargo.sh`.
- Итого по зависимостям: ни одного нового crates.io-пакета и ни одной новой версии — только
  прямые декларации пакетов, уже находящихся в Cargo.lock (winit 0.30.13, glutin 0.32.3,
  glutin-winit 0.5.0, glow 0.17, egui/egui-winit/egui_glow 0.37.0, image 0.25, eframe для
  примеров). Подтверждение пользователя на эти декларации — при утверждении плана.

## Open questions

- none

## Changes

- 2026-11-04T12:09:08+03:00 -- v1, initial spec (draft + research; уточнения пользователя
  включены: workspace, клик-сквозность параметром, кроссплатформенность Win+Linux,
  virshot = только shot success|fail)
- 2026-11-04T12:11:44+03:00 -- review: картинка в тосте — API принимает bytes,
  отдельный helper `load(path) -> bytes` (решение пользователя); юнит-тесты — только чистые
  правила, viewport-механика проверяется примерами (решение пользователя)
- 2026-11-04T12:15:03+03:00 -- уточнение синхронности: в spec добавлен контракт хоста
  (таймеры не внутри либ — repaint будит приложение; winit-операции только с главного треда;
  между тредами только mailbox)
- 2026-11-04T12:27:06+03:00 -- v2, переработка синхронности по `.tmp/overlay-async.md` и TLDR
  пользователя: синхронное ядро на выделенном OS-потоке с блокирующим winit-циклом в каждом
  crate; публичный API — неблокирующий фасад на `std::sync::mpsc` (unbounded), без async fn и
  без tokio; оверлей рендерит через glutin+glutin-winit+egui_glow+egui-winit, content-protected
  и click-through через winit-атрибуты напрямую; таймеры тоста качает toast-поток
  (`about_to_wait`); контракт хоста из v1 (host-driven repaint/time) отменён
- 2026-11-04T15:02:00+03:00 -- v3 по фактам Stage 1–2: winit 0.30.13 допускает ровно один
  `EventLoop` на процесс (event_loop.rs:118, global flag) — оверлей-поток владеет loop'ом
  только в standalone-процессах; для хоста (virshot/eframe) добавляется host-бэкенд
  `OverlayHost` (egui multi-viewport поверх `egui::Context`, `ViewportCommand::ContentProtected`,
  passthrough, рисуется в logic-pass хоста); таймер-поток тоста — обычный std-поток с
  `recv_timeout`, без winit; unsafe: `deny(unsafe_code)` + scoped `allow` в `gl_boot.rs`
  (glutin unsafe-only); workspace-факты: gamepad — участник через path-dep, hidpad-poc —
  пустой `[workspace]`; Linux-верификация ограничена sandbox'ом (stall в winit X11-construct под
  xvfb), Windows-верификация — пользователем
- 2026-11-04T17:00:26+03:00 -- v4 по фактам Stage 3: `dismiss()` без id (latest-wins ⇒ один
  текущий тост), `update` ≡ `show`; monitors — снапшот, инжектируется при `ToastService::spawn`
  (хост передаёт логические точки: xcap physical ÷ pixels_per_point); fade — 25ms redraw-тики
  (`FADE_TICK_MS=25`), тост сам толкает промежуточные alpha в sink; `wake: Option<egui::Context>`
  (`request_repaint` после каждого forwarded-действия); `ToastSink` реализован напрямую на
  `OverlayHandle`/`OverlayHostHandle`; `IMAGE_CHIP_SIZE=160`, `MAX_IMAGE_BYTES=4MiB`
- 2026-11-04T19:00:41+03:00 -- v5 по фактам Stage 4: `OverlayHost`+`ToastService` живут в
  `SettingsApp` (создаются в eframe create-closure, monitors — логические точки xcap/pp,
  снапшот на старте); handle в trigger-треды через `capture::ToastHandoff`
  (`Arc<Mutex<Option<ToastHandle>>>`, latest-only — trigger-треды стартуют до gui::run);
  handle — отдельный параметр `run_capture`, не поле `CaptureCommand`; headless CLI —
  `toast=None`, notification skip-лог, host/service не создаются; `src/overlay.rs` удалён;
  positive-доказательство — chip window + пиксели + INFO "Toast service started";
  resident-тост verified на Linux Xvfb (placement ровно по insets)
- 2026-11-04T19:20:30+03:00 -- v6 (Stage 5, docs-truth): AGENTS.md — workspace-факты
  (`crates/gamepad` — член workspace через path-dep, `exclude` не вето; `hidpad-poc`
  пустой `[workspace]`), unsafe-исключение (`overlay` deny + allow в `gl_boot.rs`,
  `toast`/virshot forbid), тред-модель (один winit EventLoop на процесс); два
  pre-existing lints в тестах исправлены; `clippy --workspace --all-targets` зелёный;
  Linux release 14.2MB (Windows-бюджет §5 — на Windows-хосте)
- 2026-11-04T19:36:30+03:00 -- v7: Windows-приёмка (user run) выявила 3 дефекта оверлея/тоста
  — GL-контекст в overlay_demo не создаётся (0xC007000D), содержимое chip невидимо (только
  тень; в toast_debug и virshot — одна причина в библиотеке), «all monitors» не показывает
  на всех мониторах; плюс косметика — чёрный фон debug-окна. Доработки того же task: Stage 6
  в plan.md. Также user правка: gamepad — явный член workspace (коммит chore 81c03ad).
- 2026-11-04T23:39:35+03:00 -- v8: Windows-приёмка после Stage 6: тосты работают и не
  попадают в скриншоты ✓. Две новые доработки того же task (Stage 7): chip'ы/оверлеи
  появляются в панели задач (надо скрыть); на мониторах 2/3 chip'ы создаются (3 окна в
  taskbar), но показываются неверно (белые градиентные окна/чёрная полоса вместо плашки в
  углу) — units/DPI child viewport на Windows + отладочный вывод в консоль (разрешён user).
- 2026-11-05T09:32:12+03:00 -- v9: доработки того же task (Stage 8): перечисление мониторов
  выносится из toast_debug::host_monitors в crates/overlay/src/monitors.rs как
  переиспользуемая часть crate overlay; вывод добавляет primary-флаг, и при снятой галочке
  «все мониторы» chip ставится на основной монитор (сейчас ставится на первый в перечислении
  winit, который на машине user не основной); gui.rs virshot использует тот же модуль вместо
  собственного дублирующего кода.
