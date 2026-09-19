import { ref, watch } from "vue";
import type { LengthUnit } from "~/types/printSpec";

const KEY = "pbg.printUnit";

function stored(): LengthUnit {
  try {
    return localStorage.getItem(KEY) === "mm" ? "mm" : "in";
  } catch {
    return "in";
  }
}

// Module-level so the footer label and the panel flip together.
const unit = ref<LengthUnit>(stored());
watch(unit, (next) => {
  try {
    localStorage.setItem(KEY, next);
  } catch {
    // A display preference; losing it costs one click.
  }
});

/**
 * Inches or millimetres for every print length on screen. A display
 * preference only: it is never written into a `PrintSpec`, so flipping it can
 * never change a book.
 */
export function usePrintUnit() {
  return unit;
}
