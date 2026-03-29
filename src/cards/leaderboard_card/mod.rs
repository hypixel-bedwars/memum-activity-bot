/// Leaderboard card image generator.
///
/// Produces a 1200x700 PNG leaderboard image using the shared
/// [`crate::font::renderer::FontRenderer`] bitmap font engine.
/// Shows up to 10 players per page with rank, username, level, and total XP.
use std::io::Cursor;

use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};
use tracing::debug;

use crate::font::renderer::FontRenderer;
use crate::hypixel::models::{HypixelRank, plus_color_to_rgba};

// ---------------------------------------------------------------------------
// Colour constants
// ---------------------------------------------------------------------------

const BG: Rgba<u8> = Rgba([0, 0, 0, 0]);
const PANEL: Rgba<u8> = Rgba([0x1a, 0x1a, 0x2e, 0xff]);
const ROW_EVEN: Rgba<u8> = Rgba([0x1e, 0x1e, 0x34, 0xff]);
const ROW_ODD: Rgba<u8> = Rgba([0x22, 0x22, 0x3a, 0xff]);
const GOLD_ROW: Rgba<u8> = Rgba([0x2a, 0x24, 0x10, 0xff]);
const SILVER_ROW: Rgba<u8> = Rgba([0x20, 0x22, 0x28, 0xff]);
const BRONZE_ROW: Rgba<u8> = Rgba([0x28, 0x1e, 0x14, 0xff]);
const WHITE: Rgba<u8> = Rgba([0xff, 0xff, 0xff, 0xff]);
const CYAN: Rgba<u8> = Rgba([0x00, 0xbf, 0xff, 0xff]);
const MUTED: Rgba<u8> = Rgba([0x88, 0x88, 0xaa, 0xff]);
const GOLD: Rgba<u8> = Rgba([0xff, 0xaa, 0x00, 0xff]);
const SILVER: Rgba<u8> = Rgba([0xc0, 0xc0, 0xc0, 0xff]);
const BRONZE: Rgba<u8> = Rgba([0xcd, 0x7f, 0x32, 0xff]);
const DIVIDER: Rgba<u8> = Rgba([0x30, 0x30, 0x50, 0xff]);
const HEADER_BG: Rgba<u8> = Rgba([0x16, 0x16, 0x28, 0xff]);
const RANK_GREEN: Rgba<u8> = Rgba([0x55, 0xff, 0x55, 0xff]);
const LIGHT_BLUE: Rgba<u8> = Rgba([0x55, 0xff, 0xff, 0xff]);

// ---------------------------------------------------------------------------
// Image dimensions
// ---------------------------------------------------------------------------

const IMG_W: u32 = 1200;
/// Height of the player-rows section (header + column headers + 10 rows).
const BASE_IMG_H: u32 = 700;
const MARGIN: u32 = 20;
const HEADER_H: u32 = 70;
const ROW_H: u32 = 60;
const AVATAR_SIZE: u32 = 40;
const CORNER_R: u32 = 12;

/// Height of the milestone section header row (title + divider).
const MILESTONE_SECTION_HEADER_H: u32 = 48;
/// Height per individual milestone entry row.
const MILESTONE_ROW_H: u32 = 50;
/// Padding below the last milestone row before the image edge.
const MILESTONE_BOTTOM_PAD: u32 = 40;
const MILESTONE_HEADER_GAP: u32 = 80;

/// Extra vertical space reserved at the top when a card title is present.
const TITLE_H: u32 = 50;

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
    pub level: i32,
    /// Total XP.
    pub total_xp: f64,
    /// Raw avatar PNG/JPEG bytes, or `None` for a placeholder.
    pub avatar_bytes: Option<Vec<u8>>,
    /// The player's Hypixel rank package string (e.g. `"VIP"`, `"MVP_PLUS"`, `"SUPERSTAR"`).
    pub hypixel_rank: Option<String>,
    /// The colour of the `+` symbol in the player's rank badge (e.g. `"GOLD"`, `"RED"`).
    pub hypixel_rank_plus_color: Option<String>,
}

