//! Tauri app layer: commands + events over ogma-core.

#[cfg(desktop)]
mod tray;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ogma_core::models::{ActionItem, Meeting, MeetingNotes, MeetingStatus, TranscriptSegment};
use ogma_core::pipeline::{Pipeline, ProgressEvent};
use ogma_core::recording::{self, Recorder};
use ogma_core::storage::Storage;
use ogma_core::Config;
use tauri::{AppHandle, Emitter, Manager, State};

struct ActiveRecording {
    recorder: Recorder,
    meeting_id: String,
}

struct AppState {
    storage: Arc<Mutex<Storage>>,
    config: Mutex<Config>,
    recording: Mutex<Option<ActiveRecording>>,
    /// Serializes recording start/stop/quit transitions: toggle decisions
    /// stay atomic (no stop-then-surprise-restart) and quit waits out a stop
    /// that is still finalizing instead of exiting mid-write.
    transition: tokio::sync::Mutex<()>,
    /// Last tray/shortcut toggle, to debounce key auto-repeat.
    last_toggle: Mutex<Option<Instant>>,
    /// Caps how many Whisper→Claude→Notion pipelines run at once. A batch or
    /// drag-drop import creates one meeting per file, and each pipeline buffers
    /// a whole ~9.6MB segment per in-flight Whisper request and shares the
    /// provider rate limits — so the fan-out is bounded instead of one live
    /// pipeline per dropped file. Queued meetings just wait their turn; the
    /// pipeline is resumable, so waiting costs nothing.
    pipeline_slots: Arc<tokio::sync::Semaphore>,
    /// Set once an OTA update install is committed (download → relaunch). While
    /// set, `start_recording_locked` refuses to start a new recording, so the
    /// installer's relaunch can't kill an active segment. Toggled and read only
    /// under `transition`, so it stays atomic against recording-start.
    install_committed: AtomicBool,
    data_dir: PathBuf,
}

/// Max concurrent pipelines (see `AppState::pipeline_slots`). Small on purpose:
/// Whisper transcribes a meeting's chunks serially anyway, so extra parallelism
/// buys little and mostly multiplies memory and rate-limit pressure.
const PIPELINE_CONCURRENCY: usize = 2;

impl AppState {
    fn meetings_dir(&self) -> PathBuf {
        self.data_dir.join("meetings")
    }
}

type CmdResult<T> = Result<T, String>;

fn err_str(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(serde::Serialize)]
struct MeetingDetail {
    meeting: Meeting,
    segments: Vec<TranscriptSegment>,
    notes: Option<MeetingNotes>,
    action_items: Vec<ActionItem>,
    audio_path: Option<String>,
}

#[derive(serde::Serialize, Clone)]
struct RecordingState {
    meeting_id: Option<String>,
    elapsed_ms: i64,
    paused: bool,
}

// ── meetings ────────────────────────────────────────────────────────────────

#[tauri::command]
fn list_meetings(state: State<AppState>) -> CmdResult<Vec<Meeting>> {
    let storage = state.storage.lock().unwrap();
    storage.list_meetings().map_err(err_str)
}

#[tauri::command]
fn get_meeting_detail(state: State<AppState>, meeting_id: String) -> CmdResult<MeetingDetail> {
    let storage = state.storage.lock().unwrap();
    let meeting = storage.get_meeting(&meeting_id).map_err(err_str)?;
    let segments = storage.get_segments(&meeting_id).map_err(err_str)?;
    let notes = storage.get_notes(&meeting_id).map_err(err_str)?;
    let action_items: Vec<ActionItem> = storage
        .list_action_items(None)
        .map_err(err_str)?
        .into_iter()
        .filter(|item| item.meeting_id == meeting_id)
        .collect();
    let audio = PathBuf::from(&meeting.audio_dir).join("audio.wav");
    let audio_path = audio.exists().then(|| audio.to_string_lossy().to_string());
    Ok(MeetingDetail {
        meeting,
        segments,
        notes,
        action_items,
        audio_path,
    })
}

#[tauri::command]
fn set_meeting_title(state: State<AppState>, meeting_id: String, title: String) -> CmdResult<()> {
    let storage = state.storage.lock().unwrap();
    storage.set_title(&meeting_id, &title).map_err(err_str)
}

