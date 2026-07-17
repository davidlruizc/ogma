import { getVersion } from "@tauri-apps/api/app";
import { api, errorMessage } from "../api";
import { h } from "../dom";
import { toast } from "../toast";
import type { Config } from "../types";
import { checkForUpdate, installUpdate, type Update } from "../updater";
import type { Navigate, View } from "../view";

/** Pull a 32-hex Notion id out of a pasted URL or raw id. */
export function extractNotionId(input: string): string {
  const withoutQuery = input.trim().split("?")[0];
  const dashless = withoutQuery.replace(/-/g, "");
  const matches = dashless.match(/[0-9a-f]{32}/gi);
  if (matches && matches.length > 0) return matches[matches.length - 1];
  return input.trim();
}

function keyRow(label: string, value: string): { row: HTMLElement; input: HTMLInputElement } {
  const input = h("input", {
    class: "key-input",
    type: "password",
    value,
    autocomplete: "off",
    spellcheck: false,
  });
  const valid = h("span", { class: `key-valid ${value ? "" : "off"}` });
  const dot = h("span", { class: `status-dot ${value ? "ok" : "muted"}` });
  valid.append(dot, value ? "valid" : "empty");
  input.addEventListener("input", () => {
    const ok = input.value.trim() !== "";
    valid.replaceChildren(h("span", { class: `status-dot ${ok ? "ok" : "muted"}` }), ok ? "valid" : "empty");
    valid.className = `key-valid ${ok ? "" : "off"}`;
  });
  const row = h("div", { class: "key-row" }, h("span", { class: "key-label" }, label), input, valid);
  return { row, input };
}

function field(label: string, value: string, placeholder: string, hint?: string): { row: HTMLElement; input: HTMLInputElement } {
  const input = h("input", { class: "field-input", type: "text", value, placeholder, spellcheck: false });
  const row = h(
    "div",
    { class: "field" },
    h("span", { class: "field-label" }, label),
    input,
    hint ? h("span", { class: "field-hint" }, hint) : null,
  );
  return { row, input };
}

