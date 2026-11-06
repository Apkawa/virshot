# План: правки окна настроек

Stage 0 — артефакты этого трека (research.md, spec.md v1, plan.md): не требует кода, коммитом не фиксируется (`.dumbspec/` — рабочий артефакт трека; коммит артефактов задачи делается в финальной стадии по решению пользователя).

## Stage 1 — Рамка окна и нижние кнопки (Apply / OK / Cancel) — ✓ done, commit `983e252`

**Files:** `src/gui.rs`, `src/gui.tests.rs`

**Approach:**
- `gui::run` (`:1296-1312`): `ViewportBuilder::new().with_minimize_button(false).with_maximize_button(false)` — `with_close_button` не отключать (крест = hide-to-tray), `resizable` не отключать.
- Нижняя строка (`:1176-1184`): убрать `Apply` из левого края; `ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), …)` — порядок справа налево `Cancel`, `OK`, `Apply` (в right_to_left добавляем в обратном порядке либо `ui.horizontal` с `ui.with_layout`), индикатор "Unsaved changes" слева.
- Чистые хелперы (TDD): `pub(crate) fn can_save(draft, applied, duplicates: &[Action]) -> bool` и `pub(crate) enum BottomAction { Apply, Ok, Cancel }` + `fn bottom_actions(draft, applied) -> (bool save_enabled, bool close_enabled)` — чтобы семантика была тестируемой без egui.
- `OK` = `apply()` + `ctx.send(ViewportCommand::Close)`; при `hide_to_tray` закрытие должно вести себя как существующий выход (exit path `:1111-1112`), а не как `CancelClose` (`:1119-1126`) — закрытие по кнопке отличается от закрытия по кресту (флаг/счётчик в `logic`).
- `Cancel` = `draft = applied.clone()` + `exit_capture()` + закрытие; несохранённый черновик не публикуется.

**Verify:** `./scripts/check.sh`; `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`; `./scripts/ui-verify.sh` (скриншот с тремя кнопками справа внизу). Кнопки min/max на Linux проверить нельзя (winit реализует EnabledButtons только на Windows) — фиксируется как deferred в docs (§9).

**Commit:** `feat(gui): bottom-right Apply/OK/Cancel and close-only window buttons`

## Stage 2 — Hotkeys: таблица, иконка слева, Esc/Backspace подсказка — ✓ done, commit `2a7c66c`

**Files:** `src/gui.rs`, `src/gui.tests.rs`

**Approach:**
- Один `egui::Grid::new("hotkeys_grid")` на весь таб вместо per-row Grid (`:917-919`): колонки name | bind key | pad, строки FullScreen/Window/RectArea; ширина кнопок `BIND_BUTTON_MIN_WIDTH`.
- Иконка статуса — в первой колонке перед именем/кнопкой: `ui.horizontal` внутри ячейки с `icon.icon()` сначала, затем кнопка бинда; `on_disabled_hover_text` для disabled.
- Подсказка в начале формы: `ui.label("Esc to cancel, Backspace to clear")` (или `ui.set_tooltip` на строке захвата) — текст английский.
- `binding_from_press` (`:373-388`) остаётся; в capture-loop (`:766-782`) добавить ветку: `Event::Key(Key::Backspace)` → `clear_binding(action)` (key=None, mods=vec![], pad остаётся) + `exit_capture()` + log; `Key::Escape` → отмена (без изменения бинда).
- Чистый хелпер `pub(crate) fn clear_binding(binding: &mut Binding) -> Binding` для TDD.

**Verify:** `./scripts/check.sh` (новые тесты: иконка слева через pure helper порядка элементов, если achievable; иначе — интеграционный тест на `clear_binding` + `binding_from_press`); ui-verify скриншот hotkeys-таба; Windows cross-compile.

**Commit:** `feat(gui): hotkeys table alignment, status icon left, Backspace clears a binding`

## Stage 3 — Дубликаты биндов: предупреждение и запрет сохранения — ✓ done, commit `3e5e278`

**Files:** `src/gui.rs`, `src/gui.tests.rs`, (нормализация бинда — `src/bindings.rs`, если нужна публичная форма)