/// A single milestone entry with its reach count.
pub struct MilestoneEntry {
    /// The level threshold for this milestone.
    pub level: i32,
    /// Number of users in the guild who have reached this level or higher.
    pub user_count: i64,
}

/// Parameters for rendering a leaderboard page.
pub struct LeaderboardCardParams {
    /// The rows to display (up to 10).
    pub rows: Vec<LeaderboardRow>,
    /// Current page number (1-indexed) for the footer.
    pub page: u32,
    /// Total number of pages for the footer.
    pub total_pages: u32,
    /// Optional title drawn above the column headers (e.g. event name).
    /// When `Some`, the image height is increased by `TITLE_H` to make room.
    pub title: Option<String>,
    /// Whether to show the Level column. Set to `false` for event leaderboards.
    pub show_level: bool,
    /// Override the empty-state message (shown when `rows` is empty).
    /// Falls back to `"No players to display"` when `None`.
    pub custom_empty_message: Option<String>,
    /// Optional display limit for dynamic header text (e.g., "TOP 20").
    pub display_limit: Option<i64>,
}

/// Parameters for rendering a standalone milestone card.
pub struct MilestoneCardParams {
    /// Milestones to display, ordered by level ascending.
    pub milestones: Vec<MilestoneEntry>,
    /// Total number of registered users in the guild (for context).
    pub total_users: i64,
}

