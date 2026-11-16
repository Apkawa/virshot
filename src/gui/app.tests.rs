use super::*;

use crate::config::Binding;

use std::sync::MutexGuard;

/// Take the crate-wide locale lock (`crate::LOCALE_LOCK`): the locale and its
/// cache version are process-global, so locale-touching tests in every file
/// serialize on this one lock, not on a per-file one. A poisoned lock (a panic
/// in another locale test) is recovered rather than unwrapped, like every
/// other mailbox in this crate.
fn locale_guard() -> MutexGuard<'static, ()> {
    crate::LOCALE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Assert a rendered UI string goes through the catalog `key` AND the
/// catalog's `en` value equals the shipped English contract byte-for-byte.
/// Callers must hold `locale_guard()` and pin `"en"` first — this helper
/// calls `t!` itself. Tests that never leave `en` need no further restore:
/// `crate::LOCALE_LOCK` serializes locale mutations process-wide, so no other
/// test changes the locale while this test holds it.
fn assert_en_catalog(key: &str, rendered: &str, contract: &str) {
    assert_eq!(
        rendered,
        rust_i18n::t!(key).as_ref(),
        "the rendered text must come from catalog key `{key}`"
    );
    assert_eq!(
        rust_i18n::t!(key).as_ref(),
        contract,
        "catalog `en` value for `{key}` must equal the shipped English wording"
    );
}

/// Hotkey-row fixture: one `Binding` exactly as a draft row would hold it.
fn binding(key: &str, mods: &[&str], pad: Option<&str>) -> Binding {
    Binding {
        key: key.to_owned(),
        mods: mods.iter().copied().map(str::to_owned).collect(),
        pad: pad.map(str::to_owned),
    }
}

#[test]
fn bottom_buttons_save_disabled_while_bindings_duplicate() {
    let mut draft = Settings::default();
    draft.controls.window = binding("PrintScreen", &[], None);
    assert!(chrome::is_dirty(&draft, &Settings::default()));
    assert!(!binding::duplicate_bindings(&draft).is_empty());
    // The Stage-3 gate composes uniqueness into `savable`; Cancel stays free.
    let savable = binding::draft_savable(&draft) && binding::duplicate_bindings(&draft).is_empty();
    assert!(!savable, "a duplicated draft must not be savable");
    let blocked = chrome::bottom_buttons_enabled(true, savable);
    assert!(
        !blocked.apply_enabled,
        "Apply must be disabled for a duplicate"
    );
    assert!(!blocked.ok_enabled, "OK must be disabled for a duplicate");
    assert!(
        blocked.cancel_enabled,
        "Cancel stays enabled while a duplicate blocks saving"
    );
}

#[test]
fn tab_labels_are_english() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Each tab header renders through its catalog key; the en value pins the
    // shipped English wording byte-for-byte.
    assert_en_catalog("general.tab_general", &Tab::General.label(), "General");
    assert_en_catalog("general.tab_hotkeys", &Tab::Hotkeys.label(), "Hotkeys");
    assert_en_catalog("general.tab_overlay", &Tab::Overlay.label(), "Overlay");
}

/// Stage 3 (spec §2/§7): under the `ru` locale the tab headers render the
/// Russian catalog values. `ru` is restored BEFORE the assertions so a
/// failure never leaves it installed for other tests.
#[test]
fn tab_labels_follow_russian_locale() {
    let _guard = locale_guard();
    rust_i18n::set_locale("ru");

    let labels = [
        Tab::General.label(),
        Tab::Hotkeys.label(),
        Tab::Overlay.label(),
    ];

    rust_i18n::set_locale("en");

    assert_eq!(
        labels,
        [
            "Общие".to_owned(),
            "Горячие клавиши".to_owned(),
            "Оверлей".to_owned(),
        ],
        "tab headers must render Russian under the ru locale"
    );
}

// ── Child-viewport repaint throttling (task 2026-11-13_gpu-repaint-throttle,
// Stage 3) ──
//
// The assertions read the EFFECTIVE schedule — `FullOutput::viewport_output[id]
// .repaint_delay` after a pass — because that is what the backend acts on.
// `Context::set_repaint_observer` also sees deduplicated requests, so it would
// report a throttled request that egui threw away; the pass output cannot.
//
// egui 0.37 facts used here, from the locked source, not from memory:
// - a pass with no outstanding request puts `repaint_delay` back to
//   `Duration::MAX` (`context.rs:102-127`), so the CHILD paint pass is what
//   clears the previous schedule and lets the next logic pass land a request;
// - `Duration::ZERO` sets `outstanding = 1` and keeps the delay at ZERO, and a
//   delayed request only lands when it is STRICTLY shorter than the current
//   delay (`context.rs:133-172`) — so an immediate pin swallows the throttled
//   request for the next couple of passes, which is why every steady assertion
//   below drives past the settling window instead of asserting on pass one;
// - opening a Deferred viewport pins it to ZERO once, from egui's own
//   `send_viewport_cmd_to` (`context.rs:4569`), not from app code. That pin is
//   correct: the first paint of a new window must be immediate.

