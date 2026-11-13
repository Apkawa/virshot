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
/// app itself, so the schedules below are the app's doing alone).
fn repaint_test_app() -> SettingsApp {
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
    )
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