/// Render a leaderboard card and return the PNG bytes.
pub fn render(params: &LeaderboardCardParams) -> Vec<u8> {
    debug!(
        "leaderboard_card::render: page={} total_pages={} rows={} title={:?} show_level={}",
        params.page,
        params.total_pages,
        params.rows.len(),
        params.title,
        params.show_level,
    );

    let font = FontRenderer::get();

    // If a title is provided, the image is taller to accommodate it.
    let img_h = if params.title.is_some() {
        BASE_IMG_H + TITLE_H
    } else {
        BASE_IMG_H
    };
    // Vertical offset applied to every element below the title row.
    let y_offset = if params.title.is_some() { TITLE_H } else { 0 };

    let mut img = RgbaImage::from_pixel(IMG_W, img_h, BG);

    // == TITLE (optional) =====================================================
    if let Some(title) = &params.title {
        let title_scale = 5u32;
        let title_w = font.measure_text(title, title_scale);
        let title_x = (IMG_W - title_w) / 2;
        let title_y = MARGIN;
        font.render_formatted_shadowed(&mut img, title_x, title_y, title, title_scale, WHITE);
    }

    // == HEADER ===============================================================
    let header_x = MARGIN;
    let header_y = MARGIN + y_offset;

    // == COLUMN LAYOUT ========================================================
    // When show_level is false (event leaderboard) the Level column is hidden
    // and the XP column stays in its original position.
    let rank_column_x = header_x + 20;

    // == COLUMN HEADERS =======================================================
    let col_header_y = header_y + 10;

    // Dynamic header: "TOP {count}" on page 1 only, nothing on other pages
    if params.page == 1 {
        let text = if let Some(limit) = params.display_limit {
            format!("TOP {}", limit)
        } else {
            "Top 10".to_string()
        };

        // Bold rendering (multiple passes for thickness)
        font.render_formatted(&mut img, rank_column_x, header_y, &text, 5, GOLD);
        font.render_formatted(&mut img, rank_column_x + 1, header_y, &text, 5, GOLD);
        font.render_formatted(&mut img, rank_column_x - 1, header_y, &text, 5, GOLD);
        font.render_formatted(&mut img, rank_column_x, header_y + 1, &text, 5, GOLD);
    }
    // Pages 2+ show no header at all

    // == ROWS =================================================================
    let rows_start_y = col_header_y + 32;

    for (i, row) in params.rows.iter().enumerate() {
        debug!(
            "leaderboard_card::render: drawing row index={} rank={} username={}",
            i, row.rank, row.username
        );

        let row_y = rows_start_y + (i as u32) * ROW_H;

        let raw_rank = row.hypixel_rank.as_deref();
        let (new_pkg, monthly_pkg) = if raw_rank == Some("SUPERSTAR") {
            (None, Some("SUPERSTAR"))
        } else {
            (raw_rank, None)
        };
        let hypixel_rank = HypixelRank::from_api(new_pkg, monthly_pkg);

        let mut cursor_x = rank_column_x;
        let y = row_y + 14;
        let scale = 5;

        // #rank

        let rank_color = if row.rank == 1 {
            GOLD
        } else if row.rank == 2 {
            SILVER
        } else if row.rank == 3 {
            BRONZE
        } else {
            RANK_GREEN
        };

        let rank_str = format!("#{}", row.rank);
        font.render_formatted_shadowed(&mut img, cursor_x, y, &rank_str, scale, rank_color);
        cursor_x += font.measure_text(&rank_str, scale);

        // space
        cursor_x += font.measure_text(" ", scale);

        let name_col = hypixel_rank.name_color();

        // hypixel rank label
        if hypixel_rank != HypixelRank::None {
            let label = hypixel_rank.display_label();
            let name_col = hypixel_rank.name_color();
            let plus_color = plus_color_to_rgba(row.hypixel_rank_plus_color.as_deref());

            if let Some(plus_pos) = label.find('+') {
                let before = &label[..plus_pos];

                let plus_count = label[plus_pos..].chars().take_while(|&c| c == '+').count();
                let plus_end = plus_pos + plus_count;

                let plus_part = &label[plus_pos..plus_end];
                let after = &label[plus_end..];

                // text before +
                font.render_formatted_shadowed(&mut img, cursor_x, y, before, scale, name_col);
                cursor_x += font.measure_text(before, scale);

                // +++
                font.render_text(&mut img, cursor_x + 5, y, plus_part, scale, plus_color);
                cursor_x += font.measure_text(plus_part, scale);

                // text after +
                if !after.is_empty() {
                    font.render_formatted_shadowed(&mut img, cursor_x, y, after, scale, name_col);
                    cursor_x += font.measure_text(after, scale);
                }
            } else {
                font.render_formatted_shadowed(&mut img, cursor_x, y, label, scale, name_col);
                cursor_x += font.measure_text(label, scale);
            }

            cursor_x += font.measure_text(" ", scale);
        }

        // username
        font.render_formatted_shadowed(&mut img, cursor_x, y, &row.username, scale, name_col);
        cursor_x += font.measure_text(&row.username, scale);

        // dash
        font.render_formatted(&mut img, cursor_x, y, " - ", scale, MUTED);
        cursor_x += font.measure_text(" - ", scale);

        if params.show_level {
            // level
            let level_str = format!("Level {}", row.level);
            font.render_formatted_shadowed(&mut img, cursor_x, y, &level_str, scale, LIGHT_BLUE);
            cursor_x += font.measure_text(&level_str, scale);

            // xp
            let xp_str = format!(" ({})", format_xp(row.total_xp));
            font.render_formatted_shadowed(&mut img, cursor_x, y, &xp_str, scale, MUTED);
        } else {
            // event mode: show only XP, no level
            let xp_str = format!("{}xp", format_xp(row.total_xp));
            font.render_formatted_shadowed(&mut img, cursor_x, y, &xp_str, scale, LIGHT_BLUE);
        }
    }

    // == EMPTY STATE ==========================================================
    if params.rows.is_empty() {
        debug!("leaderboard_card::render: no rows to render (empty state)");
        let empty_text = params
            .custom_empty_message
            .as_deref()
            .unwrap_or("No players to display");
        let text_w = font.measure_text(empty_text, 3);
        let cx = (IMG_W - text_w) / 2;
        font.render_text(&mut img, cx, img_h / 2 - 12, empty_text, 3, MUTED);
    }

    // == ENCODE PNG ============================================================
    let mut buf: Vec<u8> = Vec::new();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .expect("PNG encoding should not fail");

    debug!(
        "leaderboard_card::render: finished encoding PNG (bytes={})",
        buf.len()
    );
    buf
}

