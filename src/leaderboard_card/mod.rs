/// Leaderboard card image generator.
///
/// Produces a 1200x700 PNG leaderboard image using the same Minecraft bitmap
/// font sheet as the level card. Shows up to 10 players per page with rank,
/// avatar, username, level, and total XP.
use std::io::Cursor;

use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};

// ---------------------------------------------------------------------------
// Embedded font sheet (shared with level_card)
// ---------------------------------------------------------------------------

static FONT_PNG: &[u8] = include_bytes!("../font/assets/textures/font/ascii.png");

// ---------------------------------------------------------------------------
// Colour constants
// ---------------------------------------------------------------------------

const BG: Rgba<u8> = Rgba([0x1a, 0x1a, 0x2e, 0xff]);
const PANEL: Rgba<u8> = Rgba([0x1a, 0x1a, 0x2e, 0xff]);
const ROW_EVEN: Rgba<u8> = Rgba([0x1e, 0x1e, 0x34, 0xff]);
const ROW_ODD: Rgba<u8> = Rgba([0x22, 0x22, 0x3a, 0xff]);
const GOLD_ROW: Rgba<u8> = Rgba([0x2a, 0x24, 0x10, 0xff]);
const SILVER_ROW: Rgba<u8> = Rgba([0x20, 0x22, 0x28, 0xff]);
const BRONZE_ROW: Rgba<u8> = Rgba([0x28, 0x1e, 0x14, 0xff]);
const WHITE: Rgba<u8> = Rgba([0xff, 0xff, 0xff, 0xff]);
const CYAN: Rgba<u8> = Rgba([0x00, 0xbf, 0xff, 0xff]);
const MUTED: Rgba<u8> = Rgba([0x88, 0x88, 0xaa, 0xff]);
const GOLD: Rgba<u8> = Rgba([0xff, 0xd7, 0x00, 0xff]);
const SILVER: Rgba<u8> = Rgba([0xc0, 0xc0, 0xc0, 0xff]);
const BRONZE: Rgba<u8> = Rgba([0xcd, 0x7f, 0x32, 0xff]);
const DIVIDER: Rgba<u8> = Rgba([0x30, 0x30, 0x50, 0xff]);
const HEADER_BG: Rgba<u8> = Rgba([0x16, 0x16, 0x28, 0xff]);

// ---------------------------------------------------------------------------
// Image dimensions
// ---------------------------------------------------------------------------

const IMG_W: u32 = 1200;
const IMG_H: u32 = 700;
const MARGIN: u32 = 20;
const HEADER_H: u32 = 70;
const ROW_H: u32 = 56;
const AVATAR_SIZE: u32 = 40;
const CORNER_R: u32 = 12;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A single entry to render on the leaderboard image.
pub struct LeaderboardRow {
    /// Global rank (1-indexed).
    pub rank: u32,
    /// Display name (Minecraft username preferred, Discord fallback).
    pub username: String,
    /// Level number.
    pub level: i64,
    /// Total XP.
    pub total_xp: f64,
    /// Raw avatar PNG/JPEG bytes, or `None` for a placeholder.
    pub avatar_bytes: Option<Vec<u8>>,
}

/// Parameters for rendering a leaderboard page.
pub struct LeaderboardCardParams {
    /// The rows to display (up to 10).
    pub rows: Vec<LeaderboardRow>,
    /// Current page number (1-indexed) for the footer.
    pub page: u32,
    /// Total number of pages for the footer.
    pub total_pages: u32,
}

