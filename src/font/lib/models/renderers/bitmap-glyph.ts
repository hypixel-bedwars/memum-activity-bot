import Renderer from "../renderer";

export interface BitmapGlyph extends Renderer {
  scale: number;
  offsetX: number;
  offsetY: number;
  width: number;
  height: number;
  advance: number;
  ascent: number;
  boldOffset?: number;
  shadowOffset?: number;
}
export default BitmapGlyph;