/// The interval the app throttles static-content children to
/// (`CHILD_VIEWPORT_REPAINT_INTERVAL`). Pinned as a literal here, mirroring
/// `crates/overlay/src/host.rs`: a silent change to the const must fail a test.
const CHILD_INTERVAL: Duration = Duration::from_millis(200);

/// Number of cycles needed to get past the open-time pin described above: the
/// pin costs two repaints (`context.rs:133-142`), so the third cycle is the
/// first one that reports the steady schedule.
const SETTLE_CYCLES: usize = 3;

/// An app wired exactly like `gui::run`, minus everything that needs a real
/// display, tray, or network: no hotkey controller, no tray, no check channel
/// traffic, an overlay host with no monitor rects (chips are a logged no-op),
/// and a toast service with `wake = None` (nothing wakes this context but the
/// app itself, so the schedules below are the app's doing alone). `startup_hidden`
/// mirrors the app's ROOT suppression (always on under spec v3, kept as a knob so
/// the pre-existing suppression tests still exercise both states); the settings
/// CHILD reveal flag is its inverse, mirroring `gui::run`'s `open_settings_on_start`.
fn test_app(startup_hidden: bool) -> SettingsApp {
    let (update_tx, update_rx) = mpsc::channel::<crate::update::CheckOutcome>();
    let host = OverlayHost::new();
    let service = ToastService::spawn(Arc::new(host.handle()), Vec::new(), None);
    SettingsApp::new(
        PathBuf::from("virshot-test-config.toml"),
        Settings::default(),
        None,
        Vec::new(),
        service,
        host,
        crate::capture::new_handoff(),
        None,
        Arc::new(Mutex::new(Settings::default())),
        crate::gamepad::new_capture_mailbox(),
        update_rx,
        update_tx,
        Arc::new(AtomicBool::new(false)),
        startup_hidden,
        !startup_hidden,
    )
}

/// The repaint-schedule harness app: suppression off (so the ROOT suppression
/// never fires in these tests) and first-run reveal on, exactly what `gui::run`
/// passes when this run created the default config.
fn repaint_test_app() -> SettingsApp {
    test_app(false)
}

/// The settings child viewport id (spec v3: the settings UI lives in a CHILD
/// viewport, not the root). The name is pinned as a literal here, mirroring
/// `CHILD_INTERVAL`: a silent rename of the implementation's viewport id must
/// fail the settings-child tests below, not silently pass on some other id.
fn settings_child_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("virshot-settings")
}

/// One logic-only pass, exactly how eframe runs a HIDDEN root window
/// (`eframe native/epi_integration.rs::update_logic_only` → `Context::run_logic`:
/// no ui pass, so `logic` runs alone — the shape a startup-hidden process gets).
/// Returns the FULL `LogicOutput`: `viewport_commands` is keyed by viewport id,
/// so a command sent with `Context::send_viewport_cmd_to(child, …)` lands under
/// the CHILD id, not ROOT — that keying is the observable shape of the spec-v3
/// semantics (`egui-0.37.0/src/context.rs:1193-1199` hands every viewport's
/// queued commands out and takes them).
fn logic_pass(
    ctx: &egui::Context,
    app: &mut SettingsApp,
    input: &egui::RawInput,
) -> egui::LogicOutput {
    ctx.run_logic(input, |ctx| {
        eframe::App::logic(app, ctx, &mut eframe::Frame::_new_kittest())
    })
}

/// The commands one pass emitted for `viewport`: empty when the pass sent
/// nothing to it — and, under kittest, when the app never commanded (or never
/// registered) that viewport at all.
fn commands_for(
    output: &egui::LogicOutput,
    viewport: egui::ViewportId,
) -> Vec<egui::ViewportCommand> {
    output
        .viewport_commands
        .get(&viewport)
        .cloned()
        .unwrap_or_default()
}