/// Render a standalone milestone card and return the PNG bytes.
///
/// The card is 1200px wide and tall enough to fit all milestone rows.
/// A minimum height is enforced so the card never looks empty.
pub fn render_milestone_card(params: &MilestoneCardParams) -> Vec<u8> {
    debug!(
        "leaderboard_card::render_milestone_card: milestones={} total_users={}",
        params.milestones.len(),
        params.total_users,
    );

    let font = FontRenderer::get();
    let scale = 5;

    // Calculate dynamic height: Header + (Rows * RowH) + Bottom Padding
    let rows_count = (params.milestones.len() as u32).max(1);
    let img_h = MILESTONE_HEADER_GAP + (rows_count * MILESTONE_ROW_H) + MILESTONE_BOTTOM_PAD;
    let img_h = img_h.max(300);

    let mut img = RgbaImage::from_pixel(IMG_W, img_h, BG);

    let users_text = format!("{} registered players", params.total_users);
    let users_w = font.measure_text(&users_text, scale);
    let users_x = (IMG_W - MARGIN * 2).saturating_sub(20 + users_w) + MARGIN;
    font.render_text(
        &mut img,
        users_x,
        MARGIN + 10,
        &users_text,
        scale - 1,
        MUTED,
    );

    // == MILESTONE ROWS =======================================================
    if params.milestones.is_empty() {
        let msg = "No milestones configured.";
        let msg_w = font.measure_text(msg, scale);
        let cx = (IMG_W - msg_w) / 2;
        font.render_text(&mut img, cx, MILESTONE_HEADER_GAP + 20, msg, scale, MUTED);
    } else {
        for (i, milestone) in params.milestones.iter().enumerate() {
            let row_y = MILESTONE_HEADER_GAP + (i as u32) * MILESTONE_ROW_H;

            let level_text = format!("Level {}", milestone.level);
            font.render_text(
                &mut img,
                MARGIN + 20,
                row_y + (MILESTONE_ROW_H / 2) - 20,
                &level_text,
                scale - 1,
                GOLD,
            );

            // User count (Centered in row)
            let count_text = format!(
                "{} player{} reached this",
                milestone.user_count,
                if milestone.user_count == 1 {
                    " has"
                } else {
                    "s have"
                },
            );

            font.render_text(
                &mut img,
                MARGIN + 270, // Adjusted X offset for Level text width
                row_y + (MILESTONE_ROW_H / 2) - 20,
                &count_text,
                scale - 1,
                WHITE,
            );
        }
    }

    // == ENCODE PNG ===========================================================
    let mut buf: Vec<u8> = Vec::new();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .expect("PNG encoding should not fail");

    debug!(
        "leaderboard_card::render_milestone_card: finished encoding PNG (bytes={})",
        buf.len()
    );
    buf
}

// ---------------------------------------------------------------------------
// Event milestone card
// ---------------------------------------------------------------------------

/// A single event milestone entry with its reach count.
pub struct EventMilestoneEntry {
    /// The XP threshold for this milestone.
    pub xp_threshold: f64,
    /// Number of participants whose total event XP >= xp_threshold.
    pub user_count: i64,
}

/// Parameters for rendering a standalone event milestone card.
pub struct EventMilestoneCardParams {
    /// Milestones to display, ordered by xp_threshold ascending.
    pub milestones: Vec<EventMilestoneEntry>,
    /// Total number of event participants (for context line).
    pub total_participants: i64,
    /// Event name shown as the card title.
    pub event_name: String,
}