/// Render a leaderboard card and return the PNG bytes.
pub fn render(params: &LeaderboardCardParams) -> Vec<u8> {
    let font = image::load_from_memory(FONT_PNG)
        .expect("embedded font sheet is valid PNG")
        .to_rgba8();

    let mut img = RgbaImage::from_pixel(IMG_W, IMG_H, BG);

    // == OUTER PANEL ==========================================================
    fill_rounded_rect(
        &mut img,
        MARGIN / 2,
        MARGIN / 2,
        IMG_W - MARGIN,
        IMG_H - MARGIN,
        CORNER_R,
        PANEL,
    );

    // == HEADER ===============================================================
    let header_x = MARGIN;
    let header_y = MARGIN;
    let header_w = IMG_W - MARGIN * 2;

    fill_rounded_rect(
        &mut img, header_x, header_y, header_w, HEADER_H, 10, HEADER_BG,
    );

    // Title: "LEADERBOARD"
    render_text(
        &font,
        &mut img,
        header_x + 20,
        header_y + 12,
        "LEADERBOARD",
        3,
        WHITE,
    );

    // Page info right-aligned in header
    let page_text = format!("PAGE {}/{}", params.page, params.total_pages);
    let page_w = measure_text(&font, &page_text, 2);
    let page_x = (header_x + header_w).saturating_sub(20 + page_w);
    render_text(&font, &mut img, page_x, header_y + 22, &page_text, 2, MUTED);

    // == COLUMN HEADERS =======================================================
    let col_header_y = header_y + HEADER_H + 10;
    render_text(&font, &mut img, header_x + 20, col_header_y, "#", 2, MUTED);
    render_text(
        &font,
        &mut img,
        header_x + 100,
        col_header_y,
        "PLAYER",
        2,
        MUTED,
    );
    render_text(
        &font,
        &mut img,
        header_x + 700,
        col_header_y,
        "LEVEL",
        2,
        MUTED,
    );

    // Right-align "XP" header
    let xp_header = "XP";
    let xp_header_w = measure_text(&font, xp_header, 2);
    let xp_header_x = (header_x + header_w).saturating_sub(20 + xp_header_w);
    render_text(
        &font,
        &mut img,
        xp_header_x,
        col_header_y,
        xp_header,
        2,
        MUTED,
    );

    // Divider below column headers
    fill_rect(&mut img, header_x, col_header_y + 22, header_w, 1, DIVIDER);

    // == ROWS =================================================================
    let rows_start_y = col_header_y + 28;

    for (i, row) in params.rows.iter().enumerate() {
        let row_y = rows_start_y + (i as u32) * ROW_H;

        // Row background (top 3 get special colours)
        let row_bg = match row.rank {
            1 => GOLD_ROW,
            2 => SILVER_ROW,
            3 => BRONZE_ROW,
            _ => {
                if i % 2 == 0 {
                    ROW_EVEN
                } else {
                    ROW_ODD
                }
            }
        };
        fill_rounded_rect(&mut img, header_x, row_y, header_w, ROW_H - 2, 8, row_bg);

        // Rank number
        let rank_color = match row.rank {
            1 => GOLD,
            2 => SILVER,
            3 => BRONZE,
            _ => MUTED,
        };
        let rank_text = format!("#{}", row.rank);
        render_text(
            &font,
            &mut img,
            header_x + 16,
            row_y + 16,
            &rank_text,
            2,
            rank_color,
        );

        // Avatar
        let avatar_x = header_x + 80;
        let avatar_y = row_y + (ROW_H - 2 - AVATAR_SIZE) / 2;
        draw_avatar(&mut img, avatar_x, avatar_y, &row.avatar_bytes);

        // Username
        let name_x = avatar_x + AVATAR_SIZE + 16;
        let name_color = match row.rank {
            1 => GOLD,
            2 => SILVER,
            3 => BRONZE,
            _ => WHITE,
        };
        render_text(
            &font,
            &mut img,
            name_x,
            row_y + 8,
            &row.username,
            2,
            name_color,
        );

        // Small subtitle: rank badge text
        let subtitle = match row.rank {
            1 => "1st Place",
            2 => "2nd Place",
            3 => "3rd Place",
            _ => "",
        };
        if !subtitle.is_empty() {
            render_text(&font, &mut img, name_x, row_y + 30, subtitle, 1, rank_color);
        }

        // Level
        let level_text = format!("{}", row.level);
        render_text(
            &font,
            &mut img,
            header_x + 700,
            row_y + 16,
            &level_text,
            2,
            CYAN,
        );

        // XP right-aligned
        let xp_text = format_xp(row.total_xp);
        let xp_w = measure_text(&font, &xp_text, 2);
        let xp_x = (header_x + header_w).saturating_sub(20 + xp_w);
        render_text(&font, &mut img, xp_x, row_y + 16, &xp_text, 2, WHITE);
    }

    // == EMPTY STATE ==========================================================
    if params.rows.is_empty() {
        let empty_text = "No players to display";
        let text_w = measure_text(&font, empty_text, 3);
        let cx = (IMG_W - text_w) / 2;
        render_text(&font, &mut img, cx, IMG_H / 2 - 12, empty_text, 3, MUTED);
    }

    // == ENCODE PNG ============================================================
    let mut buf: Vec<u8> = Vec::new();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .expect("PNG encoding should not fail");
    buf
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format XP with comma separators (e.g. 12450.0 -> "12,450").
fn format_xp(xp: f64) -> String {
    let whole = xp.round() as i64;
    if whole < 0 {
        return "0".to_string();
    }
    let s = whole.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Draw an avatar (or placeholder) at the given position.
fn draw_avatar(img: &mut RgbaImage, x: u32, y: u32, avatar_bytes: &Option<Vec<u8>>) {
    let radius = 6u32;
    if let Some(bytes) = avatar_bytes {
        if let Ok(dyn_img) = image::load_from_memory(bytes) {
            let avatar = dyn_img.resize_exact(
                AVATAR_SIZE,
                AVATAR_SIZE,
                image::imageops::FilterType::Nearest,
            );
            for ay in 0..AVATAR_SIZE {
                for ax in 0..AVATAR_SIZE {
                    if is_inside_rounded_rect(ax, ay, AVATAR_SIZE, AVATAR_SIZE, radius) {
                        let px = x + ax;
                        let py = y + ay;
                        if px < img.width() && py < img.height() {
                            img.put_pixel(px, py, avatar.get_pixel(ax, ay));
                        }
                    }
                }
            }
            return;
        }
    }
    // Fallback placeholder
    fill_rounded_rect(img, x, y, AVATAR_SIZE, AVATAR_SIZE, radius, MUTED);
}

// ---------------------------------------------------------------------------
// Drawing primitives (same as level_card)
// ---------------------------------------------------------------------------

fn fill_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
    let img_w = img.width();
    let img_h = img.height();
    for dy in 0..h {
        for dx in 0..w {
            let px = x + dx;
            let py = y + dy;
            if px < img_w && py < img_h {
                img.put_pixel(px, py, color);
            }
        }
    }
}

fn is_inside_rounded_rect(px: u32, py: u32, w: u32, h: u32, r: u32) -> bool {
    let in_left = px < r;
    let in_right = px >= w.saturating_sub(r);
    let in_top = py < r;
    let in_bottom = py >= h.saturating_sub(r);

    if (in_left || in_right) && (in_top || in_bottom) {
        let cx = if in_left { r - 1 } else { w - r };
        let cy = if in_top { r - 1 } else { h - r };
        let dx = px as i64 - cx as i64;
        let dy = py as i64 - cy as i64;
        dx * dx + dy * dy <= (r as i64) * (r as i64)
    } else {
        true
    }
}

fn fill_rounded_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, r: u32, color: Rgba<u8>) {
    let img_w = img.width();
    let img_h = img.height();
    for dy in 0..h {
        for dx in 0..w {
            if is_inside_rounded_rect(dx, dy, w, h, r) {
                let px = x + dx;
                let py = y + dy;
                if px < img_w && py < img_h {
                    img.put_pixel(px, py, color);
                }
            }
        }
    }
}

