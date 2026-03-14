//! Shared Minecraft bitmap font renderer.
//!
//! Loads the three font sheets from `assets/textures/font/` and the
//! `default.json` descriptor once (via [`FontRenderer::get`]), then exposes
//! fast render and measure helpers used by every card module.
//!
//! # Text colour / formatting codes
//! [`render_formatted`] and [`render_formatted_shadowed`] parse Minecraft
//! `§`-codes inline:
//!
//! | Code | Effect |
//! |------|--------|
//! | `§0`–`§9`, `§a`–`§f` | Set text colour (the 16 standard Minecraft colours) |
//! | `§l` | Enable bold (glyph drawn twice, 1-scaled-pixel right offset) |
//! | `§r` | Reset colour to `default_color` and clear bold |
//!
//! All other text (no `§` prefix) is rendered with the supplied
//! `default_color`.
//!
//! # Shadow support
//! Shadow is **opt-in per call site**.  Call [`render_text_shadowed`] or
//! [`render_formatted_shadowed`] to draw a 1-scaled-pixel down-right
//! drop-shadow at ≈25 % brightness before the main glyph.
//!
//! # Unicode coverage
//! Three bitmap sheets are loaded on first use:
//! - `ascii.png`            – printable ASCII 0x20–0x7E plus some extra symbols
//! - `nonlatin_european.png`– Greek, Cyrillic, Hebrew, IPA, and more
//! - `accented.png`         – Latin Extended A/B (accented European characters)
//!
//! Any character not found in these sheets advances the cursor by a
//! space-width placeholder (4 scaled pixels).
//! 
//! Most of the code was just recreated from a different renderer
//! I will probably switch to HTML soon as Armenium also wants to work on this project 

use std::collections::HashMap;
use std::sync::OnceLock;

use image::{Rgba, RgbaImage};
use serde::Deserialize;
use tracing::debug;

// ---------------------------------------------------------------------------
// Embedded assets
// ---------------------------------------------------------------------------

static ASCII_PNG: &[u8] = include_bytes!("assets/textures/font/ascii.png");
static NONLATIN_PNG: &[u8] = include_bytes!("assets/textures/font/nonlatin_european.png");
static ACCENTED_PNG: &[u8] = include_bytes!("assets/textures/font/accented.png");
static DEFAULT_JSON: &str = include_str!("assets/font/default.json");

// ---------------------------------------------------------------------------
// Minecraft § colour table
// ---------------------------------------------------------------------------

/// (R, G, B) text colours for § codes `0`–`9` then `a`–`f` (index 10–15).
///
/// Source: `chat-formattings.ts` in the TypeScript prototype.
const CHAT_COLORS: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00), // §0  BLACK
    (0x00, 0x00, 0xaa), // §1  DARK_BLUE
    (0x00, 0xaa, 0x00), // §2  DARK_GREEN
    (0x00, 0xaa, 0xaa), // §3  DARK_AQUA
    (0xaa, 0x00, 0x00), // §4  DARK_RED
    (0xaa, 0x00, 0xaa), // §5  DARK_PURPLE
    (0xff, 0xaa, 0x00), // §6  GOLD
    (0xaa, 0xaa, 0xaa), // §7  GRAY
    (0x55, 0x55, 0x55), // §8  DARK_GRAY
    (0x55, 0x55, 0xff), // §9  BLUE
    (0x55, 0xff, 0x55), // §a  GREEN
    (0x55, 0xff, 0xff), // §b  AQUA
    (0xff, 0x55, 0x55), // §c  RED
    (0xff, 0x55, 0xff), // §d  LIGHT_PURPLE
    (0xff, 0xff, 0x55), // §e  YELLOW
    (0xff, 0xff, 0xff), // §f  WHITE
];

/// Derive a shadow colour from a text colour.
///
/// Matches the TypeScript formula: `shadow_channel = text_channel * 63 / 255`
/// (roughly 25 % brightness).
#[inline]
fn make_shadow(color: Rgba<u8>) -> Rgba<u8> {
    Rgba([
        (color[0] as u32 * 63 / 255) as u8,
        (color[1] as u32 * 63 / 255) as u8,
        (color[2] as u32 * 63 / 255) as u8,
        0xff,
    ])
}