**Approach:**
- `pub(crate) fn duplicate_actions(draft: &Controls) -> Vec<(Action, String)>` в `src/gui.rs`: сравнение нормализованных key-биндов (`key + mods` в канонической строке) и pad-строки между FullScreen/Window/RectArea; `rect` исключён. Возвращает пары + description строку для tooltip.
- Warning-иконка (`IconKind::Warning`) на каждой дублированной строке, tooltip = `"{action} duplicates {other}: {binding}"`; `Apply`/`OK` disabled (или apply-вызов не выполняется), "Unsaved changes" остаётся.
- Проверка считается на каждой итерации UI (cheap: 3 действия), до `config::save`.

**Verify:** TDD-тесты: `duplicate_actions_detects_same_key_with_same_mods`, `duplicate_actions_ignores_rect_placeholder`, `duplicate_actions_detects_pad_duplicate`, `duplicate_actions_allows_different_mods`; `./scripts/check.sh`; ui-verify скриншот с warning.

**Commit:** `feat(gui): reject duplicate hotkey and pad bindings`

## Stage 4 — Расширенный набор клавиш для бинда — ✓ done, commit `0680c54`

**Files:** `src/bindings.rs`, `src/bindings.tests.rs`, `src/gui.rs`, `src/gui.tests.rs`

**Approach:**
- Внутри стадии — глубокая проверка: пересечь `egui::Key::VARIANTS` + `egui::Key::name()` (`egui-0.37.0/key.rs`, ключи, реально доставляемые `egui-winit` через `key_from_key_code`/`key_from_named_key`) с таблицей Windows `key_to_vk` (`global-hotkey-0.9.0/src/key.rs:83-237`). Отбросить `Escape`, `Backspace` (зарезервированы), modifier-keys-as-main-key и `Fn/FnLock/Eject` (не маппятся), lock-keys помечить как рискованные (consume system key) — решение фиксируется в коммите.
- `SUPPORTED_KEY_NAMES`: расширенный список; имена = `Code::to_string()` (`Insert/Delete/Home/End/PageUp/PageDown/ArrowDown/...`), поэтому нужен `egui_name_to_config_name(name)` mapping для стрелок (`Down→ArrowDown`) и для ключей, где имена расходятся.
- `parse_key`: тот же набор + rejection `escape`/`backspace` (новый `Error::ReservedKey { action, key }` или `UnknownKey` — выбрать один, с тестом на сообщение).
- Capture-loop в `gui.rs`: расширенный whitelist + `repeat:false` filter остаётся; учёт: `Ctrl+Insert`/`Shift+Insert`/`Shift+Delete` приходят как clipboard events и не захватываются (документируется в tooltip/spec §9); numpad digits → main-row digit notation.

**Verify:** TDD: `parse_key_accepts_navigation_keys`, `parse_key_rejects_reserved_escape_and_backspace`, `supported_key_names_all_parse_as_bindings` (расширенный cross-check), `egui_key_names_all_parse_as_bindings` (mapping таб), `parse_key_arrow_names_round_trip`; `./scripts/check.sh`; Windows cross-compile + `./cargo.sh test --workspace`.

**Commit:** `feat(bindings): extend bindable key set to the registrable global-hotkey codes`

## Stage 5 — Человекочитаемые pad-имена — ✓ done, commit `f27ea78`

**Files:** `crates/gamepad/src/lib.rs`, `crates/gamepad/src/gameinput.rs`, `src/gamepad.rs`, `src/gamepad.tests.rs`, `src/bindings.rs`, `src/bindings.tests.rs`, `src/gui.rs`, `src/gui.tests.rs`, `src/config.rs` (doc)

**Approach:**
- `crates/gamepad`: публичный доступ к labels устройства — `pub fn controller_button_count(device_index) -> Option<usize>` и/или `pub fn controller_button_labels(device_index) -> Option<Vec<Option<&'static str>>>` поверх `controller_label_v3/v2` (`gameinput.rs:1947-2004`); v0 и non-Windows → `None`. Строки — `button_name(label)` (`lib.rs:176-291`). Никаких новых deps, `#![forbid(unsafe_code)]`/существующий FFI-модуль не расширяем.
- `src/gamepad.rs`: `pad_notation(event)` → для `Controller` имя из `button_label(device,index)` (если есть), иначе `unknown:<N>`; `Guide`/`Share` как сейчас. Резолв имя→индекс при парсинге конфига: `fn pad_index_for_name(name) -> Option<u8>` — обход labels подключённых устройств (`AppObserver.devices`), первое совпадение; `unknown:<N>` → прямой индекс. Матчинг остаётся bit-index (`match_controller :534-540`).
- `src/bindings.rs`: `parse_pad` принимает canonical имена (таблица `button_name`-значений → резолв делегируется в `gamepad` через callback/функцию, чтобы `bindings` не знал про device-info — решение: `parse_pad` возвращает `PadBind::Named(String)`, матчер резолвит; либо `parse_pad` принимает `dyn Fn(&str)->Option<u8>`). Выбор: `PadBind::{Controller(index), Named(name), Guide, Share}` с резолвом в `pad_triggers` (`src/gamepad.rs:337-350`) — нотация остаётся строкой в конфиге.
- `src/gui.rs`: `pad_field_label` показывает имя (или hint из device-info при захвате); `drain_pad_capture` пишет имя; дубликаты pad-биндов сверяются по строке (Stage 3).
- Обратная совместимость: `unknown:<N>` продолжает парситься; 12 gilrs-имён остаются.

