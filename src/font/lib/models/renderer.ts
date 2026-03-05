import {
  CanvasGradient,
  CanvasPattern,
  CanvasRenderingContext2D,
} from "canvas";

export interface Renderer {
  empty?: boolean;
  render(
    context: CanvasRenderingContext2D,
    x: number,
    y: number,
    color: string | CanvasGradient | CanvasPattern,
  ): void;
}
export default Renderer;