/// Render a standalone event milestone card and return the PNG bytes.
///
/// Mirrors `render_milestone_card` but uses `"X,XXX XP"` labels instead of
/// `"Level N"` and shows the event name as a title row.
pub fn render_event_milestone_card(params: &EventMilestoneCardParams) -> Vec<u8> {
    debug!(
        "leaderboard_card::render_event_milestone_card: event={} milestones={} total_participants={}",
        params.event_name,
        params.milestones.len(),
        params.total_participants,
    );

    let font = FontRenderer::get();
    let scale = 5;

    // Calculate dynamic height
    // Header space + (number of rows * row height) + bottom padding
    let rows_count = (params.milestones.len() as u32).max(1);
    let img_h = MILESTONE_HEADER_GAP + (rows_count * MILESTONE_ROW_H) + MILESTONE_BOTTOM_PAD;
    let img_h = img_h.max(300); // Minimum height to look good

    let mut img = RgbaImage::from_pixel(IMG_W, img_h, BG);

    font.render_text(
        &mut img,
        MARGIN + 20,
        MARGIN + 10,
        &params.event_name,
        scale,
        CYAN,
    );

    let users_text = format!("{} participants", params.total_participants);
    let users_w = font.measure_text(&users_text, scale);
    let users_x = (IMG_W - MARGIN * 2).saturating_sub(20 + users_w) + MARGIN;
    font.render_text(
        &mut img,
        users_x - 20,
        MARGIN + 10,
        &users_text,
        scale - 2,
        MUTED,
    );

    if params.milestones.is_empty() {
        let msg = "No milestones configured.";
        let msg_w = font.measure_text(msg, scale);
        let cx = (IMG_W - msg_w) / 2;
        font.render_text(&mut img, cx, MILESTONE_HEADER_GAP + 20, msg, scale, MUTED);
    } else {
        for (i, milestone) in params.milestones.iter().enumerate() {
            let row_y = MILESTONE_HEADER_GAP + (i as u32) * MILESTONE_ROW_H;

            // Optional: Draw a subtle separator line between rows
            if i > 0 {
                fill_rect(
                    &mut img,
                    MARGIN + 20,
                    row_y,
                    IMG_W - (MARGIN * 2) - 40,
                    2,
                    DIVIDER,
                );
            }

            // XP threshold badge (Vertically centered: row_y + half height - half font size)
            let threshold_text = format!("{} XP", format_xp(milestone.xp_threshold));
            font.render_text(
                &mut img,
                MARGIN + 20,
                row_y + (MILESTONE_ROW_H / 2) - 20,
                &threshold_text,
                4,
                GOLD,
            );

            // User count (Vertically centered)
            let count_text = format!(
                "{} player{} reached this",
                milestone.user_count,
                if milestone.user_count == 1 {
                    " has"
                } else {
                    "s have"
                },
            );

            font.render_text(
                &mut img,
                MARGIN + 280, // Pushed right to clear the XP text
                row_y + (MILESTONE_ROW_H / 2) - 20,
                &count_text,
                scale - 1,
                WHITE,
            );
        }
    }

    // Encode PNG
    let mut buf: Vec<u8> = Vec::new();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .expect("PNG encoding should not fail");
    buf
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const BAR_BG: Rgba<u8> = Rgba([0x2a, 0x2a, 0x4a, 0xff]);

/// Format XP with comma separators (e.g. 12450.0 -> "12,450").
fn format_xp(xp: f64) -> String {
    debug!("leaderboard_card::format_xp: xp={}", xp);
    let whole = xp.round() as i64;
    if whole < 0 {
        debug!("leaderboard_card::format_xp: negative xp, returning 0");
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
    let formatted = result.chars().rev().collect();
    debug!("leaderboard_card::format_xp: formatted={}", formatted);
    formatted
}

/// Draw an avatar (or placeholder) at the given position.
fn draw_avatar(img: &mut RgbaImage, x: u32, y: u32, avatar_bytes: &Option<Vec<u8>>) {
    let radius = 6u32;
    if let Some(bytes) = avatar_bytes {
        debug!(
            "leaderboard_card::draw_avatar: avatar bytes len={}",
            bytes.len()
        );
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
            debug!("leaderboard_card::draw_avatar: rendered avatar image");
            return;
        } else {
            debug!(
                "leaderboard_card::draw_avatar: failed to decode avatar bytes, using placeholder"
            );
        }
    }
    // Fallback placeholder
    fill_rounded_rect(img, x, y, AVATAR_SIZE, AVATAR_SIZE, radius, MUTED);
}

// ---------------------------------------------------------------------------
// Drawing primitives
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
