export type Route =
  | { name: "record" }
  | { name: "library" }
  | { name: "detail"; meetingId: string }
  | { name: "settings" };

export type Navigate = (route: Route) => void;

export interface View {
  el: HTMLElement;
  destroy?: () => void;
}
