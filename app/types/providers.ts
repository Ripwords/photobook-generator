/** The print services a book can be sent to. Mirrors Rust's `ProviderId`. */
export type ProviderId = "pixajoy";

/** Mirrors `provider_status`. An expired sign-in reads as `signedIn: false`. */
export interface ProviderStatus {
  signedIn: boolean;
  email: string | null;
}

/** Mirrors `provider_send_plan`. `pageCount` is what Pixajoy receives, padded and even. */
export interface SendPlan {
  bookPages: number;
  pageCount: number;
}

/** Mirrors Rust's `SendEvent`, streamed over a `tauri::ipc::Channel`. */
export type SendEvent =
  | { kind: "exporting"; done: number; total: number }
  | { kind: "creating" }
  | { kind: "uploading"; done: number; total: number }
  | { kind: "layingOut" }
  | { kind: "done"; projectName: string; openUrl: string }
  | { kind: "failed"; message: string; projectName: string | null };

/** What the send dialog shows. `progress` is 0..1, or `null` for indeterminate. */
export type SendView =
  | { kind: "idle" }
  | { kind: "sending"; label: string; progress: number | null }
  | { kind: "done"; projectName: string; openUrl: string }
  | { kind: "failed"; message: string; leftover: string | null };

function counted(label: string, done: number, total: number): SendView {
  return {
    kind: "sending",
    label: `${label}, ${done} of ${total}`,
    progress: total > 0 ? done / total : null,
  };
}

/** The dialog's state follows from the latest event alone; `null` is before any send. */
export function sendView(event: SendEvent | null): SendView {
  if (event === null) return { kind: "idle" };
  switch (event.kind) {
    case "exporting":
      return counted("Exporting photos", event.done, event.total);
    case "creating":
      return { kind: "sending", label: "Creating the Pixajoy project", progress: null };
    case "uploading":
      return counted("Uploading photos", event.done, event.total);
    case "layingOut":
      return { kind: "sending", label: "Laying out the pages", progress: null };
    case "done":
      return { kind: "done", projectName: event.projectName, openUrl: event.openUrl };
    case "failed":
      return {
        kind: "failed",
        message: event.message,
        leftover:
          event.projectName === null
            ? null
            : `A partly filled project named “${event.projectName}” may be in My Projects.`,
      };
  }
}

const PIXAJOY_MIN_PAGES = 20;

/** The page count Pixajoy gets, and why it differs from the book's when it does. */
export function planNote(plan: SendPlan): { pages: string; padding: string | null } {
  const pages = `${plan.pageCount} pages`;
  const added = plan.pageCount - plan.bookPages;
  if (added <= 0) return { pages, padding: null };
  const reason =
    plan.bookPages < PIXAJOY_MIN_PAGES
      ? `Pixajoy needs at least ${PIXAJOY_MIN_PAGES}`
      : "Pixajoy needs an even count";
  const blanks = added === 1 ? "1 blank page is" : `${added} blank pages are`;
  return {
    pages,
    padding: `Your book has ${plan.bookPages} pages. ${reason}, so ${blanks} added at the end.`,
  };
}
