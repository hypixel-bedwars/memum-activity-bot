import { getFontSet } from "./font-manager";
import { iterateFormatted } from "./helpers";
import BitmapGlyph from "./models/renderers/bitmap-glyph";
import { CanvasRenderingContext2D } from "canvas";

export function fetchMinecraftAssets(): never {
  // Fetch the assets from the Minecraft cdn instead but im lazy as fuck
  throw "Not implemented!";
}

export function measureMinecraftText(text: string, font?: string) {
  const fontSet = getFontSet(font);
  if (fontSet === undefined) {
    throw Error(`font '${font}' is undefined`);
  }
  let width = 0;

  iterateFormatted(text, (style, char) => {
    const glyph = fontSet.getGlyph(char);
    const { advance, boldOffset = 1 } = glyph as BitmapGlyph;

    width += advance + (style.bold ? boldOffset : 0);
  });

  return width;
}

export function drawMinecraftText(
  context: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  scale: number = 1,
  font?: string,
) {
  const fontSet = getFontSet(font);

  if (fontSet === undefined) {
    throw Error(`font '${font}' is undefined`);
  }

  context.save();
  context.scale(scale, scale);
  context.imageSmoothingEnabled = false;

  iterateFormatted(text, (style, char) => {
    const glyph = fontSet.getGlyph(char);
    const { advance, boldOffset = 1, shadowOffset = 1 } = glyph as BitmapGlyph;

    if (!glyph.empty) {
      glyph.render(
        context,
        x + shadowOffset,
        y + shadowOffset,
        style.color!.shadow,
      );

      if (style.bold) {
        glyph.render(
          context,
          x + shadowOffset + boldOffset,
          y + shadowOffset,
          style.color!.shadow,
        );
      }

      glyph.render(context, x, y, style.color!.text);

      if (style.bold) {
        glyph.render(context, x + boldOffset, y, style.color!.text);
      }
    }

    x += advance + (style.bold ? boldOffset : 0);
  });

  context.restore();
}
