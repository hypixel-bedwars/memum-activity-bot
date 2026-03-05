import JSONGlyphData from "./json-glyph-data";
import Provider from "./provider";
import { JSONGlyph } from "./renderers/json-glyph";

export type CreateFunction = (
  arg: JSONGlyphData,
) => Promise<Provider<JSONGlyph>>;

export interface GlyphProvider {
  create: CreateFunction;
}
export default GlyphProvider;
