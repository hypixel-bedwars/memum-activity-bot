import Logger, { Severity } from "../../util/logger";
import { createFontSet, initFontSet } from "./font-set";
import { srcFontDir } from "./helpers";
import GlyphProvider from "./models/glyph-provider";
import JSONGlyphData, {
  assertValidJSONProvider,
} from "./models/json-glyph-data";
import Provider from "./models/provider";
import { AnyGlyph } from "./models/renderers";
import { BitmapGlyph } from "./models/renderers/bitmap-glyph";
import { JSONGlyph } from "./models/renderers/json-glyph";
import type { PathLike } from "fs";
import fs from "fs/promises";

if (typeof require.main == "undefined")
  throw "This file cannot be run dynamically!";

const DEFAULT_FONT = "default";
const fontSets: Record<string, Provider<AnyGlyph>> = {};
const baseDir = srcFontDir("assets", "font");

type JsonProvider = Provider<JSONGlyph>;

async function getJSONProviders(fp: PathLike): Promise<unknown[] | undefined> {
  const json = JSON.parse(await fs.readFile(fp, "utf8"));
  if (
    typeof json == "object" &&
    "providers" in json &&
    Array.isArray(json.providers)
  ) {
    return json.providers;
  }
}

async function getGlyphProvider(
  type: string,
): Promise<GlyphProvider | undefined> {
  let mod: unknown;

  try {
    mod = await import(`./glyph-providers/${type}`);
  } catch (e: any) {
    if (e?.code === "ERR_MODULE_NOT_FOUND") {
      Logger.log("Missing glyph provider!", Severity.ERROR);
      return;
    }

    throw e;
  }

  if (
    typeof mod == "object" &&
    mod !== null &&
    "create" in mod &&
    typeof mod.create == "function"
  ) {
    return mod as GlyphProvider;
  }
}

export async function loadFontSets(): Promise<void> {
  await initFontSet();

  const filepaths = await fs.readdir(baseDir);

  for (const fp of filepaths) {
    if (!fp.endsWith(".json")) {
      continue;
    }

    const providers = await getJSONProviders(`${baseDir}/${fp}`);
    const glyphProviders: JsonProvider[] = [];

    if (providers === undefined) {
      Logger.log("No providers found in file: " + fp, Severity.WARNING);
      continue;
    }

    for (const data of providers) {
      try {
        assertValidJSONProvider(data);
      } catch (e) {
        Logger.log(
          "Error parsing JSON provider: " + e + "\nin file: " + fp,
          Severity.ERROR,
        );
        continue;
      }

      let mod = await getGlyphProvider(data.type);
      if (mod === undefined) {
        continue;
      }

      const glyphProvider: Provider<BitmapGlyph> = await mod.create(data);
      glyphProviders.push(glyphProvider);
    }

    fontSets[fp.slice(0, -".json".length)] = createFontSet(glyphProviders);
  }

  if (Object.keys(fontSets).length == 0) {
    Logger.log("No values were inserted into font-manager.fontSets.");
  }
}

export function getFontSet(
  name = DEFAULT_FONT,
): Provider<AnyGlyph> | undefined {
  return fontSets[name];
}
