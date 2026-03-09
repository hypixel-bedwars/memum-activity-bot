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
const GOLD: Rgba<u8> = Rgba([0xff, 0xd7, 0x00, 0xff]);
const SILVER: Rgba<u8> = Rgba([0xc0, 0xc0, 0xc0, 0xff]);
const BRONZE: Rgba<u8> = Rgba([0xcd, 0x7f, 0x32, 0xff]);
const DIVIDER: Rgba<u8> = Rgba([0x30, 0x30, 0x50, 0xff]);
const HEADER_BG: Rgba<u8> = Rgba([0x16, 0x16, 0x28, 0xff]);

// ---------------------------------------------------------------------------
// Image dimensions
// ---------------------------------------------------------------------------

const IMG_W: u32 = 1200;
/// Height of the player-rows section (header + column headers + 10 rows).
const BASE_IMG_H: u32 = 700;
const MARGIN: u32 = 20;
const HEADER_H: u32 = 70;
const ROW_H: u32 = 40;
const AVATAR_SIZE: u32 = 40;
const CORNER_R: u32 = 12;

/// Height of the milestone section header row (title + divider).
const MILESTONE_SECTION_HEADER_H: u32 = 48;
/// Height per individual milestone entry row.
const MILESTONE_ROW_H: u32 = 40;
/// Padding below the last milestone row before the image edge.
const MILESTONE_BOTTOM_PAD: u32 = 16;
const MILESTONE_HEADER_GAP: u32 = 30;

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
        "leaderboard_card::render: page={} total_pages={} rows={}",
        params.page,
        params.total_pages,
        params.rows.len(),
    );

    let font = FontRenderer::get();

    let mut img = RgbaImage::from_pixel(IMG_W, BASE_IMG_H, BG);

    // == HEADER ===============================================================
    let header_x = MARGIN;
    let header_y = MARGIN;
    let header_w = IMG_W - MARGIN * 2;

    // == COLUMN HEADERS =======================================================
    let col_header_y = header_y + 10;

    // Column layout
    let rank_column_x = header_x + 20;

    // Center of the entire "rank badge + username" column
    let username_column_center = header_x + 350;

    let level_column_center = header_x + 700;
    let xp_column_center = header_x + header_w - 120;

    // Rank header
    font.render_text(&mut img, rank_column_x, col_header_y, "Rank", 3, MUTED);

    let username_header = "Username";
    let username_header_w = font.measure_text(username_header, 3);

    font.render_text(
        &mut img,
        username_column_center - username_header_w / 2,
        col_header_y,
        username_header,
        3,
        MUTED,
    );

    let level_header = "Level";
    let level_header_w = font.measure_text(level_header, 3);

    font.render_text(
        &mut img,
        level_column_center - level_header_w / 2,
        col_header_y,
        level_header,
        3,
        MUTED,
    );

    let xp_header = "XP";
    let xp_header_w = font.measure_text(xp_header, 3);

    font.render_text(
        &mut img,
        xp_column_center - xp_header_w / 2,
        col_header_y,
        xp_header,
        3,
        MUTED,
    );
    // == ROWS =================================================================
    let rows_start_y = col_header_y + 28;

    for (i, row) in params.rows.iter().enumerate() {
        debug!(
            "leaderboard_card::render: drawing row index={} rank={} username={}",
            i, row.rank, row.username
        );

        let row_y = rows_start_y + (i as u32) * ROW_H;

        // Position number (#1, #2, …)
        let rank_color = if row.rank == 1 {
            GOLD
        } else if row.rank == 2 {
            SILVER
        } else if row.rank == 3 {
            BRONZE
        } else {
            MUTED
        };
        let rank_text = format!("#{}", row.rank);
        font.render_formatted_shadowed(
            &mut img,
            rank_column_x,
            row_y + 16,
            &rank_text,
            5,
            rank_color,
        );

        // Hypixel rank badge + username, starting after the position number
        let text_y = row_y + 14;
        let text_scale = 5u32;

        // Parse the stored rank string into a HypixelRank enum so we can
        // derive colours and label text.
        // The DB stores "SUPERSTAR" directly for MVP++ players (matching the
        // Hypixel monthlyPackageRank value). For all other ranks it stores the
        // newPackageRank value (e.g. "VIP", "MVP_PLUS").
        let raw_rank = row.hypixel_rank.as_deref();
        let (new_pkg, monthly_pkg) = if raw_rank == Some("SUPERSTAR") {
            (None, Some("SUPERSTAR"))
        } else {
            (raw_rank, None)
        };
        let hypixel_rank = HypixelRank::from_api(new_pkg, monthly_pkg);

        // Measure rank badge width (must come after from_api so hypixel_rank is in scope)
        let mut badge_w = 0;

        if hypixel_rank != HypixelRank::None {
            let label = hypixel_rank.display_label();
            badge_w = font.measure_text(label, text_scale) + 6;
        }

        // Measure username width
        let username_w = font.measure_text(&row.username, text_scale);

        // Combined width of the whole badge + username block
        let total_name_w = badge_w + username_w;

        // Center the combined block inside the username column
        let mut cursor_x = username_column_center - total_name_w / 2;

        let name_col = hypixel_rank.name_color();

        debug!(
            "rank debug: username={} raw_rank={:?} parsed_rank={:?}",
            row.username, raw_rank, hypixel_rank
        );

        if hypixel_rank != HypixelRank::None {
            // Decompose the label into coloured segments.
            // e.g. "[MVP+]" → "[MVP" in cyan, "+" in gold, "]" in cyan.
            let label = hypixel_rank.display_label(); // e.g. "[MVP+]"
            let name_col = hypixel_rank.name_color();

            // Determine where the "+" sits inside the label (if any).
            let plus_color = plus_color_to_rgba(row.hypixel_rank_plus_color.as_deref());

            if let Some(plus_pos) = label.find('+') {
                // Segment before the '+' (includes '[' and rank name)
                let before = &label[..plus_pos];
                // Number of '+' characters (VIP+ → 1, MVP++ → 2)
                let plus_count = label[plus_pos..].chars().take_while(|&c| c == '+').count();
                // Segment after the last '+' (usually ']')
                let after_start = plus_pos + plus_count;
                let after = &label[after_start..];

                // Render "[RANK" part
                font.render_text(&mut img, cursor_x, text_y, before, text_scale, name_col);
                cursor_x += font.measure_text(before, text_scale);

                // Render '+' (or '++') in plus_color
                let plus_str = &label[plus_pos..after_start];
                font.render_text(&mut img, cursor_x, text_y, plus_str, text_scale, plus_color);
                cursor_x += font.measure_text(plus_str, text_scale);

                // Render ']' in name color
                if !after.is_empty() {
                    font.render_formatted_shadowed(
                        &mut img, cursor_x, text_y, after, text_scale, name_col,
                    );
                    cursor_x += font.measure_text(after, text_scale);
                }
            } else {
                // No '+' (e.g. "[VIP]") — render entire label in name color
                font.render_formatted_shadowed(
                    &mut img, cursor_x, text_y, label, text_scale, name_col,
                );
                cursor_x += font.measure_text(label, text_scale);
            }

            // Small gap between badge and username
            cursor_x += 6;
        }

        // Username
        font.render_formatted_shadowed(
            &mut img,
            cursor_x,
            text_y,
            &row.username,
            text_scale,
            name_col,
        );

        let level_text = format!("{}", row.level);
        let level_w = font.measure_text(&level_text, text_scale);

        let level_column_center = header_x + 700;
        let level_x = level_column_center - level_w / 2;

        // Level
        font.render_formatted_shadowed(
            &mut img,
            level_x,
            row_y + 14,
            &level_text,
            text_scale,
            CYAN,
        );
        // XP right aligned
        let xp_text = format_xp(row.total_xp);
        let xp_w = font.measure_text(&xp_text, text_scale);

        let xp_column_center = header_x + header_w - 120;
        let xp_x = xp_column_center - xp_w / 2;

        font.render_formatted_shadowed(&mut img, xp_x, row_y + 14, &xp_text, text_scale, WHITE);
    }

    // == EMPTY STATE ==========================================================
    if params.rows.is_empty() {
        debug!("leaderboard_card::render: no rows to render (empty state)");
        let empty_text = "No players to display";
        let text_w = font.measure_text(empty_text, 3);
        let cx = (IMG_W - text_w) / 2;
        font.render_text(&mut img, cx, BASE_IMG_H / 2 - 12, empty_text, 3, MUTED);
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

    // Height: header + one row per milestone + bottom padding.
    // Minimum 200 px so an empty card still looks intentional.
    let content_h = (params.milestones.len() as u32).max(1) * MILESTONE_ROW_H + 40;
    let img_h = content_h.max(200);

    let mut img = RgbaImage::from_pixel(IMG_W, img_h, BG);

    // == PANEL ================================================================
    // fill_rounded_rect(
    //     &mut img,
    //     MARGIN,
    //     MARGIN,
    //     IMG_W - MARGIN * 2,
    //     img_h - MARGIN * 2,
    //     CORNER_R,
    //     PANEL,
    // );

    // Total users right-aligned
    let users_text = format!("{} registered players", params.total_users);
    let users_w = font.measure_text(&users_text, 2);
    let users_x = (IMG_W - MARGIN * 2).saturating_sub(20 + users_w) + MARGIN;
    font.render_text(&mut img, users_x, MARGIN + 10, &users_text, 2, MUTED);

    // Divider below header
    // let divider_y = MARGIN + MILESTONE_SECTION_HEADER_H - 6;
    // fill_rect(&mut img, MARGIN, divider_y, IMG_W - MARGIN * 2, 2, DIVIDER);

    // == MILESTONE ROWS =======================================================
    if params.milestones.is_empty() {
        let msg = "No milestones configured for this server.";
        let msg_w = font.measure_text(msg, 2);
        let cx = (IMG_W - msg_w) / 2;
        font.render_text(
            &mut img,
            cx,
            MARGIN + MILESTONE_SECTION_HEADER_H + 10,
            msg,
            3,
            MUTED,
        );
    } else {
        let first_row_y = MARGIN + 20;
        for (i, milestone) in params.milestones.iter().enumerate() {
            let row_y = first_row_y + (i as u32) * MILESTONE_ROW_H;

            // Alternating row background
            // let row_bg = if i % 2 == 0 { ROW_EVEN } else { ROW_ODD };
            // fill_rect(
            //     &mut img,
            //     MARGIN,
            //     row_y,
            //     IMG_W - MARGIN * 2,
            //     MILESTONE_ROW_H,
            //     row_bg,
            // );

            // Level badge
            let level_text = format!("Level {}", milestone.level);
            font.render_text(&mut img, MARGIN + 20, row_y + 8, &level_text, 3, GOLD);

            // User count
            let count_text = format!(
                "{} player{} reached this milestone",
                milestone.user_count,
                if milestone.user_count == 1 {
                    " has"
                } else {
                    "s have"
                },
            );
            font.render_text(
                &mut img,
                MARGIN + 220,
                row_y + (MILESTONE_ROW_H / 2) - 12,
                &count_text,
                3,
                WHITE,
            );

            // Progress bar (right side)
            // let bar_x = MARGIN + 700;
            // let bar_w = IMG_W - MARGIN * 2 - 700 - 20;
            // let bar_h = 12u32;
            // let bar_y = row_y + (MILESTONE_ROW_H - bar_h) / 2;
            // fill_rounded_rect(&mut img, bar_x, bar_y, bar_w, bar_h, bar_h / 2, BAR_BG);
            // let pct = if params.total_users > 0 {
            //     (milestone.user_count as f64 / params.total_users as f64).clamp(0.0, 1.0)
            // } else {
            //     0.0
            // };
            // let fill_w = ((bar_w as f64) * pct).round() as u32;
            // if fill_w > 0 {
            //     fill_rounded_rect(
            //         &mut img,
            //         bar_x,
            //         bar_y,
            //         fill_w.max(bar_h),
            //         bar_h,
            //         bar_h / 2,
            //         CYAN,
            //     );
            // }
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