#[tauri::command]
fn rename_speaker(
    state: State<AppState>,
    meeting_id: String,
    from: String,
    to: String,
) -> CmdResult<()> {
    let mut storage = state.storage.lock().unwrap();
    storage.rename_speaker(&meeting_id, &from, &to).map_err(err_str)
}

#[tauri::command]
fn delete_meeting(state: State<AppState>, meeting_id: String) -> CmdResult<()> {
    let audio_dir = {
        let storage = state.storage.lock().unwrap();
        let meeting = storage.get_meeting(&meeting_id).map_err(err_str)?;
        storage.delete_meeting(&meeting_id).map_err(err_str)?;
        meeting.audio_dir
    };
    let _ = std::fs::remove_dir_all(audio_dir);
    Ok(())
}

#[tauri::command]
fn set_action_item_status(state: State<AppState>, id: i64, status: String) -> CmdResult<()> {
    let storage = state.storage.lock().unwrap();
    storage.set_action_item_status(id, &status).map_err(err_str)
}

// ── recording ───────────────────────────────────────────────────────────────

#[tauri::command]
async fn start_recording(app: AppHandle, title: Option<String>) -> CmdResult<Meeting> {
    do_start_recording(&app, title).await
}

/// Shared by the `start_recording` command, the tray menu and the global
/// shortcut, so recording can begin without the window being open.
async fn do_start_recording(app: &AppHandle, title: Option<String>) -> CmdResult<Meeting> {
    let state = app.state::<AppState>();
    let _transition = state.transition.lock().await;
    start_recording_locked(app, title)
}

/// Body of a start transition; callers must hold `AppState::transition`.
fn start_recording_locked(app: &AppHandle, title: Option<String>) -> CmdResult<Meeting> {
    let state = app.state::<AppState>();
    // Refuse to start once an OTA install is committed — the pending relaunch
    // would kill the segment we're about to open. Read under `transition`
    // (held by every caller), so it can't race `begin_update_install`.
    if state.install_committed.load(Ordering::SeqCst) {
        return Err("an update is being installed — recording is unavailable until restart".into());
    }
    let mut active = state.recording.lock().unwrap();
    if active.is_some() {
        return Err("a recording is already in progress".into());
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Local::now();
    let title = title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| format!("Meeting {}", now.format("%Y-%m-%d %H:%M")));
    let dir = state.meetings_dir().join(&id);

    let meeting = Meeting {
        id: id.clone(),
        title,
        created_at: now.to_rfc3339(),
        duration_ms: 0,
        status: MeetingStatus::Recording,
        error: None,
        audio_dir: dir.to_string_lossy().to_string(),
        notion_page_id: None,
    };

    let device_name = state.config.lock().unwrap().input_device.clone();
    let level_app = app.clone();
    let recorder = Recorder::start(
        &dir,
        Some(device_name),
        Box::new(move |level| {
            let _ = level_app.emit("recording:level", level);
        }),
    )
    .map_err(err_str)?;

    {
        let storage = state.storage.lock().unwrap();
        storage.create_meeting(&meeting).map_err(err_str)?;
    }
    *active = Some(ActiveRecording {
        recorder,
        meeting_id: id,
    });
    let _ = app.emit("meetings:changed", ());
    update_tray(app, true);
    Ok(meeting)
}

#[tauri::command]
fn pause_recording(state: State<AppState>) -> CmdResult<()> {
    let active = state.recording.lock().unwrap();
    match active.as_ref() {
        Some(session) => {
            session.recorder.pause();
            Ok(())
        }
        None => Err("no active recording".into()),
    }
}

#[tauri::command]
fn resume_recording(state: State<AppState>) -> CmdResult<()> {
    let active = state.recording.lock().unwrap();
    match active.as_ref() {
        Some(session) => {
            session.recorder.resume();
            Ok(())
        }
        None => Err("no active recording".into()),
    }
}

#[tauri::command]
fn list_input_devices() -> CmdResult<Vec<String>> {
    Ok(recording::list_input_devices())
}

