/// Statistics card image generator.
///
/// Produces a 1000x420 PNG using the [`crate::font::renderer::FontRenderer`]
/// shared bitmap font engine and the `image` crate for compositing.
///
/// Visual style matches the level card: same dark palette, rounded panels,
/// bitmap font renderer.
use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use tracing::debug;

use crate::database::queries::GuildStatistics;
use crate::font::renderer::FontRenderer;

// ---------------------------------------------------------------------------
// Colour constants (matching level_card palette)
// ---------------------------------------------------------------------------

const BG: Rgba<u8> = Rgba([0x1a, 0x1a, 0x2e, 0xff]);
const PANEL: Rgba<u8> = Rgba([0x22, 0x22, 0x3a, 0xff]);
const WHITE: Rgba<u8> = Rgba([0xff, 0xff, 0xff, 0xff]);
const CYAN: Rgba<u8> = Rgba([0x00, 0xbf, 0xff, 0xff]);
const MUTED: Rgba<u8> = Rgba([0x88, 0x88, 0xaa, 0xff]);
const GREEN: Rgba<u8> = Rgba([0x44, 0xff, 0x88, 0xff]);
const DIVIDER: Rgba<u8> = Rgba([0x30, 0x30, 0x50, 0xff]);

// Card dimensions
const CARD_W: u32 = 1000;
const CARD_H: u32 = 420;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// All data required to render a statistics card.
pub struct StatisticsCardParams {
    /// Title shown at the top (e.g. `"Server Statistics"` or event name).
    pub title: String,
    /// Optional subtitle (e.g. `"All Time"` or `"Active: 12 participants"`).
    pub subtitle: Option<String>,
    pub stats: GuildStatistics,
}

/// Render the statistics card and return the PNG bytes.
pub fn render(params: &StatisticsCardParams) -> Vec<u8> {
    debug!(
        "statistics_card::render: title={:?}, subtitle={:?}",
        params.title, params.subtitle
    );

    let font = FontRenderer::get();
    let mut img = RgbaImage::from_pixel(CARD_W, CARD_H, BG);

    // == INNER PANEL =========================================================
    fill_rounded_rect(&mut img, 8, 8, CARD_W - 16, CARD_H - 16, 12, PANEL);

    // == TITLE ===============================================================
    font.render_text(&mut img, 28, 28, &params.title, 3, CYAN);

    // == SUBTITLE ============================================================
    let subtitle_y: u32 = 64;
    if let Some(sub) = &params.subtitle {
        font.render_text(&mut img, 28, subtitle_y, sub, 2, MUTED);
    }

    // == DIVIDER =============================================================
    fill_rect(&mut img, 28, 90, CARD_W - 56, 2, DIVIDER);

    // == HEADLINE STATS (2-column, 4 stats) ==================================
    // Row 1: Total Messages | Valid Messages
    // Row 2: Voice Minutes  | Total XP
    // (Row 3 if participants present): Participants

    let hl_base_y: u32 = 108;
    let hl_row_h: u32 = 64;
    let col1_x: u32 = 28;
    let col2_x: u32 = 380;
    let col3_x: u32 = 610; // for participants if present, otherwise unused

    // Helper closure — renders a label+value pair in a mini sub-panel.
    let render_headline =
        |img: &mut RgbaImage, x: u32, y: u32, label: &str, value: &str, value_color: Rgba<u8>| {
            font.render_text(img, x, y, label, 2, MUTED);
            font.render_text(img, x, y + 20, value, 2, value_color);
        };

    render_headline(
        &mut img,
        col1_x,
        hl_base_y,
        "TOTAL MESSAGES",
        &format_stat(params.stats.total_messages),
        WHITE,
    );
    render_headline(
        &mut img,
        col2_x,
        hl_base_y,
        "VALID MESSAGES",
        &format_stat(params.stats.valid_messages),
        GREEN,
    );

    render_headline(
        &mut img,
        col1_x,
        hl_base_y + hl_row_h,
        "VOICE MINUTES",
        &format_stat(params.stats.total_vc_minutes),
        WHITE,
    );
    render_headline(
        &mut img,
        col2_x,
        hl_base_y + hl_row_h,
        "TOTAL XP",
        &format_stat_xp(params.stats.total_xp),
        CYAN,
    );

    let mut next_section_y: u32 = hl_base_y + hl_row_h * 2 + 8;

    if let Some(participants) = params.stats.participants {
        render_headline(
            &mut img,
            col3_x,
            hl_base_y,
            "PARTICIPANTS",
            &participants.to_string(),
            CYAN,
        );
    }

    // == DIVIDER =============================================================
    fill_rect(&mut img, 28, next_section_y, CARD_W - 56, 2, DIVIDER);
    next_section_y += 14;

    // == OTHER STAT CHANGES GRID =============================================
    if !params.stats.other_stat_changes.is_empty() {
        font.render_text(&mut img, col1_x, next_section_y, "STAT BREAKDOWN", 2, CYAN);
        next_section_y += 22;

        let step: u32 = 22;
        let max_rows = 4usize;
        let max_items = max_rows * 2;

        for (i, stat) in params
            .stats
            .other_stat_changes
            .iter()
            .take(max_items)
            .enumerate()
        {
            let col_x = if i < max_rows { col1_x } else { col2_x };
            let row = (i % max_rows) as u32;
            let y = next_section_y + row * step;
            let line = format!("{}: {}", stat.label, format_stat(stat.value));
            font.render_text(&mut img, col_x, y, &line, 2, GREEN);
        }
    } else {
        font.render_text(
            &mut img,
            col1_x,
            next_section_y,
            "No stat changes recorded yet.",
            2,
            MUTED,
        );
    }

    // == ENCODE PNG ===========================================================
    let mut buf: Vec<u8> = Vec::new();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .expect("PNG encoding should not fail");
    debug!(
        "statistics_card::render: finished encoding PNG (bytes={})",
        buf.len()
    );
    buf
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format a stat value with thousand separators and no decimal for whole numbers.
fn format_stat(v: i64) -> String {
    let s = v.to_string();
    let mut result = String::new();

    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }

    result.chars().rev().collect()
}

fn format_stat_xp(v: f64) -> String {
    if v.fract() == 0.0 {
        let n = v as i64;
        // Insert commas
        let s = n.to_string();
        let mut result = String::new();
        for (i, c) in s.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                result.push(',');
            }
            result.push(c);
        }
        result.chars().rev().collect()
    } else {
        format!("{:.1}", v)
    }
}

// ---------------------------------------------------------------------------
// Drawing primitives (copied from level_card for self-containment)
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
