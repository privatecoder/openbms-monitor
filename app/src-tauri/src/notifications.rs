//! What runs `notify.rs` in the app: the connection's begin and end, the poll thread's answers and rounds, a ticker for
//! the lost connection, the wording from the UI, and the OS notification (`tauri-plugin-notification`; the OS asks for
//! permission on first use, a click brings the window to the front).

use crate::notify::{self, Note, Reading, Settings, Texts};
use crate::{bank, history::now_ms, link_now, maint, AppState};
use openbms_proto::{Status, Telemetry};
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::NotificationExt;

/// A new connection (generation `gen`): its installation's name and settings.
pub(crate) fn begin(state: &AppState, gen: u64, installation: Option<&str>) {
    let (name, settings) = installation
        .and_then(|id| maint::with_store(state, |s| Ok((s.installation(id)?.name, Settings::parse(s.notifications(id)?.as_deref())))).ok())
        .unwrap_or_default();
    if let Ok(mut n) = state.notify.lock() {
        n.begin(gen, installation.map(Into::into), name, settings);
    }
}

/// The connection ended; `lost`: the link died and the UI reconnects (the lost watch goes on).
pub(crate) fn end(state: &AppState, lost: bool) {
    if let Ok(mut n) = state.notify.lock() {
        n.end(lost);
    }
}

pub(crate) fn settings_changed(state: &AppState, installation: &str, text: Option<&str>) {
    if let Ok(mut n) = state.notify.lock() {
        n.set_settings(installation, Settings::parse(text));
    }
}

/// One poll of `address` on connection `gen`, as fresh as it came.
pub(crate) fn answer(state: &AppState, gen: u64, address: u8, t: Option<&Telemetry>, s: Option<&Status>) {
    let confirmed = bank::notify_allowed(state, gen, address);
    let reading = Reading::of(t, s);
    if let Ok(mut n) = state.notify.lock() {
        n.answer(gen, address, now_ms(), confirmed, reading.as_ref());
    }
}

/// The end of a polling round: what fired goes out.
pub(crate) fn round_end(app: &AppHandle, state: &AppState) {
    let Ok((notes, name)) = state.notify.lock().map(|mut n| (n.flush(now_ms()), n.name().to_string())) else { return };
    send(app, state, &notes, &name);
}

fn send(app: &AppHandle, state: &AppState, notes: &[Note], name: &str) {
    if notes.is_empty() {
        return;
    }
    let texts = state.notify_texts.lock().map(|t| t.clone()).unwrap_or_default();
    for note in notes {
        let (title, body) = notify::render(note, &texts, name);
        if let Err(e) = show(app, &title, &body) {
            eprintln!("notification: {e}");
        }
    }
}

fn show(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    app.notification().builder().title(title).body(body).show().map_err(|e| e.to_string())
}

/// At start-up: a click on a notification brings the window to the front; every 15 s the lost connection is checked
/// (the poll thread ends with a dead link, so it cannot).
pub(crate) fn start(app: &AppHandle) {
    let front = app.clone();
    let _ = app.notification().on_action(move |_| {
        if let Some(w) = front.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    });
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(15));
        let state = app.state::<AppState>();
        let now = now_ms();
        let paused = state.memory_busy.load(Ordering::SeqCst);
        let lost = state.notify.lock().ok().and_then(|mut n| {
            if paused {
                n.touch(now);
            }
            n.tick(now).map(|note| (note, n.name().to_string()))
        });
        if let Some((note, name)) = lost {
            send(&app, &state, &[note], &name);
        }
    });
}

/// The wording in the UI's language (at start and after a change of language).
#[tauri::command]
pub fn notify_texts(state: State<AppState>, texts: Texts) {
    if let Ok(mut t) = state.notify_texts.lock() {
        *t = texts;
    }
}

/// „Test-Mitteilung senden“.
#[tauri::command]
pub fn notify_test(app: AppHandle, state: State<AppState>, installation: Option<String>) -> Result<(), String> {
    let name = installation.and_then(|id| maint::with_store(&state, |s| Ok(s.installation(&id)?.name)).ok()).unwrap_or_default();
    let texts = state.notify_texts.lock().map(|t| t.clone()).unwrap_or_default();
    let title = texts.title.replace("{{name}}", &name);
    show(&app, &title, &texts.test)
}

/// For the settings: the installation of the connection and whether notifications are off for it (CAN socket, master
/// not matched).
#[derive(Serialize)]
pub struct NotifyStatus {
    installation: Option<String>,
    can_unchecked: bool,
}

#[tauri::command]
pub fn notify_status(state: State<AppState>) -> NotifyStatus {
    let link = link_now(&state);
    let can_unchecked = link.installation.is_some() && bank::can_unchecked(&state, link.gen);
    NotifyStatus { installation: link.installation, can_unchecked }
}
