//! Tauri app layer: commands + events over ogma-core.

#[cfg(desktop)]
mod tray;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    data_dir: PathBuf,
}

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
fn start_recording(app: AppHandle, title: Option<String>) -> CmdResult<Meeting> {
    do_start_recording(&app, title)
}

/// Shared by the `start_recording` command, the tray menu and the global
/// shortcut, so recording can begin without the window being open.
fn do_start_recording(app: &AppHandle, title: Option<String>) -> CmdResult<Meeting> {
    let state = app.state::<AppState>();
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

#[tauri::command]
async fn stop_recording(app: AppHandle) -> CmdResult<String> {
    do_stop_recording(app).await
}

/// Shared by the `stop_recording` command, the tray menu, the global shortcut
/// and Quit — finalizes the audio and kicks off the pipeline.
async fn do_stop_recording(app: AppHandle) -> CmdResult<String> {
    let state = app.state::<AppState>();
    let session = {
        let mut active = state.recording.lock().unwrap();
        active.take().ok_or("no active recording")?
    };
    update_tray(&app, false);
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
    spawn_pipeline(&app, meeting_id.clone());
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

// ── tray & global shortcut ──────────────────────────────────────────────────

/// Start if idle, stop if recording — the tray/global-shortcut entry point.
/// Errors are logged, not surfaced: there may be no window to show them in.
fn toggle_recording(app: AppHandle) {
    let recording = app.state::<AppState>().recording.lock().unwrap().is_some();
    tauri::async_runtime::spawn(async move {
        let result = if recording {
            do_stop_recording(app).await.map(|_| ())
        } else {
            do_start_recording(&app, None).map(|_| ())
        };
        if let Err(e) = result {
            tracing::warn!("toggle recording failed: {e}");
        }
    });
}

/// Quit from the tray: finalize any active recording first so the audio and
/// meeting status are safe (the pipeline resumes via Retry on next launch).
fn quit(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let recording = app.state::<AppState>().recording.lock().unwrap().is_some();
        if recording {
            let _ = do_stop_recording(app.clone()).await;
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
    let progress_app = app.clone();
    let on_progress: ogma_core::pipeline::ProgressCallback = Arc::new(move |event: ProgressEvent| {
        let _ = progress_app.emit("meeting:progress", event.clone());
        let _ = progress_app.emit("meetings:changed", ());
    });
    tauri::async_runtime::spawn(async move {
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
    for status in [
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
                data_dir,
            });
            #[cfg(desktop)]
            {
                tray::init(app.handle())?;
                init_global_shortcut(app.handle());
            }
            Ok(())
        })
        // Closing the window hides to the tray so quick-start keeps working;
        // Quit lives in the tray menu.
        .on_window_event(|window, event| {
            #[cfg(desktop)]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
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
            stop_recording,
            discard_recording,
            retry_pipeline,
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
