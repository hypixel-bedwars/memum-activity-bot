import { convertHexToString } from "../helpers";
import { ApplyEmpty } from "../models/apply";
import { ChatFormatting, RawFormatting } from "../models/chat-formatting";
import Color from "../models/color";

// Ernie Note: I hate people who use javascript as a funclang
function generate_chat_formattings(): Record<string, ChatFormatting> {
  // Strangely, it seems that the `?` is necessary to specify a possibly unset value,
  // adding yet another entry to my knowledge stack of unset/undefined/null/void/etc
  //
  // This type just says "It has a `name` value, plus either `color` or `apply`, but not both"
  // Will be converted into a `ChatFormatting` object later.
  const base_data: Record<string | number, RawFormatting> = {
    0: {
      name: "BLACK",
      hexColor: 0,
    },
    1: {
      name: "DARK_BLUE",
      hexColor: 170,
    },
    2: {
      name: "DARK_GREEN",
      hexColor: 43520,
    },
    3: {
      name: "DARK_AQUA",
      hexColor: 43690,
    },
    4: {
      name: "DARK_RED",
      hexColor: 0xaa0000,
    },
    5: {
      name: "DARK_PURPLE",
      hexColor: 0xaa00aa,
    },
    6: {
      name: "GOLD",
      hexColor: 0xffaa00,
    },
    7: {
      name: "GRAY",
      hexColor: 0xaaaaaa,
    },
    8: {
      name: "DARK_GRAY",
      hexColor: 0x555555,
    },
    9: {
      name: "BLUE",
      hexColor: 0x5555ff,
    },
    a: {
      name: "GREEN",
      hexColor: 0x55ff55,
    },
    b: {
      name: "AQUA",
      hexColor: 0x55ffff,
    },
    c: {
      name: "RED",
      hexColor: 0xff5555,
    },
    d: {
      name: "LIGHT_PURPLE",
      hexColor: 0xff55ff,
    },
    e: {
      name: "YELLOW",
      hexColor: 0xffff55,
    },
    f: {
      name: "WHITE",
      hexColor: 0xffffff,
    },
    l: {
      name: "BOLD",
      apply(style) {
        return {
          ...style,
          bold: true,
        };
      },
    },
    r: {
      name: "RESET",
      apply() {
        return { ...EMPTY_STYLE };
      },
    },
  };

  let res: Record<string, ChatFormatting> = {};

  for (const [code, { name, hexColor: hexColor, apply }] of Object.entries(
    base_data,
  )) {
    let color: Color | undefined = undefined;

    if (typeof hexColor != "undefined") {
      color = {
        text: convertHexToString(hexColor),
        shadow: convertHexToString(hexColor * (63 / 255)),
      };
    }

    res[code] = {
      name,
      color,
      apply:
        apply ??
        (function () {
          return { ...EMPTY_STYLE, color: this.color };
          // fuckass language needs me to specify a value that should be obvious from the union type.
          // the ONLY reason that `ApplyEmpty<T>` exists is SPECIFICALLY for this invocation
        } as ApplyEmpty<ChatFormatting>),
    };
  }

  return res;
}

const CHAT_FORMATTINGS = generate_chat_formattings();
const EMPTY_STYLE = Object.freeze({
  color: CHAT_FORMATTINGS.f.color,
  bold: false,
});

export { CHAT_FORMATTINGS, EMPTY_STYLE };
