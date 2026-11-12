//! Hotkeys tab (spec §4.4): the row catalog labels and hints, the bind-by-press
//! capture wiring for both fields (keyboard + gamepad probe, pad pump mailbox),
//! and the tab painting. The `SettingsApp` methods live in this file; `gui/app.rs`
//! drives them from the `logic`/`ui` passes. Pure binding decisions stay in
//! [`super::binding`], status icons in [`super::icons`].

use crate::bindings::Action;
use crate::config::Binding;
use crate::gamepad::CaptureField;
use crate::gamepad::active_capture;
use crate::gamepad::pad_notation;
use crate::gamepad::set_capture_mode;
use crate::gamepad::take_captured;
use crate::hotkeys::ProbePress;
use crate::hotkeys::Registration;
use crate::hotkeys::key_label;

use super::SettingsApp;
use super::binding;
use super::icons;

/// Hotkeys tab rows in spec §4.4 order.
pub(crate) const HOTKEY_ROWS: [Action; 3] = [Action::FullScreen, Action::Window, Action::Rect];

/// UI labels for the action rows (catalog `hotkey.row_*`, spec §2).
pub(crate) fn row_label(action: Action) -> String {
    match action {
        Action::FullScreen => rust_i18n::t!("hotkey.row_full_screen").into_owned(),
        Action::Window => rust_i18n::t!("hotkey.row_window").into_owned(),
        Action::Rect => rust_i18n::t!("hotkey.row_rect").into_owned(),
    }
}

/// Tooltip while capturing: Escape cancels (catalog `hotkey.capture_tooltip`).
/// Live hotkeys are PAUSED during capture (Stage 18, spec §4.4 v5.5), so a
/// press cannot fire a registered combo (the old PrintScreen quirk note is
/// gone).
fn capture_tooltip() -> String {
    rust_i18n::t!("hotkey.capture_tooltip").into_owned()
}

/// Tooltip while the pad field is capturing — the same affordances as the
/// shared [`bind_hint`] line (§4.4, v2): Escape cancels, Backspace clears the
/// pad slot; clicking the field again cancels too (catalog
/// `gamepad.capture_tooltip`).
fn pad_capture_tooltip() -> String {
    rust_i18n::t!("gamepad.capture_tooltip").into_owned()
}

/// Tooltip for a Windows pad button (spec §4.1: the pad trigger ORs with the
/// keyboard combo through one shared debounce gate; catalog
/// `gamepad.bind_tooltip`).
fn pad_bind_tooltip() -> String {
    rust_i18n::t!("gamepad.bind_tooltip").into_owned()
}

/// Always-visible one-line hint at the start of the bind form (§4.4, v8,
/// catalog `hotkey.bind_hint`): Escape cancels a capture without changing the
/// row and Backspace clears the binding — for keyboard capture the keyboard
/// half (the pad slot stays), for pad capture the pad slot (the keyboard half
/// stays). Shown whether or not a capture is active, so the keys are
/// discoverable before anyone clicks a field. The wording is field-agnostic,
/// so one line covers both fields.
fn bind_hint() -> String {
    rust_i18n::t!("hotkey.bind_hint").into_owned()
}

impl SettingsApp {
    fn binding_mut(&mut self, action: Action) -> &mut Binding {
        match action {
            Action::FullScreen => &mut self.draft.controls.full_screen,
            Action::Window => &mut self.draft.controls.window,
            Action::Rect => &mut self.draft.controls.rect,
        }
    }

    fn binding(&self, action: Action) -> &Binding {
        match action {
            Action::FullScreen => &self.draft.controls.full_screen,
            Action::Window => &self.draft.controls.window,
            Action::Rect => &self.draft.controls.rect,
        }
    }

    /// Icon for a row's registration status (spec §4.4): info on the reserved
    /// slot. A failed registration is NOT an icon here — it belongs to the
    /// bind button (§4.4, Stage 10), see [`super::binding::keyboard_warning`].
    fn status_icon_for(&self, action: Action) -> Option<icons::StatusIcon> {
        self.status
            .iter()
            .find(|registration| registration.action() == action)
            .and_then(icons::icon_for_status)
    }

