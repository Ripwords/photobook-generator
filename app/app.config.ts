export default defineAppConfig({
  ui: {
    colors: {
      // Brand palette (see app/assets/css/main.css for the full ramps and
      // ui-design-report.md for the rationale). Only primary and neutral are
      // registered as Nuxt UI semantic colours - steel is the one
      // interactive colour used through buttons/focus rings/links; charcoal
      // is neutral everywhere else. The other two brand accents (lavender
      // for structural dividers, sunlight for the hero tile) are
      // deliberately NOT registered here: they mark exactly two things in
      // the design, and giving them a generic `color="..."` slot on every
      // component would invite them to leak into places they don't belong.
      // They're applied directly as utility classes at their two call sites
      // instead (app/pages/index.vue, app/components/PhotoTile.vue).
      primary: "steel",
      neutral: "charcoal",
    },
  },
});
