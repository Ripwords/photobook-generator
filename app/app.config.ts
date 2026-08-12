export default defineAppConfig({
  ui: {
    colors: {
      // Single restrained accent for the whole app: interactive elements,
      // active states, and the progress bar. Deliberately not the
      // AI-default purple/violet - blue reads as calm, native macOS chrome.
      primary: "blue",
      neutral: "slate",
    },
  },
});