**Verify:** TDD: `pad_notation_prefers_device_label_over_raw_index` (fixture device-info через инъекцию labels — pure helper, без COM), `pad_notation_falls_back_to_unknown_index_without_label`, `parse_pad_accepts_xbox_names`, `parse_pad_still_accepts_unknown_index`, `pad_index_for_name_resolves_first_matching_label`; `crates/gamepad` inline-тесты нового API (non-Windows stub → None); `./scripts/check.sh` + `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` + `cd crates/gamepad && ../../cargo.sh test`. Hardware acceptance deferred (Windows, spec §9).

**Commit:** `feat(gamepad): human-readable pad bindings from device-info labels`

## Stage 6 — Справочный accordion шаблонов (General) — ✓ done, commit `4f24011`

**Files:** `src/template.rs`, `src/template.tests.rs`, `src/gui.rs`, `src/gui.tests.rs`

**Approach:**
- `src/template.rs`: `pub(crate) const TOKENS` (или `pub(crate) fn token_reference() -> &[(&str, &str)]` с коротким описанием) — единственный источник списка; UI его использует, не дублирует.
- `src/gui.rs` `general_tab`: `egui::CollapsingHeader::new("Template reference")` под полем Window template (egui 0.37 не имеет CollapsibleGroup); строки `%Y — 4-digit year`, `%y — 2-digit year`, `%mo`, `%d`, `%H`, `%M`, `%S`, `%iso — "YYYY-MM-DD HH:MM:SS"`, `%t — window title`, `%n — counter on name collision`, `%ext — extension without dot, eg 'png'`. Пример строки: `%t -- window title`.
- Тест синхронности: `token_reference_matches_template_tokens` (список UI = `TOKENS`), плюс дефолтные шаблоны цитируются из кода (`config::default`), не из docs.

**Verify:** TDD тест синхронности; `./scripts/check.sh`; ui-verify скриншот general-таба с раскрытым accordion.

**Commit:** `feat(gui): template reference accordion under the window template field`

## Stage 7 — Кнопка теста оверлей-чипа — ✓ done, commit `9899304`

**Files:** `src/gui.rs`, `src/gui.tests.rs`

**Approach:**
- В `overlay_tab` (`:1045-1078`) рядом с чекбокс "Show overlay toast" — кнопка "Test overlay" (`ui.horizontal` в той же строке), `enabled = draft.overlay.enabled` + `on_disabled_hover_text("Enable the overlay toast to test it")`.
- По клику: собрать `toast::Toast { monitor: All/Index(0), text: "Saved: test.png", image: None, color: ToastColor::Success, corner: from draft.position, duration_ms: clamp(draft.duration_ms) }` и `capture::handoff_handle(&self.handoff).map(|h| h.show(toast))`; show-ошибку логировать (WARN), не паниковать. Чип рисуется на следующем `overlay_host.update(ctx)` (`:1140`).
- Чистый хелпер `pub(crate) fn test_overlay_toast(overlay: &Overlay) -> toast::Toast` — TDD (corner/monitor/duration mapping из draft), чтобы не duplicate private `capture::*` логику: mapping переносится/воспроизводится в `gui.rs` (capture helpers приватны).
- Никаких правок `crates/toast`/`crates/overlay`.

**Verify:** TDD: `test_overlay_toast_maps_position_monitor_and_duration`, `test_overlay_toast_is_disabled_when_overlay_unchecked` (pure predicate); `./scripts/check.sh`; ui-verify (`shot` уже ищет чип-окно; добавить скриншот overlay-таба).

