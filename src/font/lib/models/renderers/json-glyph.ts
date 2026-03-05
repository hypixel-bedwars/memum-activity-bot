import BitmapGlyph from "./bitmap-glyph";

// This type should enumerate the variants exportable by `create`
// functions in `/src/lib/glyph-providers/`.
//
// At time of writing, there's only `bitmap.ts`, so this should be fine.
export type JSONGlyph = BitmapGlyph;