#[tauri::command]
fn recording_state(state: State<AppState>) -> CmdResult<RecordingState> {
    let active = state.recording.lock().unwrap();
    Ok(match active.as_ref() {
        Some(session) => RecordingState {
            meeting_id: Some(session.meeting_id.clone()),
            elapsed_ms: session.recorder.elapsed_ms(),
            paused: session.recorder.is_paused(),
        },
        None => RecordingState {
            meeting_id: None,
            elapsed_ms: 0,
            paused: false,
        },
    })
}

/// Commit to installing an OTA update: refuse if a recording is active, else
/// latch `install_committed` so no recording can start while the installer
/// downloads and relaunches. Runs under `transition` so the check-and-latch is
/// atomic against `start_recording_locked` (the toggle/tray/shortcut paths).
#[tauri::command]
async fn begin_update_install(app: AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let _transition = state.transition.lock().await;
    if state.recording.lock().unwrap().is_some() {
        return Err("a recording is in progress — stop it before installing the update".into());
    }
    state.install_committed.store(true, Ordering::SeqCst);
    Ok(())
}

/// Undo `begin_update_install` when the install fails to complete, so recording
/// becomes available again without restarting the app.
#[tauri::command]
fn cancel_update_install(state: State<AppState>) {
    state.install_committed.store(false, Ordering::SeqCst);
}

#[tauri::command]
async fn stop_recording(app: AppHandle) -> CmdResult<String> {
    do_stop_recording(app).await
}

/// Shared by the `stop_recording` command, the tray menu, the global shortcut
/// and Quit — finalizes the audio and kicks off the pipeline.
async fn do_stop_recording(app: AppHandle) -> CmdResult<String> {
    let state = app.state::<AppState>();
    let _transition = state.transition.lock().await;
    stop_recording_locked(&app).await
}

/// Body of a stop transition; callers must hold `AppState::transition` so
/// quit/toggle can't observe a half-finalized stop.
async fn stop_recording_locked(app: &AppHandle) -> CmdResult<String> {
    let state = app.state::<AppState>();
    let session = {
        let mut active = state.recording.lock().unwrap();
        active.take().ok_or("no active recording")?
    };
    update_tray(app, false);
    let meeting_id = session.meeting_id.clone();

    // Recorder::stop joins audio threads — run it off the async runtime.
    let result = tauri::async_runtime::spawn_blocking(move || session.recorder.stop())
        .await
        .map_err(err_str)?
        .map_err(err_str)?;

    let audio_dir = {
        let storage = state.storage.lock().unwrap();
        storage
            .set_duration(&meeting_id, result.duration_ms)
            .map_err(err_str)?;
        storage
            .set_status(&meeting_id, MeetingStatus::Recorded, None)
            .map_err(err_str)?;
        storage.get_meeting(&meeting_id).map_err(err_str)?.audio_dir
    };

    // Concatenate segments into audio.wav for playback (streamed, cheap).
    let segments = result.segments.clone();
    let out = PathBuf::from(&audio_dir).join("audio.wav");
    let concat_result =
        tauri::async_runtime::spawn_blocking(move || recording::wav::concat(&segments, &out))
            .await
            .map_err(err_str)?;
    if let Err(e) = concat_result {
        tracing::warn!("audio concat failed: {e}");
    }

    let _ = app.emit("meetings:changed", ());
    spawn_pipeline(app, meeting_id.clone());
    Ok(meeting_id)
}

#[tauri::command]
async fn discard_recording(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let session = {
        let mut active = state.recording.lock().unwrap();
        active.take().ok_or("no active recording")?
    };
    let meeting_id = session.meeting_id.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || session.recorder.stop()).await;
    let audio_dir = {
        let storage = state.storage.lock().unwrap();
        let meeting = storage.get_meeting(&meeting_id).map_err(err_str)?;
        storage.delete_meeting(&meeting_id).map_err(err_str)?;
        meeting.audio_dir
    };
    let _ = std::fs::remove_dir_all(audio_dir);
    let _ = app.emit("meetings:changed", ());
    update_tray(&app, false);
    Ok(())
}

/// Extensions offered by the import picker and accepted by the importer.
const IMPORT_AUDIO_EXTS: [&str; 7] = ["wav", "m4a", "mp3", "flac", "ogg", "mp4", "aac"];

