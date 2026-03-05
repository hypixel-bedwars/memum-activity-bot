import { CHAT_FORMATTINGS, EMPTY_STYLE } from "./constants";
import Style from "./models/style";
import path from "path";

type AcceptFunction = { (style: Style, char: string): void };

export function iterateFormatted(text: string, accept: AcceptFunction) {
  let style: Style = { ...EMPTY_STYLE };

  for (let index = 0; index < text.length; index++) {
    const char: string = text[index]!;

    if (char === "§") {
      if (++index === text.length) {
        break;
      }

      const code = text[index]!;
      const formatting = CHAT_FORMATTINGS[code];

      if (formatting) {
        style = formatting.apply(style);
      }
    } else {
      accept(style, char);
    }
  }
}

/** Converts a number in hexadecimal format to string format. */
export function convertHexToString(color: number): string {
  return `#${color.toString(16).padStart(6, "0")}`;
}

/** Gets a directory relative to `src/font/`.
 *
 * If `sub` is supplied, it is interpreted as a subdirectory.
 * */
export function srcFontDir(...sub: string[]) {
  return path.join(__dirname, "..", ...sub);
}