/// Raw input reporting the ROOT window as hidden (minimized): the state the
/// chip recipe's root forcing (`crates/overlay/src/host.rs:529-542`) is written
/// for — `RequestPaintWhileHidden` to ROOT fires only while the root REPORTS
/// hidden (`ViewportInfo::visible() == Some(false)`, derived from
/// `minimized`/`occluded`, `egui-0.37.0/src/data/input/viewport_info.rs:98-104`).
/// kittest creates no OS windows, so the report is simulated here; the tests
/// assert on the commands, never on window existence.
fn raw_root_hidden() -> egui::RawInput {
    let mut input = egui::RawInput::default();
    input.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            minimized: Some(true),
            ..Default::default()
        },
    );
    input
}

/// Run one FULL pass (the shape eframe runs once `RequestPaintWhileHidden` has
/// been asked for) and take the settings child's Deferred paint callback — the
/// same registration `register_child` reads for About/update, with a message
/// naming the settings viewport: a registered settings child is exactly the
/// spec-v3 seam these tests demand.
fn settings_callback(
    ctx: &egui::Context,
    app: &mut SettingsApp,
    settings: egui::ViewportId,
) -> Arc<egui::DeferredViewportUiCallback> {
    let output = ctx.run_ui(egui::RawInput::default(), |_ui| {
        eframe::App::logic(app, ctx, &mut eframe::Frame::_new_kittest())
    });
    let callback = output
        .viewport_output
        .get(&settings)
        .and_then(|viewport| viewport.viewport_ui_cb.clone());
    // Consume the pass output BEFORE the assertion: `TexturesDelta::drop`
    // debug-asserts on unapplied deltas, and panicking while the output is
    // alive would unwind into that destructor and abort the test process.
    output.drop_without_applying_deltas();
    callback.expect("the settings window must live in a registered CHILD viewport")
}

/// The visibility commands out of one pass output — the commands this unit is
/// about, with every other viewport command filtered away so an assertion
/// reports exactly what the window visibility did.
fn visibility_commands(commands: &[egui::ViewportCommand]) -> Vec<egui::ViewportCommand> {
    commands
        .iter()
        .filter(|command| matches!(command, egui::ViewportCommand::Visible(_)))
        .cloned()
        .collect()
}

/// Run one root logic pass and report the schedules it produced:
/// `(child, root)`. The pass runs `App::logic` INSIDE the ui pass the way
/// eframe runs a VISIBLE window (`eframe epi_integration`), so the requests it
/// makes are in-pass and become the effective schedule.
/// `eframe::Frame::_new_kittest()` is the headless frame — this app's `logic`
/// never touches it. The output is consumed here: `TexturesDelta::drop`
/// debug-asserts on unapplied deltas (`epaint-0.37.0/src/textures.rs:333-339`),
/// so every pass output must be dropped through `drop_without_applying_deltas`.
fn pass_schedules(
    ctx: &egui::Context,
    app: &mut SettingsApp,
    input: &egui::RawInput,
    child: egui::ViewportId,
) -> (Duration, Duration) {
    let output = ctx.run_ui(input.clone(), |_ui| {
        eframe::App::logic(app, ctx, &mut eframe::Frame::_new_kittest())
    });
    let child_delay = output.viewport_output[&child].repaint_delay;
    let root_delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
    output.drop_without_applying_deltas();
    (child_delay, root_delay)
}

/// Raw input reporting a child viewport WITH a window (the case the guard in
/// `schedule_child_viewport_repaint` is written for).
fn raw_with_child_window(child: egui::ViewportId) -> egui::RawInput {
    let mut input = egui::RawInput::default();
    input.viewports.insert(child, egui::ViewportInfo::default());
    input
}

/// Raw input reporting a child window the user clicked the X on: the SAME
/// per-viewport info as `raw_with_child_window`, plus the `ViewportEvent::Close`
/// eframe pushes into the CHILD's `ViewportInfo` (`glow_integration.rs:1075`).
/// `RawInput::viewport()` resolves by `viewport_id` (`data/input/raw_input.rs:116-118`),
/// so a CHILD paint pass with this input — `child_pass` sets `viewport_id` to the
/// child — is the only place `ui.input(|i| i.viewport().close_requested())` sees
/// the click, which is exactly what `SettingsUi::paint` records into the shared
/// `settings_close` flag. egui clears `events` through `ViewportInfo::take`, so a
/// close is one-shot per pass; later passes must pass a plain input.
fn raw_with_child_close(child: egui::ViewportId) -> egui::RawInput {
    let mut input = raw_with_child_window(child);
    input
        .viewports
        .get_mut(&child)
        .expect("the child viewport must report its window")
        .events
        .push(egui::ViewportEvent::Close);
    input
}