/// Outcome of a (possibly multi-file) import: how many meetings were created
/// and a human-readable line per file that failed. Emitted as `import:done`
/// for drag-and-drop (Rust-initiated) and returned directly by the picker
/// command.
#[derive(serde::Serialize, Clone)]
struct ImportSummary {
    imported: usize,
    errors: Vec<String>,
}

/// Decode one already-picked audio file into a new meeting and kick off its
/// pipeline. `src` must be an OS-provided path (native picker or OS drag-drop),
/// never a webview-supplied string — that's the invariant that keeps a
/// compromised webview from feeding arbitrary files to the transcribe→Notion
/// pipeline. The meeting row is only created after a successful decode, and
/// every earlier failure removes the meeting dir, so a bad file leaves nothing
/// behind. Does not emit `meetings:changed` — the caller batches that.
async fn import_one(app: &AppHandle, src: PathBuf, title: Option<String>) -> CmdResult<String> {
    // Defense in depth behind the dialog filter / drop handler (e.g. a
    // typed-in filename or a non-audio item in a mixed drop).
    if !is_supported_audio(&src) {
        return Err(format!(
            "unsupported file type — expected one of: {}",
            IMPORT_AUDIO_EXTS.join(", ")
        ));
    }
    if !src.is_file() {
        return Err(format!("file not found: {}", src.display()));
    }
    let state = app.state::<AppState>();
    let id = uuid::Uuid::new_v4().to_string();
    let dir = state.meetings_dir().join(&id);
    let now = chrono::Local::now();
    let title = title
        .filter(|t| !t.trim().is_empty())
        .or_else(|| src.file_stem().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| format!("Imported {}", now.format("%Y-%m-%d %H:%M")));

    // Decode + segment off the async runtime; then concat for playback.
    let (src_task, dir_task) = (src.clone(), dir.clone());
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let result = recording::import::import_file(&src_task, &dir_task)?;
        if let Err(e) = recording::wav::concat(&result.segments, &dir_task.join("audio.wav")) {
            tracing::warn!("audio concat failed: {e}");
        }
        Ok::<_, ogma_core::Error>(result)
    })
    .await;
    let result = match joined {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e.to_string());
        }
        // Join failure (decode task panicked): same cleanup contract.
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(err_str(e));
        }
    };

    let meeting = Meeting {
        id: id.clone(),
        title,
        created_at: now.to_rfc3339(),
        duration_ms: result.duration_ms,
        status: MeetingStatus::Recorded,
        error: None,
        audio_dir: dir.to_string_lossy().to_string(),
        notion_page_id: None,
    };
    let created = {
        let storage = state.storage.lock().unwrap();
        storage.create_meeting(&meeting)
    };
    if let Err(e) = created {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(err_str(e));
    }

    spawn_pipeline(app, id.clone());
    Ok(id)
}

/// `src`'s file name for error messages, falling back to the full path.
fn import_label(src: &Path) -> String {
    src.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| src.display().to_string())
}

