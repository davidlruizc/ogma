/** Tiny DOM builder: h("div", {class: "x", onclick}, child, "text"). */
type Props = Record<string, unknown> | null;
type Child = Node | string | null | undefined | false;

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props?: Props,
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (props) {
    for (const [key, value] of Object.entries(props)) {
      if (value == null || value === false) continue;
      if (key.startsWith("on") && typeof value === "function") {
        node.addEventListener(key.slice(2), value as EventListener);
      } else if (key === "class") {
        node.className = String(value);
      } else if (key === "dataset") {
        Object.assign(node.dataset, value);
      } else if (key === "style") {
        node.setAttribute("style", String(value));
      } else if (key in node && key !== "list" && key !== "form") {
        // Prefer property assignment (value, checked, disabled…)
        (node as unknown as Record<string, unknown>)[key] = value;
      } else {
        node.setAttribute(key, String(value));
      }
    }
  }
  for (const child of children) {
    if (child == null || child === false) continue;
    node.append(child instanceof Node ? child : document.createTextNode(child));
  }
  return node;
}

/**
 * Two-step destructive button: first click arms it ("Sure?") for 3s,
 * second click fires. Avoids native confirm() (unreliable in webviews).
 */
export function armedButton(
  label: string,
  armedLabel: string,
  className: string,
  action: () => void,
): HTMLButtonElement {
  let armed = false;
  let timer: number | undefined;
  const btn = h("button", { class: className }, label);
  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    if (!armed) {
      armed = true;
      btn.textContent = armedLabel;
      btn.classList.add("armed");
      timer = window.setTimeout(() => {
        armed = false;
        btn.textContent = label;
        btn.classList.remove("armed");
      }, 3000);
    } else {
      if (timer !== undefined) clearTimeout(timer);
      armed = false;
      btn.textContent = label;
      btn.classList.remove("armed");
      action();
    }
  });
  return btn;
}