// ---------------------------------------------------------------------------
// JSON descriptor types  (for serde parsing of default.json)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct FontJson {
    providers: Vec<FontProvider>,
}

#[derive(Deserialize)]
struct FontProvider {
    #[serde(rename = "type")]
    provider_type: String,
    file: String,
    #[serde(default = "default_height")]
    height: u32,
    ascent: u32,
    #[serde(default)]
    chars: Vec<String>,
}

fn default_height() -> u32 {
    8
}

// ---------------------------------------------------------------------------
// Internal data structures
// ---------------------------------------------------------------------------

struct GlyphSheet {
    image: RgbaImage,
    /// Width of each glyph cell in the sheet image (pixels).
    cell_w: u32,
    /// Height of each glyph cell in the sheet image (pixels).
    cell_h: u32,
    /// Baseline ascent in native sheet units.
    /// Used for vertical alignment: `y_offset = (7 - ascent) * scale`.
    ascent: u32,
}

struct GlyphLoc {
    sheet_idx: usize,
    row: u32,
    col: u32,
}

// ---------------------------------------------------------------------------
// Public renderer
// ---------------------------------------------------------------------------

/// Shared Minecraft bitmap font renderer.
///
/// Construct once with [`FontRenderer::get`]; all methods take `&self` and
/// are safe to call concurrently.
pub struct FontRenderer {
    sheets: Vec<GlyphSheet>,
    /// Maps every known Unicode character to its location in one of the sheets.
    char_map: HashMap<char, GlyphLoc>,
}