/// One child paint pass, exactly how eframe runs a Deferred viewport
/// (`glow_integration.rs:1781`: `run_ui` with `RawInput::viewport_id` set to
/// the child and the callback egui stored in `ViewportOutput`). This pass
/// resets the child's schedule to `Duration::MAX`, which is what lets the next
/// logic pass schedule a throttled repaint instead of deduplicating.
fn child_pass(
    ctx: &egui::Context,
    child: egui::ViewportId,
    input: &egui::RawInput,
    callback: &Arc<egui::DeferredViewportUiCallback>,
) {
    let mut input = input.clone();
    input.viewport_id = child;
    let callback = Arc::clone(callback);
    ctx.run_ui(input, move |ui| callback(ui))
        .drop_without_applying_deltas();
}

/// Register a Deferred child viewport and return the paint callback egui hands
/// the backend, plus the raw input that reports its window.
fn register_child(
    ctx: &egui::Context,
    app: &mut SettingsApp,
    child: egui::ViewportId,
) -> (Arc<egui::DeferredViewportUiCallback>, egui::RawInput) {
    let output = ctx.run_ui(egui::RawInput::default(), |_ui| {
        eframe::App::logic(app, ctx, &mut eframe::Frame::_new_kittest())
    });
    let callback = output.viewport_output[&child]
        .viewport_ui_cb
        .clone()
        .expect("the deferred child viewport carries its paint callback");
    output.drop_without_applying_deltas();
    (callback, raw_with_child_window(child))
}

/// One steady cycle for a child viewport: its paint pass clears the schedule,
/// then the root logic pass re-issues it. Returns what that pass scheduled.
fn drive_child_cycle(
    ctx: &egui::Context,
    app: &mut SettingsApp,
    child: egui::ViewportId,
    input: &egui::RawInput,
    callback: &Arc<egui::DeferredViewportUiCallback>,
) -> Duration {
    child_pass(ctx, child, input, callback);
    pass_schedules(ctx, app, input, child).0
}

/// Drive cycles of unchanged content and report the LAST schedule: the steady
/// cadence. `SETTLE_CYCLES` absorbs egui's one-off open-time pin.
fn drive_steady_cycles(
    ctx: &egui::Context,
    app: &mut SettingsApp,
    child: egui::ViewportId,
    input: &egui::RawInput,
    callback: &Arc<egui::DeferredViewportUiCallback>,
    cycles: usize,
) -> Duration {
    let mut scheduled = Duration::ZERO;
    for _ in 0..cycles {
        scheduled = drive_child_cycle(ctx, app, child, input, callback);
    }
    scheduled
}

/// Steady-state cadence for one static-content child viewport: with nothing
/// changing, the logic pass must schedule the interval, never an immediate
/// repaint, and must keep scheduling it cycle after cycle.
fn assert_steady_child_throttled(app: &mut SettingsApp, child: egui::ViewportId) {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);

    let (callback, input) = register_child(&ctx, app, child);

    let settled = drive_steady_cycles(&ctx, app, child, &input, &callback, SETTLE_CYCLES);
    assert_eq!(
        settled, CHILD_INTERVAL,
        "a steady child pass must schedule the throttled interval, not ZERO"
    );

    // The throttle is re-issued every pass — egui keeps only ONE repaint per
    // non-zero delay, so a schedule that stops being re-issued starves the
    // viewport.
    let next = drive_child_cycle(&ctx, app, child, &input, &callback);
    assert_eq!(
        next, CHILD_INTERVAL,
        "the throttle must be re-issued on the next steady pass"
    );
}

#[test]
fn about_child_viewport_steady_pass_throttles_to_interval() {
    let mut app = repaint_test_app();
    app.about_window.select_from_tray();
    assert_steady_child_throttled(&mut app, egui::ViewportId::from_hash_of("virshot-about"));
}

#[test]
fn update_dialog_child_viewport_steady_pass_throttles_to_interval() {
    let mut app = repaint_test_app();
    app.update_dialog.open();
    assert_steady_child_throttled(&mut app, egui::ViewportId::from_hash_of("virshot-update"));
}

