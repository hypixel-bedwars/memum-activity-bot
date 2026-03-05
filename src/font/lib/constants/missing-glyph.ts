import MissingGlyph from "../models/renderers/missing-glyph";
import canvas from "canvas";

export const MISSING_GLYPH: MissingGlyph = {
  width: 5,
  height: 8,
  advance: 6,
  image: null,
  render(context, x, y, color) {
    if (this.image === null) {
      throw "MISSING_GLYPH not yet defined!";
    }

    const { width, height } = this;
    const bufferCanvas = canvas.createCanvas(width, height);
    const bufferContext = bufferCanvas.getContext("2d");

    bufferContext.imageSmoothingEnabled = false;
    bufferContext.fillStyle = color;
    bufferContext.fillRect(0, 0, width, height);
    bufferContext.globalCompositeOperation = "destination-in";
    bufferContext.drawImage(this.image, 0, 0);

    context.drawImage(bufferCanvas, x, y);
  },
};
