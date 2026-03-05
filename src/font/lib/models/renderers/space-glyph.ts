import Renderer from "../renderer";

export type SpaceGlyph = {
  empty: true;
  advance: 4;
} & Renderer;
export default SpaceGlyph;
