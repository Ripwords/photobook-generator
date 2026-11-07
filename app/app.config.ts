export default defineAppConfig({
  ui: {
    colors: {
      // Black and white. `--ui-primary` is pinned to neutral-900 / neutral-50
      // in main.css, so a primary button is black in light and white in dark.
      primary: "neutral",
      neutral: "neutral",
    },
    // Tooltips carry real sentences here (a tier's reason, what a drag does),
    // so they read at 14 px and wrap instead of truncating at one line.
    tooltip: {
      slots: {
        content: "h-auto min-h-7 max-w-80 py-1 text-sm",
        text: "whitespace-normal",
      },
    },
  },
});
