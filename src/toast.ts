import { h } from "./dom";
import type { ImportSummary } from "./types";

export function toast(message: string, kind: "info" | "error" | "success" = "info"): void {
  const container = document.getElementById("toasts");
  if (!container) return;
  const node = h("div", { class: `toast toast-${kind}` }, message);
  container.append(node);
  setTimeout(() => {
    node.classList.add("toast-out");
    setTimeout(() => node.remove(), 300);
  }, kind === "error" ? 6000 : 3000);
}

/** Toast the outcome of an audio import (picker or drag-and-drop). */
export function toastImportSummary(summary: ImportSummary): void {
  const { imported, errors } = summary;
  if (imported > 0) {
    const noun = imported === 1 ? "file" : "files";
    toast(`Imported ${imported} ${noun} — processing…`, "success");
  }
  for (const err of errors) toast(err, "error");
  // imported === 0 && errors.length === 0 means "picker cancelled" — stay quiet.
}