/// True if `src` has an extension the importer accepts. Extension-only by
/// design — the decoder in `recording::import` is the real arbiter of whether
/// the bytes are audio; this just keeps obvious non-audio out of the pipeline.
fn is_supported_audio(src: &Path) -> bool {
    src.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMPORT_AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Import each file independently, collecting per-file outcomes: one bad file
/// never aborts the batch, and each failure contributes exactly one error line
/// labeled with its file name. Generic over the per-file import so the
/// partitioning logic is testable without a running Tauri app.
async fn import_many<F, Fut>(files: Vec<PathBuf>, mut import: F) -> ImportSummary
where
    F: FnMut(PathBuf) -> Fut,
    Fut: std::future::Future<Output = CmdResult<String>>,
{
    let mut summary = ImportSummary { imported: 0, errors: Vec::new() };
    for src in files {
        match import(src.clone()).await {
            Ok(_) => summary.imported += 1,
            Err(e) => summary.errors.push(format!("{}: {e}", import_label(&src))),
        }
    }
    summary
}

/// Import external audio files (WAV/M4A/MP3/FLAC/OGG) as new meetings via a
/// native multi-select picker: pick → decode → 16 kHz mono 5-min segments →
/// normal pipeline. The picker runs Rust-side so the webview never supplies a
/// path. Each file is imported independently; one bad file doesn't abort the
/// rest — its error comes back in the summary. Returns a zero summary if the
/// user cancels.
#[tauri::command]
async fn import_audio_files(app: AppHandle) -> CmdResult<ImportSummary> {
    use tauri_plugin_dialog::DialogExt;

    let picker_app = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        picker_app
            .dialog()
            .file()
            .add_filter("Audio", &IMPORT_AUDIO_EXTS)
            .blocking_pick_files()
    })
    .await
    .map_err(err_str)?;
    let Some(picked) = picked else {
        return Ok(ImportSummary { imported: 0, errors: Vec::new() }); // cancelled
    };

    // A picked entry that won't resolve to a path is its own error line, and
    // doesn't stop the rest of the batch.
    let mut errors = Vec::new();
    let mut paths = Vec::new();
    for file in picked {
        match file.into_path() {
            Ok(p) => paths.push(p),
            Err(e) => errors.push(err_str(e)),
        }
    }

    let mut summary = import_many(paths, |src| import_one(&app, src, None)).await;
    summary.errors.splice(0..0, errors);
    if summary.imported > 0 {
        let _ = app.emit("meetings:changed", ());
    }
    Ok(summary)
}

/// Import audio files dropped onto the window. Paths come from the OS drag-drop
/// event (handled in `on_window_event`), not from the webview, so the same
/// "webview never supplies a path" invariant as the picker holds. Non-audio
/// items in a mixed drop are ignored. Emits `import:done` with the summary so
/// the frontend can toast the result.
async fn import_dropped(app: AppHandle, paths: Vec<PathBuf>) {
    // Silently skip non-audio items dropped alongside audio.
    let audio: Vec<PathBuf> = paths.into_iter().filter(|p| is_supported_audio(p)).collect();
    let summary = import_many(audio, |src| import_one(&app, src, None)).await;
    // Nothing droppable at all → stay silent rather than toast a no-op.
    if summary.imported == 0 && summary.errors.is_empty() {
        return;
    }
    if summary.imported > 0 {
        let _ = app.emit("meetings:changed", ());
    }
    let _ = app.emit("import:done", summary);
}

/// Native folder picker (Rust-side, same rationale as the import picker) for
/// the Markdown destination folder in settings. `None` if cancelled.
#[tauri::command]
async fn pick_folder(app: AppHandle) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;

    let picker_app = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        picker_app.dialog().file().blocking_pick_folder()
    })
    .await
    .map_err(err_str)?;
    match picked {
        Some(path) => {
            let path = path.into_path().map_err(err_str)?;
            Ok(Some(path.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

// ── tray & global shortcut ──────────────────────────────────────────────────

/// Start if idle, stop if recording — the tray/global-shortcut entry point.
/// Errors are logged, not surfaced: there may be no window to show them in.
fn toggle_recording(app: AppHandle) {
    // Debounce: Windows delivers key auto-repeat for global shortcuts
    // (RegisterHotKey without MOD_NOREPEAT), and a nervous double-press must
    // not stop the mic and immediately restart it.
    {
        let state = app.state::<AppState>();
        let mut last = state.last_toggle.lock().unwrap();
        let now = Instant::now();
        if last.is_some_and(|t| now.duration_since(t) < Duration::from_millis(400)) {
            return;
        }
        *last = Some(now);
    }
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        // Decide start-vs-stop under the transition lock so the snapshot
        // can't go stale while a previous toggle is still finalizing.
        let _transition = state.transition.lock().await;
        let recording = state.recording.lock().unwrap().is_some();
        let result = if recording {
            stop_recording_locked(&app).await.map(|_| ())
        } else {
            start_recording_locked(&app, None).map(|_| ())
        };
        if let Err(e) = result {
            tracing::warn!("toggle recording failed: {e}");
        }
    });
}

/// Quit from the tray: finalize any active recording first so the audio and
/// meeting status are safe (an interrupted pipeline is marked as a retryable
/// error on next launch by `recover_on_startup`).
fn quit(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        // Taking the transition lock waits out a stop that is already
        // finalizing, so we never exit mid-write.
        let _transition = state.transition.lock().await;
        let recording = state.recording.lock().unwrap().is_some();
        if recording {
            if let Err(e) = stop_recording_locked(&app).await {
                tracing::warn!("finalizing recording before quit failed: {e}");
            }
        }
        app.exit(0);
    });
}

