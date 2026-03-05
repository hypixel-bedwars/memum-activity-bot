import { SPACE_GLYPH, MISSING_GLYPH } from "./constants";
import Provider from "./models/provider";
import Renderer from "./models/renderer";
import MissingGlyph from "./models/renderers/missing-glyph";
import SpaceGlyph from "./models/renderers/space-glyph";
import Canvas from "canvas";

export function createFontSet<T extends Renderer>(
  providers: Provider<T>[],
): Provider<T | SpaceGlyph | MissingGlyph> {
  return {
    getGlyph(char: string) {
      if (char === " ") {
        return SPACE_GLYPH;
      }

      for (const provider of providers) {
        let glyph = provider.getGlyph(char);

        if (glyph) {
          return glyph;
        }
      }

      return MISSING_GLYPH;
    },
  };
}

export async function initFontSet(): Promise<void> {
  const { width, height } = MISSING_GLYPH;
  const data: number[] = [];

  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const index = (x + y * width) * 4;
      const value =
        x == 0 || x + 1 == width || y == 0 || y + 1 == height ? 255 : 0;

      data[index + 0] = value; // r
      data[index + 1] = value; // g
      data[index + 2] = value; // b
      data[index + 3] = value; // a
    }
  }

  const imageData = Canvas.createImageData(
    Uint8ClampedArray.from(data),
    width,
    height,
  );
  const canvas = Canvas.createCanvas(width, height);
  const context = canvas.getContext("2d");

  context.putImageData(imageData, 0, 0);

  MISSING_GLYPH.image = await Canvas.loadImage(canvas.toBuffer());
}
