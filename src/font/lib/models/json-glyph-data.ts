/** JSON-formatted data for a Provider. Can be converted into a `JSONGlyph` (for now).
 *
 * Examples are available as `src/font/assets/font/*.json`.
 * */
export default interface JSONGlyphData {
  type: string;
  file: string;
  ascent: number;
  chars: string[];
}

interface JSONGlyphDataKeys {
  type: unknown;
  file: unknown;
  ascent: unknown;
  chars: unknown;
}

// See comment a couple dozen lines below.
function assertHasJSONProviderKeys(
  value: unknown,
): asserts value is JSONGlyphDataKeys {
  if (typeof value != "object") throw "value is not an object";
  if (value === null) throw "value is null";

  let missingKeys = ["type", "file", "ascent", "chars"].filter(
    (key) => !(key in value),
  );

  if (missingKeys.length)
    throw "object is missing keys: '" + missingKeys.join("', '") + "'";
}

const EXPECTED_JSON_KEYS: Array<[string, string]> = [
  ["type", "string"],
  ["file", "string"],
  ["ascent", "number"],
];

function assertKeysMatchJSONProvider(
  obj: JSONGlyphDataKeys,
): asserts obj is JSONGlyphData {
  let invalidTypes: [string, string][] = [];

  for (const [k, v] of EXPECTED_JSON_KEYS) {
    if (typeof (obj as any)[k] != v) {
      invalidTypes.push([k, typeof (obj as any)[k]]);
    }
  }

  let { chars } = obj as any;
  if (!Array.isArray(chars)) {
    invalidTypes.push(["chars", typeof chars]);
  } else {
    let types: Set<string> = new Set();
    for (const obj of chars) {
      types.add(typeof obj);
    }

    types.delete("string");

    if (types.size) {
      if (types.delete("object")) {
        types.add("unknown");
      }
      let invalid_type = [...types.values()].sort().join(" | ");
      invalidTypes.push(["chars", `Array<${invalid_type}>`]);
    }
  }

  let formattedString = "";

  for (const [a, b] of invalidTypes) {
    if (formattedString.length != 0) {
      formattedString += "\n  ";
    }
    formattedString += `${a}: ${b}`;
  }

  switch (invalidTypes.length) {
    case 0:
      break;
    case 1:
      throw `Object has invalid types for JSON provider: { ${formattedString} }`;
    default:
      throw `B: Object has invalid types for JSON provider: {\n  ${formattedString}\n}`;
  }
}

// Type safety is not free.
//
// In another language, you could get a library like Pydantic to do
// this for you automatically, but TypeScript is full of shit.
export function assertValidJSONProvider(
  obj: unknown,
): asserts obj is JSONGlyphData {
  assertHasJSONProviderKeys(obj);
  assertKeysMatchJSONProvider(obj);
}