/// A child viewport whose window does not exist yet must never be asked to
/// repaint by app code: eframe drops a repaint request for a viewport with no
/// window (`native/run.rs` → `EventResult::Wait`), so the request buys nothing
/// and leaves egui with a `Duration::ZERO` pin that swallows every later
/// throttled request. `set_repaint_observer` is the right instrument here — it
/// sees every request, deduplicated ones included, which is exactly what must
/// not be issued.
#[test]
fn child_viewport_without_window_never_gets_an_app_repaint_request() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let requests: Arc<Mutex<Vec<(egui::ViewportId, Duration)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&requests);
    ctx.set_repaint_observer(Box::new(move |info: egui::RequestRepaintInfo| {
        sink.lock()
            .expect("observer log")
            .push((info.viewport_id, info.delay));
    }));
    let mut app = repaint_test_app();
    app.about_window.select_from_tray();
    let about = egui::ViewportId::from_hash_of("virshot-about");

    // Registration plus two further logic passes, still WITHOUT a window.
    for _ in 0..3 {
        pass_schedules(&ctx, &mut app, &egui::RawInput::default(), about);
    }
    let logged = requests.lock().expect("observer log").clone();
    assert!(
        !logged
            .iter()
            .any(|(viewport, delay)| *viewport == about && *delay == CHILD_INTERVAL),
        "the app must not schedule a throttled repaint for a viewport with no window"
    );

    // Once the window exists, the same scheduling throttles it.
    let (callback, input) = register_child(&ctx, &mut app, about);
    let steady = drive_steady_cycles(&ctx, &mut app, about, &input, &callback, SETTLE_CYCLES);
    assert_eq!(
        steady, CHILD_INTERVAL,
        "a child viewport with a window must be throttled to the interval"
    );
    let logged = requests.lock().expect("observer log").clone();
    assert!(
        logged
            .iter()
            .any(|(viewport, delay)| *viewport == about && *delay == CHILD_INTERVAL),
        "with a window present the throttled request must actually be issued"
    );
}

/// An install result arriving from the install thread is an EVENT, not steady
/// content: the root repaint stays immediate (`:774`), while the dialog child
/// keeps its throttled schedule — content changes in the dialog are reflected
/// on the next child repaint at most one interval later.
#[test]
fn update_dialog_drained_install_result_keeps_root_repaint_immediate() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut app = repaint_test_app();
    app.update_dialog.open();
    let dialog = egui::ViewportId::from_hash_of("virshot-update");
    let (callback, input) = register_child(&ctx, &mut app, dialog);
    assert_eq!(
        drive_steady_cycles(&ctx, &mut app, dialog, &input, &callback, SETTLE_CYCLES),
        CHILD_INTERVAL,
        "the dialog must be in its steady throttled state before the event arrives"
    );

    let (install_tx, install_rx) = mpsc::channel::<Result<(), String>>();
    install_tx
        .send(Err("replace failed in test".to_owned()))
        .expect("the install receiver is still alive");
    app.install_rx = Some(install_rx);

    let (dialog_delay, root_delay) = pass_schedules(&ctx, &mut app, &input, dialog);
    assert_eq!(
        root_delay,
        Duration::ZERO,
        "a drained install result must still repaint the root immediately"
    );
    assert_eq!(
        dialog_delay, CHILD_INTERVAL,
        "the dialog viewport itself stays throttled"
    );
}

/// Release notes arrive from the fetch thread while the dialog is open: that
/// pass repaints the dialog IMMEDIATELY (the old per-pass pin is what made
/// notes prompt), and the NEXT steady pass goes back to the interval — an
/// event-driven nudge must not leave the viewport pinned.
#[test]
fn update_dialog_notes_arrival_repaints_immediately_then_throttles_again() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let mut app = repaint_test_app();
    app.update_dialog.open();
    let dialog = egui::ViewportId::from_hash_of("virshot-update");
    let (callback, input) = register_child(&ctx, &mut app, dialog);
    assert_eq!(
        drive_steady_cycles(&ctx, &mut app, dialog, &input, &callback, SETTLE_CYCLES),
        CHILD_INTERVAL,
        "the dialog must be in its steady throttled state before the notes arrive"
    );

    let (notes_tx, notes_rx) = mpsc::channel::<Result<notes::ReleaseNotes, notes::NotesError>>();
    notes_tx
        .send(Err(notes::NotesError::Parse("no notes in test".to_owned())))
        .expect("the notes receiver is still alive");
    app.notes_rx = Some(notes_rx);

    let (arrival, _) = pass_schedules(&ctx, &mut app, &input, dialog);
    assert_eq!(
        arrival,
        Duration::ZERO,
        "notes arriving must repaint the dialog on the pass that drained them"
    );

    // The ZERO pin costs two repaints (`context.rs:133-142`); once it is spent,
    // the steady throttle resumes — the point of this test is that the nudge
    // does not leave the viewport pinned forever.
    let resumed = drive_steady_cycles(&ctx, &mut app, dialog, &input, &callback, SETTLE_CYCLES);
    assert_eq!(
        resumed, CHILD_INTERVAL,
        "after the notes nudge settles, the dialog must go back to the interval"
    );
}

