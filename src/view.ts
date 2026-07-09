export type Route =
  | { name: "record" }
  | { name: "library" }
  | { name: "detail"; meetingId: string }
  | { name: "settings" }
  | { name: "spike" };

export type Navigate = (route: Route) => void;

export interface View {
  el: HTMLElement;
  destroy?: () => void;
}
