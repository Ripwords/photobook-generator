/**
 * Mirrors `agent::view` in Rust: the only shape of a book a model is shown.
 * Built in Rust because Rust holds the per-photo features; this side only
 * reads it. Pinned against `tests/fixtures/wire/agent-view.json` from both
 * languages.
 */

/** Mirrors `agent::view::AgentSlot`, named the way `PlacementRef` names a placement. */
export interface AgentSlot {
  /** Printed page number. */
  page: number;
  z: number;
  /** An `AgentPhoto.id`. */
  photo: number;
}

/** Mirrors `agent::view::AgentOpening`, numbered as `book::edit` numbers openings. */
export interface AgentOpening {
  index: number;
  /** Printed page numbers, in reading order. */
  pages: number[];
  templateId: string;
  locked: boolean;
  alternatives: string[];
  slots: AgentSlot[];
}

/** Mirrors `agent::view::AgentPhoto`. */
export interface AgentPhoto {
  /** The `photoIndex` `BookLayout` uses. */
  id: number;
  /** Whole days since the book's earliest dated photo; `null` when undated. */
  day: number | null;
  event: number;
  faces: number;
  faceArea: number;
  /** At most three, in Vision's order. */
  tags: string[];
  /** Percentiles within this book's photos. */
  aestheticPct: number;
  sharpnessPct: number;
  placed: boolean;
}

/** Mirrors `agent::view::AgentView`. */
export interface AgentView {
  pageCount: number;
  placedPhotos: number;
  droppedPhotos: number;
  openings: AgentOpening[];
  photos: AgentPhoto[];
}