// ── Settings child viewport (task 2026-11-15_settings-first-start-only, v3) ──
//
// The settings UI moves OUT of the ROOT viewport into a CHILD viewport
// `virshot-settings`, registered per pass while the settings window is open
// (the About pattern, `app.rs:639-684`). The root carries no settings surface
// and never becomes visible; a hidden startup keeps the child hidden (created
// with `.with_visible(false)`), forcing paints on the ROOT with
// `RequestPaintWhileHidden` (chip recipe, `crates/overlay/src/host.rs:529-542`)
// so the hidden child can still paint — and NEVER sends `Visible(true)` to it.
// A tray ShowSettings shows the CHILD (`Visible(true)` once; a later show while
// shown raises it with `Focus`, the About precedent at `app.rs:588-591`). A
// first-run startup opens the child right away, gated on its FIRST paint.
// The ROOT suppression (per-pass `Visible(false)` while `startup_hidden`) stays.
// kittest creates no OS windows, so every assertion reads the per-viewport
// command queues (child commands are keyed by the CHILD id) and the Deferred
// registration callback — never window existence. The child's paint pass runs
// the stored callback exactly as eframe runs a Deferred viewport (`child_pass`),
// which is what records the first paint the reveal gate waits on.

/// A process started hidden to the tray (`gui::run` got `visible: false`, i.e.
/// not a first run): the settings child is registered but stays hidden forever —
/// no `Visible(true)` ever lands in the CHILD queue; the ROOT keeps re-sending
/// its per-pass suppression and asks for hidden paints so the child can paint.
#[test]
fn settings_child_hidden_startup_never_shown() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let settings = settings_child_id();
    let mut app = test_app(true);
    let input = raw_root_hidden();

    let first = logic_pass(&ctx, &mut app, &input);
    assert!(
        visibility_commands(&commands_for(&first, settings)).is_empty(),
        "a hidden startup must NEVER send Visible(true) to the settings child, \
         got {:?}",
        commands_for(&first, settings)
    );
    let root = commands_for(&first, egui::ViewportId::ROOT);
    assert!(
        root.contains(&egui::ViewportCommand::Visible(false)),
        "the root suppression must keep hiding the root every pass, got {root:?}"
    );
    assert!(
        root.contains(&egui::ViewportCommand::RequestPaintWhileHidden),
        "while the hidden settings child needs painting the ROOT must ask for \
         hidden paints (chip recipe, crates/overlay/src/host.rs:529-542), got {root:?}"
    );

    // Re-issued on the next pass, not one-shot: eframe reveals after the paint
    // of the pass that revealed it, so a one-shot hide would be undone.
    let second = logic_pass(&ctx, &mut app, &input);
    assert!(
        commands_for(&second, egui::ViewportId::ROOT)
            .contains(&egui::ViewportCommand::Visible(false)),
        "the root suppression must be re-issued on the next pass too, got {:?}",
        commands_for(&second, egui::ViewportId::ROOT)
    );
    assert!(
        visibility_commands(&commands_for(&second, settings)).is_empty(),
        "the settings child must stay hidden on every hidden-startup pass, got {:?}",
        commands_for(&second, settings)
    );

    // The child is a REGISTERED Deferred viewport while the settings window is
    // open (About pattern), and even after its paint pass the hidden startup
    // still never shows it.
    let callback = settings_callback(&ctx, &mut app, settings);
    child_pass(&ctx, settings, &raw_with_child_window(settings), &callback);

    let third = logic_pass(&ctx, &mut app, &input);
    assert!(
        visibility_commands(&commands_for(&third, settings)).is_empty(),
        "even after the child has painted, a hidden startup must keep it hidden \
         — never Visible(true), got {:?}",
        commands_for(&third, settings)
    );
}