**Commit:** `feat(gui): overlay test button next to the toast checkbox`

## Stage 8 — Docs и финальные гейты — ✓ done, commit `d2cef8a` (выполнен последним, после Stage 13)

**Files:** `docs/spec.md`, `.dumbspec` артефакты (по решению пользователя)

**Approach:**
- §4.1: pad-нотация — canonical имена по умолчанию (из device-info), `unknown:<N>` fallback и обратная совместимость; §4.4: кнопки Apply/OK/Cancel, окно без min/max, подсказка Esc/Backspace, дубликаты не сохраняются, accordion шаблонов, кнопка теста оверлея; §2: расширенный набор клавиш; §9: лимиты (EnabledButtons только Windows; clipboard-chords не захватываются; Super недоступен как модификатор в захвате; pad-имена device-dependent; Windows hardware acceptance deferred).
- Финальные гейты: `./scripts/check.sh` (unit + e2e с display), `./scripts/check_full.sh`, `./cargo.sh test --workspace`, `./cargo.sh build --release --target x86_64-pc-windows-gnu`, `./scripts/ui-verify.sh` (скриншоты всех трёх таба + чип оверлея; временные файлы в `./.tmp/`).

**Commit:** `docs(spec): settings window buttons, key set, pad notation, overlay test`

---

## Stage 9 — Research: почему PrintScreen не биндится — ✓ done (агент `7a75c9ea`, findings в `research.md`)

**Criterion:** verdict per layer с file:line; слой захвата и слой триггера чистые, отказ — внешний grab (Linux) и порядок регистрации на старте (Windows).
**Commit:** N/A (research)

## Stage 10 — Иконка предупреждения привязана к своей кнопке — ✓ done, commit `a71516e`

**Files:** `src/gui.rs`, `src/gui.tests.rs`

**Approach:** клавиатурная иконка — в ячейке кнопки бинда непосредственно перед кнопкой; геймпадная иконка — в ячейке поля геймпада перед полем. Иконка относится только к своей кнопке: warning по клавиатуре не показывается у поля геймпада и наоборот (нужен split статуса: keyboard vs pad). Подсказка остаётся на иконке.

**Verify:** TDD (`warning_slot_for_keyboard_is_before_the_bind_button`, `warning_slot_for_pad_is_before_the_pad_field`, `pad_warning_only_for_pad_problem_not_for_keyboard`); `./scripts/check.sh`; Xvfb-скриншот вкладки Hotkeys с warning по клавиатуре и по геймпаду.

**Commit:** `feat(gui): warning icons bound to their own control`

## Stage 11 — Блок кнопок прижат к нижнему краю — ✓ done, commit `f60bda9`

**Files:** `src/gui.rs`, `src/gui.tests.rs`

**Approach:** нижняя строка Apply/OK/Cancel в `egui::Panel::bottom` (отступ от края окна убирается), `Unsaved changes` слева в том же блоке; поведение кнопок не меняется.

**Verify:** `./scripts/check.sh`; Xvfb-скриншот: блок у нижней границы окна.

**Commit:** `feat(gui): dock the button row to the bottom edge`

## Stage 12 — Esc/Backspace для поля геймпада — ✓ done, commit `afa61c2`

**Files:** `src/gui.rs`, `src/gui.tests.rs`, `src/gamepad.rs` (если нужен сброс `pad`)

**Approach:** активный захват поля геймпада отменяется по `Esc` (значение не меняется), `Backspace` очищает `pad` этой строки (→ `None`), `key`/`mods` не трогает; подсказка в начале формы покрывает оба поля.

**Verify:** TDD (`clear_pad_binding_removes_pad_but_keeps_key`, `cancel_pad_capture_leaves_pad_untouched`); `./scripts/check.sh`; Xvfb-скриншот (pad-поле на Linux — placeholder, поэтому UI-доказательство = состояние захвата/подсказка, честная пометка).

**Commit:** `feat(gui): Esc cancels and Backspace clears the pad binding`

## Stage 13 — PrintScreen: пере-регистрация при фокусе окна + читаемая подсказка — ✓ done, commit `a3fea3d`

**Files:** `src/main.rs`, `src/hotkeys.rs`, `src/gui.rs`, `src/gui.tests.rs`, `src/hotkeys.tests.rs` (sibling, если нужен)