export function renderSettings(_navigate: Navigate): View {
  const el = h("div", { class: "screen screen-pad" }, h("div", { class: "empty" }, "Loading…"));

  async function build() {
    let config: Config;
    try {
      config = await api.getSettings();
    } catch (e) {
      el.replaceChildren(h("div", { class: "empty" }, `Could not load settings: ${errorMessage(e)}`));
      return;
    }

    let devices: string[] = [];
    try {
      devices = await api.listInputDevices();
    } catch {
      devices = [];
    }

    let platform = "";
    try {
      platform = await api.getPlatform();
    } catch {
      platform = "";
    }

    // ── input device (radio list) ───────────────────────────────────────────
    let selectedDevice = config.input_device;
    const deviceList = h("div", { class: "device-list" });

    const deviceEntries: { value: string; label: string; sub: string }[] = [
      { value: "", label: "System default (auto)", sub: "let the OS pick the active input" },
      ...devices.map((d) => ({ value: d, label: d, sub: "connected input device" })),
    ];
    if (selectedDevice && !devices.includes(selectedDevice)) {
      deviceEntries.push({ value: selectedDevice, label: selectedDevice, sub: "not currently connected" });
    }

    function paintDevices() {
      deviceList.replaceChildren();
      for (const dev of deviceEntries) {
        const selected = selectedDevice === dev.value;
        const meter = h("div", { class: "device-meter" });
        for (let i = 0; i < 7; i++) {
          meter.append(h("span", { style: `animation-delay:${(i * 0.11).toFixed(2)}s` }));
        }
        deviceList.append(
          h(
            "div",
            {
              class: `device-row ${selected ? "selected" : ""}`,
              onclick: () => {
                selectedDevice = dev.value;
                paintDevices();
                toast(`Input device → ${dev.label}`);
              },
            },
            h("span", { class: "device-radio" }),
            h(
              "div",
              { class: "device-main" },
              h("span", { class: "device-label" }, dev.label),
              h("span", { class: "device-sub" }, dev.sub),
            ),
            meter,
          ),
        );
      }
    }
    paintDevices();

    // ── keys / models / notion ──────────────────────────────────────────────
    const openai = keyRow("OpenAI", config.openai_api_key);
    const anthropic = keyRow("Anthropic", config.anthropic_api_key);
    const notion = keyRow("Notion token", config.notion_api_key);

    const notionDb = field(
      "Notion database ID",
      config.notion_database_id,
      "leave empty to skip Notion sync",
      "Paste an existing Meetings database ID, or create one below.",
    );
    // ── extra sync destinations ─────────────────────────────────────────────
    const markdownDirInput = h("input", {
      class: "field-input",
      type: "text",
      value: config.markdown_dir,
      placeholder: "leave empty to disable",
      spellcheck: false,
    });
    const browseBtn = h("button", { class: "pill" }, "BROWSE…");
    browseBtn.addEventListener("click", async () => {
      browseBtn.disabled = true;
      try {
        const dir = await api.pickFolder();
        if (dir) markdownDirInput.value = dir;
      } catch (e) {
        toast(errorMessage(e), "error");
      } finally {
        browseBtn.disabled = false;
      }
    });

    // Apple Notes toggle — macOS only (AppleScript doesn't exist elsewhere).
    let appleNotesEnabled = config.apple_notes_enabled;
    const appleNotesRow = h("div", { class: "device-list" });
    function paintAppleNotes() {
      appleNotesRow.replaceChildren(
        h(
          "div",
          {
            class: `device-row ${appleNotesEnabled ? "selected" : ""}`,
            onclick: () => {
              appleNotesEnabled = !appleNotesEnabled;
              paintAppleNotes();
            },
          },
          h("span", { class: "device-radio" }),
          h(
            "div",
            { class: "device-main" },
            h("span", { class: "device-label" }, "Apple Notes"),
            h(
              "span",
              { class: "device-sub" },
              "one note per meeting in an “Ogma” folder · reaches iPhone via iCloud · macOS asks for permission on first sync",
            ),
          ),
          h(
            "span",
            { class: `key-valid ${appleNotesEnabled ? "" : "off"}` },
            h("span", { class: `status-dot ${appleNotesEnabled ? "ok" : "muted"}` }),
            appleNotesEnabled ? "enabled" : "off",
          ),
        ),
      );
    }
    paintAppleNotes();

    const notesModel = field("Notes model", config.notes_model, "claude-sonnet-5");
    const whisperModel = field("Transcription model", config.whisper_model, "whisper-1");
    const language = field("Language hint", config.language, "auto-detect", "Optional ISO code like en, es — helps Whisper with accents.");

    function currentConfig(): Config {
      return {
        openai_api_key: openai.input.value.trim(),
        anthropic_api_key: anthropic.input.value.trim(),
        notion_api_key: notion.input.value.trim(),
        notion_database_id: extractNotionId(notionDb.input.value),
        markdown_dir: markdownDirInput.value.trim(),
        apple_notes_enabled: appleNotesEnabled,
        notes_model: notesModel.input.value.trim(),
        whisper_model: whisperModel.input.value.trim(),
        language: language.input.value.trim(),
        input_device: selectedDevice,
      };
    }

    const saveBtn = h("button", { class: "pill pill-accent" }, "SAVE SETTINGS");
    saveBtn.addEventListener("click", async () => {
      saveBtn.disabled = true;
      try {
        await api.saveSettings(currentConfig());
        toast("Settings saved", "success");
        window.dispatchEvent(new CustomEvent("ogma:settings-saved"));
      } catch (e) {
        toast(errorMessage(e), "error");
      } finally {
        saveBtn.disabled = false;
      }
    });

    // Notion database creation helper
    const parentInput = h("input", { class: "field-input", type: "text", placeholder: "Notion parent page URL or ID", spellcheck: false });
    const createBtn = h("button", { class: "pill" }, "CREATE DATABASE");
    createBtn.addEventListener("click", async () => {
      const raw = parentInput.value.trim();
      if (!raw) {
        toast("Paste the URL of a Notion page shared with your integration", "error");
        return;
      }
      createBtn.disabled = true;
      try {
        await api.saveSettings(currentConfig());
        const dbId = await api.notionSetup(extractNotionId(raw));
        notionDb.input.value = dbId;
        toast("Meetings database created and linked", "success");
        window.dispatchEvent(new CustomEvent("ogma:settings-saved"));
      } catch (e) {
        toast(errorMessage(e), "error");
      } finally {
        createBtn.disabled = false;
      }
    });

    // ── OTA updates ─────────────────────────────────────────────────────────
    let appVersion = "";
    try {
      appVersion = await getVersion();
    } catch {
      appVersion = "";
    }

    let pendingUpdate: Update | null = null;
    const updateStatus = h("span", { class: "field-hint" }, "Checked automatically on startup · installed only when you say so.");
    const checkUpdateBtn = h("button", { class: "pill" }, "CHECK FOR UPDATES");
    const installUpdateBtn = h("button", { class: "pill pill-accent" }, "INSTALL & RESTART");
    installUpdateBtn.style.display = "none";

    checkUpdateBtn.addEventListener("click", async () => {
      checkUpdateBtn.disabled = true;
      updateStatus.textContent = "Checking…";
      try {
        pendingUpdate = await checkForUpdate();
        if (pendingUpdate) {
          updateStatus.textContent = `Ogma ${pendingUpdate.version} is available (you have ${appVersion}).`;
          installUpdateBtn.style.display = "";
        } else {
          updateStatus.textContent = `You're on the latest version (${appVersion}).`;
        }
      } catch (e) {
        updateStatus.textContent = `Could not check for updates: ${errorMessage(e)}`;
      } finally {
        checkUpdateBtn.disabled = false;
      }
    });

    installUpdateBtn.addEventListener("click", async () => {
      if (!pendingUpdate) return;
      // A restart mid-recording would lose the active segment. Commit to the
      // install on the backend: it atomically refuses if a recording is active
      // and otherwise latches a flag that blocks any recording from starting
      // (global shortcut / tray included) for the whole download→relaunch
      // window — closing the TOCTOU gap a one-shot state read would leave open.
      try {
        await api.beginUpdateInstall();
      } catch (e) {
        toast(errorMessage(e), "error");
        return;
      }
      installUpdateBtn.disabled = true;
      checkUpdateBtn.disabled = true;
      try {
        await installUpdate(pendingUpdate, ({ downloaded, total }) => {
          const pct = total ? ` ${Math.round((downloaded / total) * 100)}%` : "";
          updateStatus.textContent = `Downloading${pct}…`;
        });
        updateStatus.textContent = "Update installed — restarting…";
      } catch (e) {
        // Install failed — release the recording lock so the app stays usable.
        await api.cancelUpdateInstall().catch(() => {});
        toast(errorMessage(e), "error");
        updateStatus.textContent = `Update failed: ${errorMessage(e)}`;
        installUpdateBtn.disabled = false;
        checkUpdateBtn.disabled = false;
      }
    });

    const notionConnected = config.notion_api_key.trim() !== "" && config.notion_database_id.trim() !== "";

    el.replaceChildren(
      h("div", { class: "screen-title" }, "Settings"),

      // input device
      h(
        "div",
        { class: "card settings-card" },
        h("div", { class: "section-label" }, "INPUT DEVICE — microphone"),
        deviceList,
        h("div", { class: "field-hint" }, "16 kHz mono downmix · takes effect on your next recording"),
      ),

      // api keys
      h(
        "div",
        { class: "card settings-card" },
        h("div", { class: "section-label" }, "API KEYS — stored locally in config.json"),
        openai.row,
        anthropic.row,
        notion.row,
        h("div", { class: "field-hint" }, "Keys are sent only to their respective APIs · whisper-1 · claude-sonnet-5"),
      ),

      // notion
      h(
        "div",
        { class: "card settings-card" },
        h("div", { class: "section-label" }, "NOTION — canonical store"),
        h(
          "div",
          { class: "notion-db" },
          h("span", { class: "notion-glyph" }, "N"),
          h(
            "div",
            { class: "device-main" },
            h("span", { class: "device-label" }, "Meetings database"),
            h("span", { class: "device-sub" }, notionConnected ? "linked · pages sync automatically" : "not linked yet"),
          ),
          h("span", { class: "flex-spacer" }),
          h(
            "span",
            { class: `key-valid ${notionConnected ? "" : "off"}` },
            h("span", { class: `status-dot ${notionConnected ? "ok" : "muted"}` }),
            notionConnected ? "connected" : "offline",
          ),
        ),
        notionDb.row,
        h(
          "div",
          { class: "field" },
          h("span", { class: "field-label" }, "…or create a new database"),
          h("div", { class: "field-row" }, parentInput, createBtn),
          h(
            "span",
            { class: "field-hint" },
            "Share a Notion page with your integration, paste its URL here, and Ogma creates a Meetings database inside it.",
          ),
        ),
      ),

      // extra sync destinations
      h(
        "div",
        { class: "card settings-card" },
        h("div", { class: "section-label" }, "DESTINATIONS — beyond Notion"),
        h(
          "div",
          { class: "field" },
          h("span", { class: "field-label" }, "Markdown / Obsidian folder"),
          h("div", { class: "field-row" }, markdownDirInput, browseBtn),
          h(
            "span",
            { class: "field-hint" },
            "Each processed meeting is written as a .md file with YAML frontmatter — point this at an Obsidian vault folder to get meetings in your vault.",
          ),
        ),
        platform === "macos" ? appleNotesRow : null,
      ),

      // models
      h(
        "div",
        { class: "card settings-card" },
        h("div", { class: "section-label" }, "MODELS"),
        notesModel.row,
        whisperModel.row,
        language.row,
      ),

      // mcp
      h(
        "div",
        { class: "card settings-card" },
        h("div", { class: "section-label" }, "MCP SERVER — query meetings from Claude"),
        h("div", { class: "mono-block" }, "claude mcp add ogma -- ogma --mcp"),
        h(
          "div",
          { class: "field-hint" },
          "4 tools · list_meetings · search_transcript · get_meeting_notes · get_action_items",
        ),
      ),

      // updates
      h(
        "div",
        { class: "card settings-card" },
        h("div", { class: "section-label" }, `UPDATES — Ogma ${appVersion || "?"}`),
        h("div", { class: "field-row" }, checkUpdateBtn, installUpdateBtn),
        updateStatus,
      ),

      h("div", { class: "settings-actions" }, saveBtn),
    );
  }

  void build();
  return { el };
}
