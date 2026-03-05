import { Apply } from "./apply";

export type ChatFormatting = {
  name: string;
  color?: { text: string; shadow: string };
  apply: Apply<ChatFormatting>;
};
export default ChatFormatting;

/** A type used in `../constants/chat-formatting.ts`. Converted into `ChatFormatting`. */
export type RawFormatting = { name: string } & (
  | { hexColor: number; apply?: undefined }
  | { hexColor?: undefined; apply: Apply<ChatFormatting> }
);