**Approach:** стартовая регистрация остаётся, но провалившиеся комбо пере-пробуются, когда окно настроек получило фокус (`ctx.viewport().is_focused()` transition в `logic`) — на Windows это documented foreground override для OS-хоткея PrintScreen; успех логируется INFO, failure остаётся warning на строке. Причина отказа — читаемый текст вместо raw Debug (`HotKey already registered: HotKey { ... }`), с указанием что клавишу держит другой процесс.

**Verify:** `./scripts/check.sh`; Xvfb: под WM с `Print→scrot` строка показывает warning с читаемой причиной; под keybinding-free WM регистрация успешна (скриншоты обоих случаев). Windows-runtime acceptance deferred (`docs/spec.md §9`).

**Commit:** `feat(hotkeys): retry failed registrations on window focus, readable failure reason`

## Stage 14 — Временная хоткей-проба для проблемных клавиш (Windows) — ✓ done, commit `2f841ed`

**Files:** `src/hotkeys.rs`, `src/hotkeys.tests.rs`, `src/gui.rs`, `src/gui.tests.rs`, `src/bindings.rs`, `src/bindings.tests.rs`

**Approach:** пока захват клавиши активен (live-хоткеи уже paused), дополнительно регистрируются комбы проблемных клавиш из расширяемого списка (`PrintScreen`, `ScrollLock`, `Pause`): bare + Alt/Ctrl/Shift + Alt+Shift/Ctrl+Shift (`Win` недостижим в захвате). Слушатель (`spawn_listener`) различает пробные комбо по id из общего слота проб и вместо скриншота пишет нажатие (`Code` + `Mods`) в этот слот, будя eframe через `request_repaint` (слот — аналог `ToastHandoff`, `Arc<Mutex<...>>`, attach из `gui::run`). `SettingsApp` в `logic` забирает нажатие (`take_probe_press`) и коммитит его как обычное нажатие: `key`/`mods` строки, `pad` остаётся, capture завершается, стопы проб выполняются перед `restore()`. Проба — только `cfg(windows)`; на Linux клавиши приходят через egui, проб не создаётся. Ошибки регистрации проб — WARN, без краша.

**Verify:** TDD: `probe_combos_are_derivable_and_do_not_collide_with_action_ids`, `probe_press_is_recorded_instead_of_firing_capture`, `take_probe_press_returns_one_press_then_empty`, `probe_binding_keeps_pad_and_replaces_key_and_mods`, `scroll_lock_name_round_trips_through_parse_key`; `./scripts/check.sh`; `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`. Linux UI-доказательство: проб нет → поведение захвата PrintScreen неизменно под Xvfb (no-op path). Windows-runtime acceptance (реальный PrintScreen биндится) — deferred, пользователь проверяет на машине.

**Commit:** `feat(hotkeys): temporary probe registrations for keys Windows never delivers`

## Stage 15 — docs/spec.md: обход проблемных клавиш (v3) — ✓ done, commit `34af62d`

**Files:** `docs/spec.md`

**Approach:** §4.4 — описание временной хоткей-пробы на Windows (PrintScreen/ScrollLock/Pause, список расширяемый, Snipping Tool перехватывается только на время захвата), §4.1/§9 — причина (Windows не отправляет `WM_CHAR` для `VK_SNAPSHOT`, winit откладывает keydown до CHAR и теряет его), ограничение: проба только на Windows, на Linux клавиши доставляются обычно; ERR/WARN-логирование пробных отказов в §8 при необходимости.

**Verify:** `./scripts/check.sh` (docs-only); проверка, что текст не противоречит §2/§4.4 о pause/restore.

**Commit:** `docs(spec): document the Windows probe workaround for undelivered keys`

## Stage 16 — косметика v4: hint показывается всегда — ✓ done, commit `0f5bcc8`

**Files:** `src/gui.rs`, `src/gui.tests.rs`, `docs/spec.md`

**Approach:** `CAPTURE_HINT` -> `BIND_HINT`, unconditional `ui.label(BIND_HINT)` в `hotkeys_tab` (§4.4 v8). Никакой gating-логики не осталось, поэтому отдельного unit-теста на видимость нет — существующий тест пинит английский текст (`bind_hint_mentions_escape_and_backspace`).

**Verify:** `./scripts/check.sh` (green); Xvfb screenshot of the Hotkeys tab with no capture active shows the hint (`./.tmp/ui-verify/hint-always/shots/02-hotkeys.png`).

**Commit:** `fix(gui): keep the bind hint visible in the Hotkeys tab`
