import { loadFontSets } from "./lib/font-manager";
import { drawMinecraftText } from "./lib/index";
import Canvas from "canvas";
import fs from "fs/promises";

const canvas = Canvas.createCanvas(200, 100);
const context = canvas.getContext("2d");

(async () => {
  await loadFontSets();
  drawMinecraftText(context, "§cHello world!", 0, 0, 2);
  drawMinecraftText(context, "§6§lBold!", 0, 20, 2);
  await fs.writeFile("example.png", canvas.toBuffer());
})();