impl FontRenderer {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Return the process-wide `FontRenderer`, initialising it on first call.
    ///
    /// All font assets are loaded and parsed exactly once; subsequent calls
    /// return a reference to the same instance instantly.
    pub fn get() -> &'static FontRenderer {
        static INSTANCE: OnceLock<FontRenderer> = OnceLock::new();
        INSTANCE.get_or_init(FontRenderer::build)
    }

    fn build() -> Self {
        let ascii_img = image::load_from_memory(ASCII_PNG)
            .expect("embedded ascii.png is valid PNG")
            .to_rgba8();
        let nonlatin_img = image::load_from_memory(NONLATIN_PNG)
            .expect("embedded nonlatin_european.png is valid PNG")
            .to_rgba8();
        let accented_img = image::load_from_memory(ACCENTED_PNG)
            .expect("embedded accented.png is valid PNG")
            .to_rgba8();

        let font_json: FontJson =
            serde_json::from_str(DEFAULT_JSON).expect("embedded default.json is valid JSON");

        let mut sheets: Vec<GlyphSheet> = Vec::new();
        let mut char_map: HashMap<char, GlyphLoc> = HashMap::new();

        for provider in &font_json.providers {
            if provider.provider_type != "bitmap" || provider.chars.is_empty() {
                continue;
            }

            // Match the file field to one of our embedded images.
            let image = if provider.file.ends_with("ascii.png") {
                ascii_img.clone()
            } else if provider.file.ends_with("nonlatin_european.png") {
                nonlatin_img.clone()
            } else if provider.file.ends_with("accented.png") {
                accented_img.clone()
            } else {
                debug!(
                    "font::renderer: skipping unknown provider file '{}'",
                    provider.file
                );
                continue;
            };

            // Derive cell dimensions from the image size and the chars grid.
            let rows = provider.chars.len() as u32;
            let cols = provider.chars[0].chars().count().max(1) as u32;
            let cell_w = image.width() / cols;
            let cell_h = image.height() / rows;

            let sheet_idx = sheets.len();
            sheets.push(GlyphSheet {
                image,
                cell_w,
                cell_h,
                ascent: provider.ascent,
            });

            // Populate the char → glyph-location map.
            // First matching provider wins (Minecraft priority order).
            for (row_idx, row_str) in provider.chars.iter().enumerate() {
                for (col_idx, ch) in row_str.chars().enumerate() {
                    // Skip empty/placeholder slots.
                    if ch == '\0' || ch == ' ' {
                        continue;
                    }
                    char_map.entry(ch).or_insert(GlyphLoc {
                        sheet_idx,
                        row: row_idx as u32,
                        col: col_idx as u32,
                    });
                }
            }
        }

        debug!(
            "font::renderer: initialised — {} sheets, {} mapped characters",
            sheets.len(),
            char_map.len()
        );

        FontRenderer { sheets, char_map }
    }

    // -----------------------------------------------------------------------
    // Low-level glyph helpers
    // -----------------------------------------------------------------------

    /// Scan a glyph cell and return the index of its rightmost opaque column
    /// plus one (i.e. the natural advance width at scale 1).
    ///
    /// Falls back to `4` for empty cells, matching Minecraft behaviour.
    fn glyph_width(&self, sheet: &GlyphSheet, row: u32, col: u32) -> u32 {
        let src_x = col * sheet.cell_w;
        let src_y = row * sheet.cell_h;
        let mut rightmost: i32 = -1;

        for fy in 0..sheet.cell_h {
            for fx in 0..sheet.cell_w {
                if sheet.image.get_pixel(src_x + fx, src_y + fy)[3] > 0 {
                    if fx as i32 > rightmost {
                        rightmost = fx as i32;
                    }
                }
            }
        }

        if rightmost < 0 {
            4
        } else {
            rightmost as u32 + 1
        }
    }

    /// Blit one glyph onto `img` at `(px, py)` using `scale` as a pixel
    /// multiplier.
    ///
    /// `y_baseline_off` is an already-scaled vertical adjustment derived from
    /// each sheet's `ascent` value so that characters from different sheets
    /// share a common baseline.
    fn blit_glyph(
        &self,
        sheet: &GlyphSheet,
        row: u32,
        col: u32,
        img: &mut RgbaImage,
        px: u32,
        py: u32,
        y_baseline_off: i32,
        scale: u32,
        color: Rgba<u8>,
    ) {
        let src_x = col * sheet.cell_w;
        let src_y = row * sheet.cell_h;
        let glyph_w = self.glyph_width(sheet, row, col);
        let img_w = img.width();
        let img_h = img.height();

        for fy in 0..sheet.cell_h {
            for fx in 0..glyph_w {
                if sheet.image.get_pixel(src_x + fx, src_y + fy)[3] > 0 {
                    // Apply baseline offset and scale.
                    let base_py = py as i32 + fy as i32 * scale as i32 + y_baseline_off;
                    if base_py < 0 {
                        continue;
                    }
                    // Paint a (scale × scale) block for this source pixel.
                    for by in 0..scale {
                        for bx in 0..scale {
                            let draw_x = px + fx * scale + bx;
                            let draw_y = base_py as u32 + by;
                            if draw_x < img_w && draw_y < img_h {
                                img.put_pixel(draw_x, draw_y, color);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Compute the scaled Y-baseline offset for a sheet.
    ///
    /// Matches the TypeScript formula `y + (7 - ascent)` applied at
    /// `context.scale(scale, scale)`.  For `ascii.png` (ascent=7) this is
    /// always 0; for `accented.png` (ascent=10) it is `-3 * scale`.
    #[inline]
    fn y_offset(&self, sheet: &GlyphSheet, scale: u32) -> i32 {
        (7i32 - sheet.ascent as i32) * scale as i32
    }

    // -----------------------------------------------------------------------
    // Public measurement API
    // -----------------------------------------------------------------------

    /// Measure the pixel width of **plain** text at `scale`.
    ///
    /// `§` characters are treated as literal characters, not as formatting
    /// codes.  For §-formatted text use [`measure_formatted`].
    pub fn measure_text(&self, text: &str, scale: u32) -> u32 {
        let mut width: u32 = 0;
        let mut last_was_glyph = false;

        for ch in text.chars() {
            if ch == ' ' {
                width += 4 * scale;
                last_was_glyph = false;
            } else if let Some(loc) = self.char_map.get(&ch) {
                let sheet = &self.sheets[loc.sheet_idx];
                let gw = self.glyph_width(sheet, loc.row, loc.col);
                width += (gw + 1) * scale; // glyph width + 1-pixel gap
                last_was_glyph = true;
            } else {
                // Unmapped character → fixed-width placeholder
                width += (4 + 1) * scale;
                last_was_glyph = false;
            }
        }

        // Strip the trailing inter-glyph gap from the last character.
        if last_was_glyph {
            width = width.saturating_sub(scale);
        }
        width
    }

    /// Measure the pixel width of **§-formatted** text at `scale`.
    ///
    /// `§`-colour codes have no width.  `§l` (bold) widens each subsequent
    /// glyph by one additional scaled pixel; `§r` clears bold.
    pub fn measure_formatted(&self, text: &str, scale: u32) -> u32 {
        let mut width: u32 = 0;
        let mut bold = false;
        let mut last_was_glyph = false;
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '§' {
                if let Some(&code) = chars.peek() {
                    chars.next();
                    match code.to_ascii_lowercase() {
                        'l' => bold = true,
                        'r' => bold = false,
                        _ => {} // colour codes: no width effect
                    }
                }
                last_was_glyph = false;
                continue;
            }

            if ch == ' ' {
                width += 4 * scale;
                last_was_glyph = false;
            } else if let Some(loc) = self.char_map.get(&ch) {
                let sheet = &self.sheets[loc.sheet_idx];
                let gw = self.glyph_width(sheet, loc.row, loc.col);
                let bold_extra = if bold { scale } else { 0 };
                width += (gw + 1) * scale + bold_extra;
                last_was_glyph = true;
            } else {
                width += (4 + 1) * scale;
                last_was_glyph = false;
            }
        }

        if last_was_glyph {
            width = width.saturating_sub(scale);
        }
        width
    }

    // -----------------------------------------------------------------------
    // Public render API
    // -----------------------------------------------------------------------

    /// Render **plain** text at `(x, y)` with `color` and `scale`.
    ///
    /// No `§`-code parsing, no shadow.  This is a drop-in replacement for the
    /// old per-card `render_text` helper and is fully backward-compatible.
    pub fn render_text(
        &self,
        img: &mut RgbaImage,
        x: u32,
        y: u32,
        text: &str,
        scale: u32,
        color: Rgba<u8>,
    ) {
        debug!(
            "font::renderer::render_text x={x} y={y} len={} scale={scale}",
            text.len()
        );
        let mut cx = x;

        for ch in text.chars() {
            if ch == ' ' {
                cx += 4 * scale;
                continue;
            }
            if let Some(loc) = self.char_map.get(&ch) {
                let sheet = &self.sheets[loc.sheet_idx];
                let gw = self.glyph_width(sheet, loc.row, loc.col);
                let yo = self.y_offset(sheet, scale);
                self.blit_glyph(sheet, loc.row, loc.col, img, cx, y, yo, scale, color);
                cx += (gw + 1) * scale;
            } else {
                cx += (4 + 1) * scale; // unknown → placeholder advance
            }
        }
    }

    /// Render **plain** text with a drop-shadow (opt-in).
    ///
    /// Draws the shadow first at `(x + scale, y + scale)` in a darkened colour
    /// (~25 % brightness), then draws the text at `(x, y)` in `color`.
    pub fn render_text_shadowed(
        &self,
        img: &mut RgbaImage,
        x: u32,
        y: u32,
        text: &str,
        scale: u32,
        color: Rgba<u8>,
    ) {
        let shadow = make_shadow(color);
        // Shadow offset: 1 "glyph unit" → `scale` screen pixels
        let so = scale;
        self.render_text(img, x + so, y + so, text, scale, shadow);
        self.render_text(img, x, y, text, scale, color);
    }

    /// Render **§-formatted** text at `(x, y)`.
    ///
    /// `default_color` is both the initial colour and the colour restored by
    /// `§r`.  No shadow is drawn; see [`render_formatted_shadowed`] for that.
    pub fn render_formatted(
        &self,
        img: &mut RgbaImage,
        x: u32,
        y: u32,
        text: &str,
        scale: u32,
        default_color: Rgba<u8>,
    ) {
        self.render_formatted_inner(img, x, y, text, scale, default_color, false);
    }

    /// Render **§-formatted** text with a drop-shadow (opt-in).
    ///
    /// Shadow and bold interact correctly: bold shadow is also doubled.
    pub fn render_formatted_shadowed(
        &self,
        img: &mut RgbaImage,
        x: u32,
        y: u32,
        text: &str,
        scale: u32,
        default_color: Rgba<u8>,
    ) {
        self.render_formatted_inner(img, x, y, text, scale, default_color, true);
    }

    // -----------------------------------------------------------------------
    // Shared inner render implementation
    // -----------------------------------------------------------------------

    fn render_formatted_inner(
        &self,
        img: &mut RgbaImage,
        x: u32,
        y: u32,
        text: &str,
        scale: u32,
        default_color: Rgba<u8>,
        with_shadow: bool,
    ) {
        debug!(
            "font::renderer::render_formatted x={x} y={y} len={} scale={scale} shadow={with_shadow}",
            text.len()
        );

        // Shadow offset: 1 glyph unit → `scale` screen pixels.
        let so = scale;

        let mut cx = x;
        let mut color = default_color;
        let mut bold = false;
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            // ------------------------------------------------------------------
            // § formatting code
            // ------------------------------------------------------------------
            if ch == '§' {
                if let Some(&code) = chars.peek() {
                    chars.next();
                    let lc = code.to_ascii_lowercase();
                    match lc {
                        '0'..='9' => {
                            let idx = (lc as u8 - b'0') as usize;
                            let (r, g, b) = CHAT_COLORS[idx];
                            color = Rgba([r, g, b, 0xff]);
                        }
                        'a'..='f' => {
                            let idx = 10 + (lc as u8 - b'a') as usize;
                            let (r, g, b) = CHAT_COLORS[idx];
                            color = Rgba([r, g, b, 0xff]);
                        }
                        'l' => bold = true,
                        'r' => {
                            color = default_color;
                            bold = false;
                        }
                        _ => {}
                    }
                }
                continue;
            }

            // ------------------------------------------------------------------
            // Space
            // ------------------------------------------------------------------
            if ch == ' ' {
                cx += 4 * scale;
                continue;
            }

            // ------------------------------------------------------------------
            // Printable glyph
            // ------------------------------------------------------------------
            if let Some(loc) = self.char_map.get(&ch) {
                let sheet = &self.sheets[loc.sheet_idx];
                let gw = self.glyph_width(sheet, loc.row, loc.col);
                let yo = self.y_offset(sheet, scale);
                let bold_off = if bold { scale } else { 0 };

                if with_shadow {
                    let shad = make_shadow(color);
                    // Shadow at (cx + so, y + so)
                    self.blit_glyph(
                        sheet,
                        loc.row,
                        loc.col,
                        img,
                        cx + so,
                        y + so,
                        yo,
                        scale,
                        shad,
                    );
                    if bold {
                        self.blit_glyph(
                            sheet,
                            loc.row,
                            loc.col,
                            img,
                            cx + so + bold_off,
                            y + so,
                            yo,
                            scale,
                            shad,
                        );
                    }
                }

                // Main glyph at (cx, y)
                self.blit_glyph(sheet, loc.row, loc.col, img, cx, y, yo, scale, color);
                if bold {
                    self.blit_glyph(
                        sheet,
                        loc.row,
                        loc.col,
                        img,
                        cx + bold_off,
                        y,
                        yo,
                        scale,
                        color,
                    );
                }

                cx += (gw + 1) * scale + bold_off;
            } else {
                cx += (4 + 1) * scale; // unknown character → placeholder
            }
        }
    }
}
