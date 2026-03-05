import Renderer from "./renderer";

export interface Provider<T extends Renderer> {
  getGlyph(char: string): T;
}
export default Provider;