/// Tray ShowSettings shows the SETTINGS CHILD, not the root: the pass after the
/// show sends `Visible(true)` to the child ONCE; a later pass re-sends nothing
/// visibility-wise; a ShowSettings while the child is already shown RAISES it
/// with `Focus` (the About precedent). The root stays hidden: its suppression
/// persists and it never receives `Visible(true)`.
#[test]
fn settings_child_tray_show_shows_child_once() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let settings = settings_child_id();
    let mut app = test_app(true);
    let input = raw_root_hidden();

    // The child exists hidden from the first pass (registered, created with
    // `.with_visible(false)`); drive its paint the way eframe runs a Deferred
    // viewport so any first-paint gate on the show is satisfied deterministically.
    let callback = settings_callback(&ctx, &mut app, settings);
    child_pass(&ctx, settings, &raw_with_child_window(settings), &callback);

    app.show_from_tray(&ctx);
    let shown = logic_pass(&ctx, &mut app, &input);
    assert_eq!(
        visibility_commands(&commands_for(&shown, settings)),
        vec![egui::ViewportCommand::Visible(true)],
        "the tray show must send Visible(true) to the settings CHILD exactly \
         once, got {:?}",
        commands_for(&shown, settings)
    );
    let root = commands_for(&shown, egui::ViewportId::ROOT);
    assert!(
        !root.contains(&egui::ViewportCommand::Visible(true)),
        "the tray show must NOT make the ROOT visible (it carries no settings \
         UI), got {root:?}"
    );
    assert!(
        root.contains(&egui::ViewportCommand::Visible(false)),
        "the root suppression stays after the child is shown, got {root:?}"
    );

    let later = logic_pass(&ctx, &mut app, &input);
    assert_eq!(
        visibility_commands(&commands_for(&later, settings)),
        Vec::new(),
        "once shown, passes must not re-send Visible(true) to the child, got {:?}",
        commands_for(&later, settings)
    );

    // A ShowSettings while the child is shown raises it — Focus, never a
    // second show (About precedent, `app.rs:588-591`).
    app.show_from_tray(&ctx);
    let raised = logic_pass(&ctx, &mut app, &input);
    assert!(
        commands_for(&raised, settings).contains(&egui::ViewportCommand::Focus),
        "a ShowSettings while the child is shown must send Focus to the child, \
         got {:?}",
        commands_for(&raised, settings)
    );
    assert_eq!(
        visibility_commands(&commands_for(&raised, settings)),
        Vec::new(),
        "a raise must not re-send Visible(true), got {:?}",
        commands_for(&raised, settings)
    );
}

/// A first-run startup (`startup_hidden: false`, the `open_settings_on_start`
/// case): the settings child is registered, painted once, and only THEN shown —
/// `Visible(true)` to the CHILD exactly once, gated on that first paint. The
/// ROOT never becomes visible: it carries no settings surface (spec v3).
#[test]
fn settings_child_first_run_shows_after_paint() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let settings = settings_child_id();
    let mut app = test_app(false);

    // Before the child has painted, nothing shows it yet — the reveal is
    // paint-gated (spawn hidden → painted → Visible(true)).
    let before = logic_pass(&ctx, &mut app, &egui::RawInput::default());
    assert_eq!(
        visibility_commands(&commands_for(&before, settings)),
        Vec::new(),
        "the first-run show must be gated on the child's first paint, got {:?}",
        commands_for(&before, settings)
    );
    assert!(
        !commands_for(&before, egui::ViewportId::ROOT)
            .contains(&egui::ViewportCommand::Visible(true)),
        "the ROOT must never become visible — it carries no settings UI, got {:?}",
        commands_for(&before, egui::ViewportId::ROOT)
    );

    let callback = settings_callback(&ctx, &mut app, settings);
    child_pass(&ctx, settings, &raw_with_child_window(settings), &callback);

    let shown = logic_pass(&ctx, &mut app, &egui::RawInput::default());
    assert_eq!(
        visibility_commands(&commands_for(&shown, settings)),
        vec![egui::ViewportCommand::Visible(true)],
        "after the child's first paint the app must send Visible(true) to the \
         settings child once, got {:?}",
        commands_for(&shown, settings)
    );
    assert!(
        !commands_for(&shown, egui::ViewportId::ROOT)
            .contains(&egui::ViewportCommand::Visible(true)),
        "the ROOT must never become visible, got {:?}",
        commands_for(&shown, egui::ViewportId::ROOT)
    );

    // Sent ONCE: a later pass re-sends nothing.
    let later = logic_pass(&ctx, &mut app, &egui::RawInput::default());
    assert_eq!(
        visibility_commands(&commands_for(&later, settings)),
        Vec::new(),
        "the first-run show must not be re-sent on later passes, got {:?}",
        commands_for(&later, settings)
    );
}