fn update_tray(app: &AppHandle, recording: bool) {
    #[cfg(desktop)]
    tray::update(app, recording);
    #[cfg(not(desktop))]
    let _ = (app, recording);
}

#[cfg(desktop)]
fn init_global_shortcut(app: &AppHandle) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

    let modifiers = if cfg!(target_os = "macos") {
        Modifiers::SUPER | Modifiers::SHIFT
    } else {
        Modifiers::CONTROL | Modifiers::SHIFT
    };
    let shortcut = Shortcut::new(Some(modifiers), Code::KeyR);

    if let Err(e) = app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, pressed, event| {
                if *pressed == shortcut && event.state() == ShortcutState::Pressed {
                    toggle_recording(app.clone());
                }
            })
            .build(),
    ) {
        tracing::warn!("global-shortcut plugin failed to initialize: {e}");
        return;
    }
    // Registration can fail if another app owns the combination — the tray
    // still works, so warn instead of failing startup.
    if let Err(e) = app.global_shortcut().register(shortcut) {
        tracing::warn!("could not register global shortcut Ctrl/Cmd+Shift+R: {e}");
    }
}

// ── pipeline ────────────────────────────────────────────────────────────────

fn spawn_pipeline(app: &AppHandle, meeting_id: String) {
    let state = app.state::<AppState>();
    let storage = Arc::clone(&state.storage);
    let config = state.config.lock().unwrap().clone();
    let slots = Arc::clone(&state.pipeline_slots);
    let progress_app = app.clone();
    let on_progress: ogma_core::pipeline::ProgressCallback = Arc::new(move |event: ProgressEvent| {
        let _ = progress_app.emit("meeting:progress", event.clone());
        let _ = progress_app.emit("meetings:changed", ());
    });
    tauri::async_runtime::spawn(async move {
        // Wait for a slot before doing any provider work: a 20-file import
        // queues here instead of opening 20 concurrent Whisper uploads.
        let Ok(_permit) = slots.acquire_owned().await else {
            return; // semaphore closed — app is shutting down
        };
        let pipeline = Pipeline::new(storage, config, on_progress);
        if let Err(e) = pipeline.run(&meeting_id).await {
            tracing::error!("pipeline for {meeting_id} failed: {e}");
        }
    });
}

#[tauri::command]
fn retry_pipeline(app: AppHandle, meeting_id: String) -> CmdResult<()> {
    spawn_pipeline(&app, meeting_id);
    Ok(())
}

// ── settings & notion ───────────────────────────────────────────────────────