fn measure_glyph_width(font: &RgbaImage, c: u8) -> u32 {
    let grid_col = (c % 16) as u32;
    let grid_row = (c / 16) as u32;
    let src_x = grid_col * 8;
    let src_y = grid_row * 8;

    let mut rightmost: i32 = -1;
    for row in 0..8u32 {
        for col in 0..8u32 {
            let px = font.get_pixel(src_x + col, src_y + row);
            if px[3] > 128 {
                if col as i32 > rightmost {
                    rightmost = col as i32;
                }
            }
        }
    }
    if rightmost < 0 {
        4
    } else {
        (rightmost + 1) as u32
    }
}

fn measure_text(font: &RgbaImage, text: &str, scale: u32) -> u32 {
    let mut width: u32 = 0;
    let mut last_was_glyph = false;

    for ch in text.chars() {
        let c = ch as u32;
        if c < 0x20 || c > 0x7e {
            width += 4 * scale + scale;
            last_was_glyph = false;
            continue;
        }
        let c = c as u8;
        if c == b' ' {
            width += 4 * scale;
            last_was_glyph = false;
            continue;
        }
        let glyph_w = measure_glyph_width(font, c);
        width += glyph_w * scale + scale;
        last_was_glyph = true;
    }

    if last_was_glyph {
        width = width.saturating_sub(scale);
    }
    width
}

fn render_text(
    font: &RgbaImage,
    img: &mut RgbaImage,
    x: u32,
    y: u32,
    text: &str,
    scale: u32,
    color: Rgba<u8>,
) {
    let img_w = img.width();
    let img_h = img.height();
    let mut cursor_x = x;

    for ch in text.chars() {
        let c = ch as u32;
        if c < 0x20 || c > 0x7e {
            cursor_x += 4 * scale + scale;
            continue;
        }
        let c = c as u8;
        if c == b' ' {
            cursor_x += 4 * scale;
            continue;
        }

        let grid_col = (c % 16) as u32;
        let grid_row = (c / 16) as u32;
        let src_x = grid_col * 8;
        let src_y = grid_row * 8;
        let glyph_w = measure_glyph_width(font, c);

        for fy in 0..8u32 {
            for fx in 0..glyph_w {
                let fpx = font.get_pixel(src_x + fx, src_y + fy);
                if fpx[3] > 128 {
                    for by in 0..scale {
                        for bx in 0..scale {
                            let px = cursor_x + fx * scale + bx;
                            let py = y + fy * scale + by;
                            if px < img_w && py < img_h {
                                img.put_pixel(px, py, color);
                            }
                        }
                    }
                }
            }
        }
        cursor_x += glyph_w * scale + scale;
    }
}
