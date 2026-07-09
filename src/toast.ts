import { h } from "./dom";

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