/// The CHILD X-close path (§4.5, HideToTray): the click is recorded inside the
/// CHILD paint pass (`SettingsUi::paint`, `app.rs:351-353`) and acted on by the
/// next root `logic` pass (`app.rs:1117-1124`) — hide the child, KEEP it
/// registered, discard the draft. Assertions: the CHILD receives exactly
/// `Visible(false)` (never `Visible(true)`, never `Close` — hidden ≠ destroyed),
/// the ROOT keeps only its per-pass suppression (`Visible(false)`, no
/// `CancelClose`: the root-era hide branch must not fire), and the dirty draft
/// is reverted to `applied` with `settings_shown` cleared. The ROOT never
/// reports a close here — under spec v3 it is undecorated and hidden, so the
/// root-era `CloseAction::HideToTray` branch stays unreachable.
#[test]
fn settings_child_close_hides_to_tray_and_reverts_draft() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(false);
    let settings = settings_child_id();
    let mut app = test_app(true);
    let input = raw_root_hidden();

    // The revert is only observable against a DIRTY draft: with a clean draft
    // `draft == applied` would pass even if the close discarded nothing.
    app.draft.controls.window.key = "F9".to_owned();
    assert_ne!(
        app.draft, app.applied,
        "the draft must be dirty before the X-close for the revert to be observable"
    );

    // A shown window is what has an X button: open the child through the tray.
    let callback = settings_callback(&ctx, &mut app, settings);
    child_pass(&ctx, settings, &raw_with_child_window(settings), &callback);
    app.show_from_tray(&ctx);
    let shown = logic_pass(&ctx, &mut app, &input);
    assert_eq!(
        visibility_commands(&commands_for(&shown, settings)),
        vec![egui::ViewportCommand::Visible(true)],
        "the tray show must show the settings child before this test closes it, got {:?}",
        commands_for(&shown, settings)
    );

    // The X click, driven deterministically: a CHILD paint pass whose
    // `ViewportInfo` carries `ViewportEvent::Close` (`raw_with_child_close`).
    // `SettingsUi::paint` records it into the shared `settings_close` flag.
    child_pass(&ctx, settings, &raw_with_child_close(settings), &callback);

    // The pass AFTER the paint acts on the record (the About-style one-frame
    // ordering): hide the child, keep the draft reverted.
    let closed = logic_pass(&ctx, &mut app, &input);
    assert_eq!(
        visibility_commands(&commands_for(&closed, settings)),
        vec![egui::ViewportCommand::Visible(false)],
        "the child X-close must hide the settings CHILD with Visible(false) \
         exactly once, got {:?}",
        commands_for(&closed, settings)
    );
    assert!(
        !commands_for(&closed, settings).contains(&egui::ViewportCommand::Close),
        "the X-close must NOT destroy the child viewport — it stays registered \
         so a later tray show re-shows the same window, got {:?}",
        commands_for(&closed, settings)
    );
    let root = commands_for(&closed, egui::ViewportId::ROOT);
    assert_eq!(
        visibility_commands(&root),
        vec![egui::ViewportCommand::Visible(false)],
        "the ROOT keeps only its startup suppression, got {root:?}"
    );
    assert!(
        !root.contains(&egui::ViewportCommand::CancelClose),
        "the root-era HideToTray branch must NOT fire — no ROOT close event exists \
         under spec v3 (undecorated, hidden root), got {root:?}"
    );

    assert!(
        !app.settings_shown,
        "the close must clear the shown flag, so the next tray show re-shows"
    );
    assert_eq!(
        app.draft, app.applied,
        "the X-close must discard the dirty draft back to the applied settings"
    );
    assert!(
        !app.settings_close.load(Ordering::Relaxed),
        "the close record must be consumed by the pass that acted on it"
    );

    // Hidden ≠ destroyed: the child is STILL a registered Deferred viewport
    // (`settings_callback` expects it), and a hidden startup-plus-close never
    // re-shows it on later passes.
    settings_callback(&ctx, &mut app, settings);
    let after = logic_pass(&ctx, &mut app, &input);
    assert_eq!(
        visibility_commands(&commands_for(&after, settings)),
        Vec::new(),
        "after the close, later passes must not re-send any visibility command to \
         the child, got {:?}",
        commands_for(&after, settings)
    );
    assert_eq!(
        app.draft, app.applied,
        "the reverted draft must stay applied on the passes after the close"
    );
    // A tray show after the close re-shows the SAME window (`Visible(true)`).
    app.show_from_tray(&ctx);
    let reshown = logic_pass(&ctx, &mut app, &input);
    assert_eq!(
        visibility_commands(&commands_for(&reshown, settings)),
        vec![egui::ViewportCommand::Visible(true)],
        "a tray show after the X-close must re-show the hidden child, got {:?}",
        commands_for(&reshown, settings)
    );
    assert!(
        !commands_for(&reshown, egui::ViewportId::ROOT)
            .contains(&egui::ViewportCommand::Visible(true)),
        "the re-show must not make the ROOT visible, got {:?}",
        commands_for(&reshown, egui::ViewportId::ROOT)
    );
}