    /// The registration failure reason of one row (§4.4, Stage 10): only
    /// `Registration::Failed` is a keyboard warning. `Registered` is healthy and
    /// `Reserved` is an info note in column 1 (§4.1), so neither warns the bind
    /// button.
    fn registration_failure(&self, action: Action) -> Option<&str> {
        self.status
            .iter()
            .find(|registration| registration.action() == action)
            .and_then(|registration| match registration {
                Registration::Failed { reason, .. } => Some(reason.as_str()),
                Registration::Registered { .. } | Registration::Reserved { .. } => None,
            })
    }

    /// Does ANY row carry a failed registration? That is the precondition for
    /// the Stage 13 focus retry — with a healthy report there is nothing to
    /// retry and the controller pass is not called at all.
    pub(crate) fn has_failed_registration(&self) -> bool {
        self.status
            .iter()
            .any(|registration| matches!(registration, Registration::Failed { .. }))
    }

    /// Reserve the status-icon slot immediately LEFT of a control and paint the
    /// warning there when that control has one (§4.4, Stage 10). The slot is
    /// allocated in every row — textured, ASCII `!` fallback, or empty — so the
    /// controls of a column stay aligned and the layout never jumps between the
    /// texture and the fallback. The embedded triangle texture is used when it
    /// loaded; otherwise the ASCII glyph is painted into the SAME slot (egui's
    /// bundled fonts have no ⚠, see [`super::icons::icon_glyph`]). The tooltip stays on the
    /// icon: warnings only appear on active rows, where `on_hover_text` works.
    fn paint_warning_slot(&self, ui: &mut egui::Ui, tooltip: Option<&str>) {
        let (rect, response) = ui.allocate_exact_size(
            egui::Vec2::splat(icons::WARNING_ICON_SIZE),
            egui::Sense::hover(),
        );
        let Some(tooltip) = tooltip else {
            return;
        };
        match &self.warning_icon {
            Some(texture) => {
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icons::icon_glyph(icons::IconKind::Warning),
                    egui::FontId::default(),
                    icons::icon_color(icons::IconKind::Warning),
                );
            }
        }
        response.on_hover_text(tooltip);
    }

    /// Keyboard bind-by-press state (Stage 18) plus the pad-capture mailbox:
    /// while ANY field is capturing, the pad shot path stays silent (Stage 5,
    /// spec §4.4). Any keyboard capture change — start or end — clears the pad
    /// capture: one field captures at a time.
    fn set_keyboard_capture(&mut self, action: Option<Action>) {
        self.capture_press = action;
        set_capture_mode(
            &self.pad_capture,
            action.map(|action| (CaptureField::Keyboard, action)),
        );
        self.capture_pad = None;
    }

    /// Start pad capture for one row (§4.4, Stage 5, Windows only). A keyboard
    /// capture is cancelled first (registrations restored): one field captures
    /// at a time.
    fn start_pad_capture(&mut self, action: Action) {
        if self.capture_press.is_some() {
            self.set_keyboard_capture(None);
            if let Some(transition) =
                binding::capture_transition(true, binding::CaptureInput::Escape)
            {
                self.apply_capture_transition(transition);
            }
        }
        self.capture_pad = Some(action);
        set_capture_mode(&self.pad_capture, Some((CaptureField::Pad, action)));
        tracing::info!("Pad capture started for {}", action.label());
    }

    /// End pad capture (§4.4, v2): the mailbox returns to idle, so pad triggers
    /// fire again. Reached by clicking the field again, by starting a keyboard
    /// capture, and by Escape — none of which writes to the draft.
    fn cancel_pad_capture(&mut self) {
        self.capture_pad = None;
        set_capture_mode(&self.pad_capture, None);
    }

    /// Cancel any active bind capture — keyboard or pad — when the window
    /// closes via OK/Cancel (§4.4): a keyboard capture had paused the live
    /// registrations, so they are restored, and the shared pad mailbox returns
    /// to idle so pad triggers fire again. Nothing is committed to the draft:
    /// the caller discards it (`cancel_draft`).
    pub(crate) fn cancel_active_capture(&mut self) {
        let restoring = binding::capture_transition(
            self.capture_press.is_some(),
            binding::CaptureInput::Escape,
        );
        // `set_keyboard_capture(None)` clears both fields — the keyboard
        // capture and the pad capture the mailbox carries.
        self.set_keyboard_capture(None);
        if let Some(transition) = restoring {
            self.apply_capture_transition(transition);
        }
    }

    /// Drain the press the pump committed into the draft (§4.4, Stage 5): the
    /// notation goes to the row that was capturing and capture state ends. The
    /// notation IS what the field shows and what config holds — the device-info
    /// label name (`XboxX`) or the raw `unknown:<N>` fallback (§9), so no
    /// separate per-row hint is kept. The draft is NOT
    /// published until Apply (§4.4 semantics unchanged) — the pad trigger fires
    /// the captured bind from then on. Runs in `logic`, so it also works with
    /// the window hidden.
    pub(crate) fn drain_pad_capture(&mut self) {
        let Some(event) = take_captured(&self.pad_capture) else {
            return;
        };
        // The notation is needed on BOTH branches: it goes to the row on a hit
        // and names the dropped press on a miss, so it is computed before the
        // decision rather than only inside the success pattern.
        let notation = pad_notation(event);
        let Some(target) = binding::pad_capture_target(self.capture_pad.take(), event) else {
            // The mailbox is the shared truth, so naming it distinguishes a
            // cancelled pad capture from a keyboard capture that never commits.
            tracing::warn!(
                "Pad press ignored: {notation} — no pad field is capturing (mailbox active {:?})",
                active_capture(&self.pad_capture),
            );
            return;
        };
        let (action, notation) = target;
        self.binding_mut(action).pad = Some(notation.clone());
        tracing::info!("Pad bind set in draft: {} → {notation}", action.label());
    }

    /// Drive the hotkey pause/restore transition on the controller (Stage 18).
    /// The controller owns the manager created on the MAIN thread, and `ui`/
    /// `logic` passes run on that same thread (winit/eframe pump), so this
    /// satisfies the WM_HOTKEY thread affinity. No controller (host without
    /// hotkeys) → no-op.
    ///
    /// The bind probe (Stage 14) rides the same transition: it is registered
    /// when a capture opens and unregistered when it ends, so the temporary
    /// GrabPrinter-style hook — and with it the Snipping Tool block — only ever
    /// exists while a row is capturing. `Restore` stops the probes BEFORE
    /// restoring: a bare probe shares its crate id with a bare action binding of
    /// the same key, so a restore first would collide with our own registration.
    fn apply_capture_transition(&mut self, transition: binding::CaptureTransition) {
        let Some(controller) = &mut self.controller else {
            return;
        };
        match transition {
            binding::CaptureTransition::Pause => {
                controller.pause();
                controller.start_bind_probe();
            }
            binding::CaptureTransition::Restore => {
                controller.stop_bind_probe();
                controller.restore();
            }
        }
    }

    /// Bind-by-press (spec §4.4): clicking the bind button records the next
    /// supported key press, REPLACING the draft binding's key+mods. Escape (or
    /// clicking the button again) cancels without changing the row; Backspace
    /// clears the keyboard half (key+mods emptied — the pad slot stays) and
    /// ends capture; unsupported keys are ignored and capture mode stays on.
    /// winit reports Alt/Ctrl/Shift as event modifiers;
    /// the Win key is NOT a winit modifier, so Win is only settable by editing
    /// `config.toml` — an accepted MVP limitation. While capturing, live
    /// hotkey registrations are PAUSED (Stage 18, spec §4.4 v5.5): a press
    /// reaches the egui window instead of being stolen by the platform hotkey
    /// hook (Windows WM_KEYDOWN→WM_HOTKEY), so binding TO PrintScreen works
    /// and does not fire the screenshot action. On capture end — commit OR
    /// cancel — the LAST APPLIED settings are re-registered; the draft stays
    /// unapplied until Apply (§4.4 semantics unchanged).
    pub(crate) fn handle_capture_press(&mut self, ui: &egui::Ui) {
        let Some(action) = self.capture_press else {
            return;
        };
        for event in ui.input(|i| i.events.clone()) {
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = event
            {
                if key == egui::Key::Escape {
                    self.set_keyboard_capture(None);
                    if let Some(transition) =
                        binding::capture_transition(true, binding::CaptureInput::Escape)
                    {
                        self.apply_capture_transition(transition);
                    }
                    return;
                }
                // Backspace clears the keyboard half of the row BEFORE any key
                // is committed (§4.4): it is a capture-END, like Escape, so the
                // paused registrations come back and capture exits. `Backspace`
                // is not in `SUPPORTED_KEY_NAMES`, so it can never become a
                // binding itself (reserved key, §4.1). The pad slot stays.
                if key == egui::Key::Backspace {
                    let cleared = binding::clear_key_binding(self.binding_mut(action));
                    tracing::info!(
                        "Binding cleared for {}: key and mods dropped, pad {}",
                        action.label(),
                        cleared.pad.as_deref().unwrap_or("none")
                    );
                    self.set_keyboard_capture(None);
                    if let Some(transition) =
                        binding::capture_transition(true, binding::CaptureInput::Clear)
                    {
                        self.apply_capture_transition(transition);
                    }
                    return;
                }
                let Some(name) = binding::supported_key_name(key) else {
                    continue; // unsupported key — stay in capture mode
                };
                let mods = binding::mods_from_press(modifiers.alt, modifiers.ctrl, modifiers.shift);
                let binding = self.binding_mut(action);
                let pad = binding.pad.clone();
                *binding = binding::binding_from_press(name, mods, pad);
                self.set_keyboard_capture(None);
                if let Some(transition) =
                    binding::capture_transition(true, binding::CaptureInput::Commit)
                {
                    self.apply_capture_transition(transition);
                }
                return;
            }
        }
    }

    /// Commit a recorded bind-probe press into the capturing row (Stage 14,
    /// spec §4.4) — the same semantics as an egui press in
    /// [`handle_capture_press`]: keyboard fields replace, `pad` stays, capture
    /// ends, and the transition restores (probes stopped first). The press comes
    /// from the listener thread, so this runs on the LOGIC pass: the row is
    /// written even while the settings window is hidden. A press with no keyboard
    /// capture open is dropped (DEBUG) — the probe only exists while a capture is
    /// active, so a stray press must never write a row; an unbindable probe key
    /// is dropped too, so no invalid binding is ever written.
    pub(crate) fn commit_probe_press(&mut self, press: ProbePress) {
        let Some(action) = self.capture_press else {
            tracing::debug!(
                "Bind probe press dropped: {} is not capturing",
                key_label(press.code)
            );
            return;
        };
        let Some(binding) = binding::probe_binding_from_press(self.binding_mut(action), &press)
        else {
            tracing::debug!(
                "Bind probe press dropped: {} has no bindable name",
                key_label(press.code)
            );
            return;
        };
        tracing::info!(
            "Probe bind set for {}: {}",
            action.label(),
            binding::binding_label(&binding.key, &binding.mods)
        );
        *self.binding_mut(action) = binding;
        self.set_keyboard_capture(None);
        if let Some(transition) = binding::capture_transition(true, binding::CaptureInput::Commit) {
            self.apply_capture_transition(transition);
        }
    }

    /// Pad bind-by-press keys (spec §4.4, v2): while one row's pad field is
    /// capturing, Escape ENDS the capture and writes nothing to the row, and
    /// Backspace clears that row's pad slot — the keyboard half stays, the
    /// mirror of [`super::binding::clear_key_binding`] in [`handle_capture_press`]. Any other
    /// key is ignored and capture stays open: the pad slot is written only by
    /// the pump (`drain_pad_capture`), never by a keyboard press. Pad capture
    /// never paused the live hotkey registrations, so ending it needs no
    /// restore — the shared mailbox simply returns to idle
    /// ([`cancel_pad_capture`]), and pad triggers fire again.
    pub(crate) fn handle_pad_capture_press(&mut self, ui: &egui::Ui) {
        let Some(action) = self.capture_pad else {
            return;
        };
        for event in ui.input(|i| i.events.clone()) {
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                ..
            } = event
            {
                let change = binding::pad_capture_action(self.binding_mut(action), key);
                match change {
                    binding::PadCaptureChange::Ignore => continue, // keep waiting for a pad press
                    binding::PadCaptureChange::Cancel => self.cancel_pad_capture(),
                    binding::PadCaptureChange::Clear => {
                        let binding = self.binding(action);
                        tracing::info!(
                            "Pad binding cleared for {}: pad dropped, keyboard {}",
                            action.label(),
                            binding::binding_label(&binding.key, &binding.mods)
                        );
                        self.cancel_pad_capture();
                    }
                }
                return;
            }
        }
    }

    /// Hotkeys tab (spec §4.4 v5.0 + Stage 2 + Stage 10): ONE `egui::Grid` for
    /// the whole tab so the columns line up across rows — action name | keyboard
    /// bind button | gamepad slot. egui 0.37 has no `Table` widget (verified in
    /// the locked source: no table module, only `grid.rs`), so `Grid` is the
    /// alignment tool; a per-row grid sizes its own columns, which is why the
    /// old rows did not line up. A status icon belongs to the CONTROL it
    /// describes (§4.4, Stage 10): a keyboard warning is painted immediately
    /// LEFT of the bind button in column 2, a pad warning immediately LEFT of
    /// the pad field in column 3, and each is driven only by its own kind of
    /// problem ([`super::binding::keyboard_warning`] vs [`super::binding::pad_warning`]). Column 1 keeps just
    /// the action name; the reserved rect row has no bind button or pad field to
    /// attach an icon to, so its Reserved info icon stays there (§4.1). The icon
    /// slot is allocated in every row — empty when there is no warning — so the
    /// controls in a column stay aligned. One hint line at the form start states
    /// what Esc and Backspace do (§4.4, v2: both fields cancel on Esc and clear
    /// on Backspace); since v8 it is painted on every pass — capture active or
    /// not — so the affordance is discoverable before anyone starts a capture.
    /// No key
    /// dropdown, no modifier checkboxes: the button shows the binding and click
    /// → bind-by-press.
    pub(crate) fn hotkeys_tab(&mut self, ui: &mut egui::Ui) {
        // Always shown (§4.4, v8): the capture state does not gate it.
        ui.label(bind_hint());

        egui::Grid::new("hotkeys_grid")
            .num_columns(3)
            .show(ui, |ui| {
                for action in HOTKEY_ROWS {
                    let reserved = action == Action::Rect;
                    let capturing = self.capture_press == Some(action);

                    // Owned before the cell closures so the row can still
                    // toggle `capture_press` while rendering.
                    let label = binding::bind_button_label(
                        capturing,
                        &self.binding(action).key,
                        &self.binding(action).mods,
                    );
                    // Per-control warnings (§4.4, Stage 10): keyboard signals
                    // warn the bind button, pad signals warn the pad field.
                    // Both are owned before the closures, which mutate `self`.
                    let keyboard = binding::keyboard_warning(
                        &self.draft,
                        action,
                        self.registration_failure(action),
                    );
                    let pad = binding::pad_warning(&self.draft, action);

                    // Column 1: the action name only. The reserved rect row
                    // renders disabled, which greys the label too; its Reserved
                    // info icon stays here because that row has no bind button
                    // or pad field of its own (§4.1) — the tooltip is what
                    // explains the greyed row.
                    ui.add_enabled_ui(!reserved, |ui| {
                        ui.horizontal(|ui| {
                            if reserved && let Some(icon) = self.status_icon_for(action) {
                                let response = ui.label(
                                    egui::RichText::new(icons::icon_glyph(icon.kind))
                                        .color(icons::icon_color(icon.kind))
                                        .strong(),
                                );
                                // `on_hover_text` is silent on disabled
                                // widgets (egui 0.37), and the reserved rect
                                // row renders disabled.
                                response.on_disabled_hover_text(icon.tooltip);
                            }
                            ui.label(row_label(action));
                        });
                    });

                    // Column 2: the keyboard bind button (§4.4), with this row's
                    // keyboard warning immediately to its LEFT (§4.4, Stage 10).
                    // Disabled on the reserved row, so `Rect` never enters
                    // capture — and therefore Backspace can never clear it; a
                    // reserved row also has no keyboard warning to paint.
                    ui.add_enabled_ui(!reserved, |ui| {
                        ui.horizontal(|ui| {
                            self.paint_warning_slot(ui, keyboard.as_deref());
                            let button = egui::Button::new(label)
                                .min_size(egui::Vec2::new(icons::BIND_BUTTON_MIN_WIDTH, 0.0));
                            let response = if capturing {
                                ui.add(button).on_hover_text(capture_tooltip())
                            } else {
                                ui.add(button)
                            };
                            // `on_hover_text` is silent on disabled widgets (egui
                            // 0.37), and both tooltip methods consume the
                            // response, so the result is rebound, not cloned.
                            let response = if reserved {
                                response.on_disabled_hover_text(binding::reserved_tooltip())
                            } else {
                                response
                            };
                            if response.clicked() {
                                if let Some(transition) = binding::capture_transition(
                                    capturing,
                                    binding::CaptureInput::Click,
                                ) {
                                    self.apply_capture_transition(transition);
                                }
                                self.set_keyboard_capture(if capturing {
                                    None
                                } else {
                                    Some(action)
                                });
                            }
                        });
                    });

                    // Column 3: pad field (§4.1 + §4.4, Stage 5) with this row's
                    // pad warning immediately to its LEFT (§4.4, Stage 10): pad
                    // signals only, so a keyboard collision never marks this
                    // cell. On Windows the field is a real bind-by-press button —
                    // click starts capture, the pump commits the first DOWN event
                    // into the mailbox and `logic` writes the notation into the
                    // draft; a click while capturing cancels. On a host without
                    // GameInput it stays a disabled placeholder (Linux gilrs is
                    // out of scope) — the pad warning icon still renders next to
                    // it, because the icon itself is not disabled. Inside
                    // `ui.add_enabled_ui(!reserved)` the button is disabled on
                    // the rect row, so a reserved action never gets a pad bind
                    // from the UI either (and it never collides, §4.1).
                    ui.add_enabled_ui(!reserved, |ui| {
                        ui.horizontal(|ui| {
                            self.paint_warning_slot(ui, pad.as_deref());
                            let pad_capturing = self.capture_pad == Some(action);
                            if cfg!(windows) {
                                let pad_label = binding::pad_field_label(
                                    true,
                                    pad_capturing,
                                    self.binding(action).pad.as_deref(),
                                );
                                let pad_button = egui::Button::new(pad_label)
                                    .min_size(egui::Vec2::new(icons::BIND_BUTTON_MIN_WIDTH, 0.0));
                                let pad_response = if pad_capturing {
                                    ui.add(pad_button).on_hover_text(pad_capture_tooltip())
                                } else {
                                    ui.add(pad_button).on_hover_text(pad_bind_tooltip())
                                };
                                // Same disabled-tooltip rule as the bind button:
                                // the tooltip methods consume the response.
                                let pad_response = if reserved {
                                    pad_response.on_disabled_hover_text(binding::reserved_tooltip())
                                } else {
                                    pad_response
                                };
                                if pad_response.clicked() {
                                    match binding::pad_capture_toggle(pad_capturing, action) {
                                        Some(start) => self.start_pad_capture(start),
                                        None => self.cancel_pad_capture(),
                                    }
                                }
                            } else {
                                ui.add_enabled(
                                    false,
                                    egui::Button::new(binding::gamepad_placeholder_label()),
                                );
                            }
                        });
                    });

                    ui.end_row();
                }
            });

        if self.controller.is_none() {
            ui.label(rust_i18n::t!("hotkey.unavailable_label"));
        }
    }
}

#[cfg(test)]
#[path = "hotkeys.tests.rs"]
mod tests;