/// "macos" | "windows" | "linux" — lets the UI hide platform-only settings
/// (the Apple Notes destination toggle).
#[tauri::command]
fn get_platform() -> &'static str {
    std::env::consts::OS
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> CmdResult<Config> {
    Ok(state.config.lock().unwrap().clone())
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: Config) -> CmdResult<()> {
    settings.save(&state.data_dir).map_err(err_str)?;
    *state.config.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
async fn notion_setup(state: State<'_, AppState>, parent_page_id: String) -> CmdResult<String> {
    let (token, mut config) = {
        let config = state.config.lock().unwrap();
        (config.notion_api_key.clone(), config.clone())
    };
    if token.is_empty() {
        return Err("Notion API key is not set".into());
    }
    let client = ogma_core::notion::NotionClient::new(token, String::new());
    let database_id = client
        .create_meetings_database(parent_page_id.trim())
        .await
        .map_err(err_str)?;
    config.notion_database_id = database_id.clone();
    config.save(&state.data_dir).map_err(err_str)?;
    *state.config.lock().unwrap() = config;
    Ok(database_id)
}

// ── startup ─────────────────────────────────────────────────────────────────

/// Recover meetings left in a non-terminal state by a crash: repair WAV
/// segments for meetings stuck in `recording`, and mark interrupted pipeline
/// runs as retryable errors.
fn recover_on_startup(storage: &Arc<Mutex<Storage>>) {
    let storage = storage.lock().unwrap();
    let Ok(stuck) = storage.meetings_with_status(MeetingStatus::Recording) else {
        return;
    };
    for meeting in stuck {
        let dir = PathBuf::from(&meeting.audio_dir);
        match recording::recover_segments(&dir) {
            Ok(result) if !result.segments.is_empty() => {
                let _ = recording::wav::concat(&result.segments, &dir.join("audio.wav"));
                let _ = storage.set_duration(&meeting.id, result.duration_ms);
                let _ = storage.set_status(&meeting.id, MeetingStatus::Recorded, None);
                tracing::info!("recovered crashed recording {}", meeting.id);
            }
            _ => {
                let _ = storage.set_status(
                    &meeting.id,
                    MeetingStatus::Error,
                    Some("recording was interrupted and no audio could be recovered"),
                );
            }
        }
    }
    // `Recorded` at startup means the audio was finalized but the pipeline
    // never wrote its first status (e.g. quit or a crash raced the spawned
    // pipeline task) — without this sweep it would sit as "queued" forever
    // with no Retry button. This also gives recovered crashed recordings
    // (set to `Recorded` above) a retryable state.
    for status in [
        MeetingStatus::Recorded,
        MeetingStatus::Transcribing,
        MeetingStatus::Summarizing,
        MeetingStatus::Syncing,
    ] {
        if let Ok(interrupted) = storage.meetings_with_status(status) {
            for meeting in interrupted {
                let _ = storage.set_status(
                    &meeting.id,
                    MeetingStatus::Error,
                    Some("processing was interrupted — press Retry"),
                );
            }
        }
    }
}

pub fn data_dir_for(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("no app data dir available")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = data_dir_for(app.handle());
            std::fs::create_dir_all(&data_dir)?;
            let storage = Arc::new(Mutex::new(Storage::open(&data_dir.join("ogma.db"))?));
            let config = Config::load(&data_dir)?;
            recover_on_startup(&storage);
            app.manage(AppState {
                storage,
                config: Mutex::new(config),
                recording: Mutex::new(None),
                transition: tokio::sync::Mutex::new(()),
                last_toggle: Mutex::new(None),
                pipeline_slots: Arc::new(tokio::sync::Semaphore::new(PIPELINE_CONCURRENCY)),
                install_committed: AtomicBool::new(false),
                data_dir,
            });
            #[cfg(desktop)]
            {
                // OTA updates: check/download driven from the frontend
                // (src/updater.ts); process plugin provides relaunch().
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
                app.handle().plugin(tauri_plugin_process::init())?;
                tray::init(app.handle())?;
                init_global_shortcut(app.handle());
            }
            Ok(())
        })
        // Closing the window hides to the tray so quick-start keeps working;
        // Quit lives in the tray menu.
        .on_window_event(|window, event| {
            #[cfg(desktop)]
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                // OS-level file drop: import any audio files off the async
                // runtime. Paths come from the OS here, not the webview, so the
                // picker's "webview never supplies a path" invariant holds.
                tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
                    let app = window.app_handle().clone();
                    let paths = paths.clone();
                    tauri::async_runtime::spawn(async move {
                        import_dropped(app, paths).await;
                    });
                }
                _ => {}
            }
            #[cfg(not(desktop))]
            let _ = (window, event);
        })
        .invoke_handler(tauri::generate_handler![
            list_meetings,
            get_meeting_detail,
            set_meeting_title,
            rename_speaker,
            delete_meeting,
            set_action_item_status,
            start_recording,
            pause_recording,
            resume_recording,
            list_input_devices,
            recording_state,
            begin_update_install,
            cancel_update_install,
            stop_recording,
            discard_recording,
            import_audio_files,
            pick_folder,
            retry_pipeline,
            get_platform,
            get_settings,
            save_settings,
            notion_setup
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Entry point for `ogma --mcp`: stdio MCP server over the same database.
pub fn run_mcp() {
    let data_dir = dirs_data_dir().expect("no app data dir available");
    let storage = Storage::open(&data_dir.join("ogma.db")).expect("failed to open database");
    let storage = Arc::new(Mutex::new(storage));
    tauri::async_runtime::block_on(async move {
        if let Err(e) = ogma_core::mcp::serve(storage).await {
            eprintln!("mcp server error: {e}");
            std::process::exit(1);
        }
    });
}

/// App data dir without a Tauri app instance (MCP mode). Must match Tauri's
/// `app_data_dir` for the identifier in tauri.conf.json.
fn dirs_data_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))
    };
    base.map(|b| b.join("com.davidruiz.ogma"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn supported_audio_accepts_known_exts_case_insensitively() {
        assert!(is_supported_audio(Path::new("a.wav")));
        assert!(is_supported_audio(Path::new("a.M4A")));
        assert!(is_supported_audio(Path::new("a.Mp3")));
        assert!(!is_supported_audio(Path::new("notes.txt")));
        assert!(!is_supported_audio(Path::new("no_extension")));
        // A directory dropped alongside audio has no audio extension.
        assert!(!is_supported_audio(Path::new("some_dir")));
        // Not fooled by an extension-looking name in a parent component.
        assert!(!is_supported_audio(Path::new("wav/readme.md")));
    }

    #[tokio::test]
    async fn import_many_counts_every_success() {
        let summary =
            import_many(paths(&["a.wav", "b.wav"]), |_| async { Ok("id".to_string()) }).await;
        assert_eq!(summary.imported, 2);
        assert!(summary.errors.is_empty());
    }

    #[tokio::test]
    async fn import_many_reports_every_failure_and_imports_nothing() {
        let summary =
            import_many(paths(&["a.wav", "b.wav"]), |_| async { Err("boom".to_string()) }).await;
        assert_eq!(summary.imported, 0);
        assert_eq!(summary.errors.len(), 2);
        assert!(summary.errors.iter().all(|e| e.contains("boom")));
    }

    /// The PR's central claim: one bad file must not abort the batch.
    #[tokio::test]
    async fn import_many_keeps_going_past_a_bad_file() {
        let attempted = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&attempted);
        let summary = import_many(paths(&["good1.wav", "bad.wav", "good2.wav"]), move |src| {
            let seen = Arc::clone(&seen);
            async move {
                seen.lock().unwrap().push(src.clone());
                if src == Path::new("bad.wav") {
                    Err("decode failed".to_string())
                } else {
                    Ok("id".to_string())
                }
            }
        })
        .await;

        // Both good files still imported, and the bad one didn't short-circuit.
        assert_eq!(summary.imported, 2);
        assert_eq!(attempted.lock().unwrap().len(), 3);
        // Exactly one error line, labeled with the offending file name.
        assert_eq!(summary.errors, vec!["bad.wav: decode failed".to_string()]);
    }

    #[tokio::test]
    async fn import_many_labels_errors_by_file_name_not_full_path() {
        let summary = import_many(paths(&["/tmp/deep/nested/take one.mp3"]), |_| async {
            Err("unsupported".to_string())
        })
        .await;
        assert_eq!(summary.errors, vec!["take one.mp3: unsupported".to_string()]);
    }

    #[tokio::test]
    async fn import_many_on_empty_input_is_a_silent_no_op() {
        let summary = import_many(Vec::new(), |_| async { Ok("id".to_string()) }).await;
        assert_eq!(summary.imported, 0);
        assert!(summary.errors.is_empty());
    }

    /// A mixed drop: `import_dropped`'s filter keeps only audio, so non-audio
    /// items never reach the importer and never produce an error line.
    #[tokio::test]
    async fn dropped_filter_passes_only_audio_to_the_importer() {
        let dropped = paths(&["talk.wav", "notes.txt", "cover.png", "call.m4a"]);
        let audio: Vec<PathBuf> =
            dropped.into_iter().filter(|p| is_supported_audio(p)).collect();
        assert_eq!(audio, paths(&["talk.wav", "call.m4a"]));

        let imported = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&imported);
        let summary = import_many(audio, move |src| {
            let seen = Arc::clone(&seen);
            async move {
                seen.lock().unwrap().push(src);
                Ok("id".to_string())
            }
        })
        .await;

        assert_eq!(summary.imported, 2);
        assert!(summary.errors.is_empty());
        assert_eq!(*imported.lock().unwrap(), paths(&["talk.wav", "call.m4a"]));
    }
}
