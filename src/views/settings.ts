import { api, errorMessage } from "../api";
import { h } from "../dom";
import { toast } from "../toast";
import type { Config } from "../types";
import type { Navigate, View } from "../view";

/** Pull a 32-hex Notion id out of a pasted URL or raw id. */
export function extractNotionId(input: string): string {
  const withoutQuery = input.trim().split("?")[0];
  const dashless = withoutQuery.replace(/-/g, "");
  const matches = dashless.match(/[0-9a-f]{32}/gi);
  if (matches && matches.length > 0) return matches[matches.length - 1];
  return input.trim();
}

function secretField(label: string, value: string): { row: HTMLElement; input: HTMLInputElement } {
  const input = h("input", { type: "password", value, autocomplete: "off", spellcheck: false });
  const toggle = h("button", { class: "btn btn-small", type: "button" }, "Show");
  toggle.addEventListener("click", () => {
    const showing = input.type === "text";
    input.type = showing ? "password" : "text";
    toggle.textContent = showing ? "Show" : "Hide";
  });
  const row = h(
    "label",
    { class: "field" },
    h("span", { class: "field-label" }, label),
    h("div", { class: "field-input-row" }, input, toggle),
  );
  return { row, input };
}

function textField(
  label: string,
  value: string,
  placeholder: string,
  hint?: string,
): { row: HTMLElement; input: HTMLInputElement } {
  const input = h("input", { type: "text", value, placeholder, spellcheck: false });
  const row = h(
    "label",
    { class: "field" },
    h("span", { class: "field-label" }, label),
    input,
    hint ? h("span", { class: "field-hint" }, hint) : null,
  );
  return { row, input };
}

/**
 * Microphone picker. Lists enumerated input devices plus a "System default"
 * option (empty value). If the saved device isn't currently connected it's
 * still shown (suffixed) so the user can see what's configured.
 */
function deviceField(
  label: string,
  saved: string,
  devices: string[],
  hint: string,
): { row: HTMLElement; select: HTMLSelectElement } {
  const options = [h("option", { value: "" }, "System default (auto)")];
  for (const name of devices) {
    options.push(h("option", { value: name }, name));
  }
  if (saved && !devices.includes(saved)) {
    options.push(h("option", { value: saved }, `${saved} (not connected)`));
  }
  const select = h("select", { class: "field-select" }, ...options);
  select.value = saved;
  const row = h(
    "label",
    { class: "field" },
    h("span", { class: "field-label" }, label),
    select,
    h("span", { class: "field-hint" }, hint),
  );
  return { row, select };
}

export function renderSettings(navigate: Navigate): View {
  const el = h("div", { class: "view view-settings" }, h("div", { class: "empty" }, "Loading…"));

  async function build() {
    let config: Config;
    try {
      config = await api.getSettings();
    } catch (e) {
      el.replaceChildren(h("div", { class: "empty" }, `Could not load settings: ${errorMessage(e)}`));
      return;
    }

    // Best-effort: an enumeration failure shouldn't block the settings screen.
    let devices: string[] = [];
    try {
      devices = await api.listInputDevices();
    } catch {
      devices = [];
    }

    const openai = secretField("OpenAI API key (Whisper transcription)", config.openai_api_key);
    const anthropic = secretField("Anthropic API key (notes & speakers)", config.anthropic_api_key);
    const notion = secretField("Notion API key (integration token)", config.notion_api_key);
    const notionDb = textField(
      "Notion database ID",
      config.notion_database_id,
      "leave empty to skip Notion sync",
      "Paste an existing Meetings database ID, or create one below.",
    );
    const notesModel = textField("Notes model", config.notes_model, "claude-sonnet-5");
    const whisperModel = textField("Transcription model", config.whisper_model, "whisper-1");
    const language = textField(
      "Language hint",
      config.language,
      "auto-detect",
      "Optional ISO code like en, es — helps Whisper with accents.",
    );
    const inputDevice = deviceField(
      "Microphone",
      config.input_device,
      devices,
      "Which input device to record from. Takes effect on your next recording.",
    );

    const saveBtn = h("button", { class: "btn btn-primary" }, "Save settings");
    saveBtn.addEventListener("click", async () => {
      saveBtn.disabled = true;
      try {
        const next: Config = {
          openai_api_key: openai.input.value.trim(),
          anthropic_api_key: anthropic.input.value.trim(),
          notion_api_key: notion.input.value.trim(),
          notion_database_id: extractNotionId(notionDb.input.value),
          notes_model: notesModel.input.value.trim(),
          whisper_model: whisperModel.input.value.trim(),
          language: language.input.value.trim(),
          input_device: inputDevice.select.value,
        };
        await api.saveSettings(next);
        toast("Settings saved", "success");
      } catch (e) {
        toast(errorMessage(e), "error");
      } finally {
        saveBtn.disabled = false;
      }
    });

    // Notion database creation helper
    const parentInput = h("input", {
      type: "text",
      placeholder: "Notion parent page URL or ID",
      spellcheck: false,
    });
    const createBtn = h("button", { class: "btn" }, "Create Meetings database");
    createBtn.addEventListener("click", async () => {
      const raw = parentInput.value.trim();
      if (!raw) {
        toast("Paste the URL of a Notion page shared with your integration", "error");
        return;
      }
      createBtn.disabled = true;
      try {
        // Make sure the token being used is the one on screen.
        await api.saveSettings({
          openai_api_key: openai.input.value.trim(),
          anthropic_api_key: anthropic.input.value.trim(),
          notion_api_key: notion.input.value.trim(),
          notion_database_id: extractNotionId(notionDb.input.value),
          notes_model: notesModel.input.value.trim(),
          whisper_model: whisperModel.input.value.trim(),
          language: language.input.value.trim(),
          input_device: inputDevice.select.value,
        });
        const dbId = await api.notionSetup(extractNotionId(raw));
        notionDb.input.value = dbId;
        toast("Meetings database created and linked", "success");
      } catch (e) {
        toast(errorMessage(e), "error");
      } finally {
        createBtn.disabled = false;
      }
    });

    el.replaceChildren(
      h(
        "div",
        { class: "detail-header" },
        h("button", { class: "btn btn-small", onclick: () => navigate({ name: "home" }) }, "← Back"),
        h("h1", { class: "settings-title" }, "Settings"),
      ),
      h(
        "section",
        { class: "card settings-card" },
        h("h2", null, "API keys"),
        openai.row,
        anthropic.row,
        notion.row,
        h(
          "p",
          { class: "field-hint" },
          "Keys are stored locally in config.json in the app data folder and only sent to their respective APIs.",
        ),
      ),
      h(
        "section",
        { class: "card settings-card" },
        h("h2", null, "Notion"),
        notionDb.row,
        h(
          "div",
          { class: "field" },
          h("span", { class: "field-label" }, "…or create a new database"),
          h("div", { class: "field-input-row" }, parentInput, createBtn),
          h(
            "span",
            { class: "field-hint" },
            "Share a Notion page with your integration, paste its URL here, and Ogma will create a Meetings database inside it.",
          ),
        ),
      ),
      h(
        "section",
        { class: "card settings-card" },
        h("h2", null, "Recording"),
        inputDevice.row,
      ),
      h(
        "section",
        { class: "card settings-card" },
        h("h2", null, "Models"),
        notesModel.row,
        whisperModel.row,
        language.row,
      ),
      h("div", { class: "settings-actions" }, saveBtn),
    );
  }

  void build();
  return { el };
}
