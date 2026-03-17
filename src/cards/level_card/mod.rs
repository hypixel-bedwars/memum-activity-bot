/// Level card image generator.
///
/// Produces a 1000x350 PNG using the [`crate::font::renderer::FontRenderer`]
/// shared bitmap font engine and the `image` crate for compositing.
use std::io::Cursor;

use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};
use tracing::debug;

use crate::font::renderer::FontRenderer;
use crate::hypixel::models::{HypixelRank, plus_color_to_rgba};

// ---------------------------------------------------------------------------
// Colour constants
// ---------------------------------------------------------------------------

const BG: Rgba<u8> = Rgba([0x1a, 0x1a, 0x2e, 0xff]);
const PANEL: Rgba<u8> = Rgba([0x22, 0x22, 0x3a, 0xff]);
const WHITE: Rgba<u8> = Rgba([0xff, 0xff, 0xff, 0xff]);
const CYAN: Rgba<u8> = Rgba([0x00, 0xbf, 0xff, 0xff]);
const MUTED: Rgba<u8> = Rgba([0x88, 0x88, 0xaa, 0xff]);
const GREEN: Rgba<u8> = Rgba([0x44, 0xff, 0x88, 0xff]);
const GOLD: Rgba<u8> = Rgba([0xff, 0xd7, 0x00, 0xff]);
const BAR_BG: Rgba<u8> = Rgba([0x2a, 0x2a, 0x4a, 0xff]);
const DIVIDER: Rgba<u8> = Rgba([0x30, 0x30, 0x50, 0xff]);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// All data required to render a level card.
pub struct LevelCardParams {
    pub minecraft_username: String,
    pub level: i32,
    pub total_xp: f64,
    /// XP accumulated inside the current level (total_xp - xp_for_level(level)).
    pub xp_this_level: f64,
    /// XP span of the current level (xp_for_level(level+1) - xp_for_level(level)).
    pub xp_for_next_level: f64,
    /// `(display_name, delta)` pairs, already filtered to `delta > 0`, up to 8.
    pub stat_deltas: Vec<(String, i64)>,
    pub xp_gained: f64,
    /// Raw PNG / JPEG bytes of the player's 80x80 Crafatar avatar.
    /// `None` -> a placeholder rectangle is drawn instead.
    pub avatar_bytes: Option<Vec<u8>>,
    /// Rank of the user in the guild by total XP, if available.
    pub rank: Option<i64>,

    pub milestone_progress: Vec<(i32, bool)>, // (milestone level, achieved)

    /// The player's Hypixel rank package string (e.g. `"VIP"`, `"MVP_PLUS"`, `"SUPERSTAR"`).
    pub hypixel_rank: Option<String>,
    /// The colour of the `+` symbol in the player's rank badge (e.g. `"GOLD"`, `"RED"`).
    pub hypixel_rank_plus_color: Option<String>,

    /// When `true` the card is rendered in event mode:
    /// - The "LEVEL X" label is suppressed.
    /// - The XP progress bar is replaced with a "TOTAL EVENT XP: N" label.
    /// - The "MILESTONES" section is hidden.
    pub event_mode: bool,
}

/// Render the level card and return the PNG bytes.
pub fn render(params: &LevelCardParams) -> Vec<u8> {
    debug!(
        "level_card::render: minecraft_username={}, level={}, total_xp={}, xp_this_level={}, xp_for_next_level={}, stat_deltas_len={}, xp_gained={}, has_avatar={}",
        params.minecraft_username,
        params.level,
        params.total_xp,
        params.xp_this_level,
        params.xp_for_next_level,
        params.stat_deltas.len(),
        params.xp_gained,
        params.avatar_bytes.is_some()
    );

    let font = FontRenderer::get();
    let mut img = RgbaImage::from_pixel(1000, 350, BG);

    // == INNER PANEL (rounded rect) ==========================================
    fill_rounded_rect(&mut img, 8, 8, 984, 334, 12, PANEL);

    // == AVATAR ==============================================================
    if let Some(bytes) = &params.avatar_bytes {
        debug!(
            "level_card::render: loading avatar from provided bytes (len={})",
            bytes.len()
        );
        if let Ok(dyn_img) = image::load_from_memory(bytes) {
            let avatar = dyn_img.resize_exact(80, 80, image::imageops::FilterType::Nearest);
            for ay in 0..80u32 {
                for ax in 0..80u32 {
                    if is_inside_rounded_rect(ax, ay, 80, 80, 8) {
                        img.put_pixel(28 + ax, 28 + ay, avatar.get_pixel(ax, ay));
                    }
                }
            }
        } else {
            debug!("level_card::render: failed to decode avatar bytes, drawing placeholder");
            fill_rounded_rect(&mut img, 28, 28, 80, 80, 8, MUTED);
        }
    } else {
        debug!("level_card::render: no avatar provided, drawing placeholder");
        fill_rounded_rect(&mut img, 28, 28, 80, 80, 8, MUTED);
    }

    // == TOP SECTION (Player Identity) =======================================
    let raw_rank = params.hypixel_rank.as_deref();
    let (new_pkg, monthly_pkg) = if raw_rank == Some("SUPERSTAR") {
        (None, Some("SUPERSTAR"))
    } else {
        (raw_rank, None)
    };
    let hypixel_rank = HypixelRank::from_api(new_pkg, monthly_pkg);

    let name_y: u32 = 29;
    let name_scale: u32 = 3;
    let mut name_cursor_x: u32 = 124;

    let name_col = if hypixel_rank != HypixelRank::None {
        hypixel_rank.name_color()
    } else {
        MUTED
    };

    if hypixel_rank != HypixelRank::None {
        let label = hypixel_rank.display_label();
        let plus_color = plus_color_to_rgba(params.hypixel_rank_plus_color.as_deref());

        if let Some(plus_pos) = label.find('+') {
            let before = &label[..plus_pos];
            let plus_count = label[plus_pos..].chars().take_while(|&c| c == '+').count();
            let after_start = plus_pos + plus_count;
            let after = &label[after_start..];

            font.render_text(
                &mut img,
                name_cursor_x,
                name_y,
                before,
                name_scale,
                name_col,
            );
            name_cursor_x += font.measure_text(before, name_scale);

            let plus_str = &label[plus_pos..after_start];
            font.render_text(
                &mut img,
                name_cursor_x,
                name_y,
                plus_str,
                name_scale,
                plus_color,
            );
            name_cursor_x += font.measure_text(plus_str, name_scale);

            if !after.is_empty() {
                font.render_text(&mut img, name_cursor_x, name_y, after, name_scale, name_col);
                name_cursor_x += font.measure_text(after, name_scale);
            }
        } else {
            font.render_text(&mut img, name_cursor_x, name_y, label, name_scale, name_col);
            name_cursor_x += font.measure_text(label, name_scale);
        }

        name_cursor_x += 8;
    }

    font.render_text(
        &mut img,
        name_cursor_x,
        name_y,
        &params.minecraft_username,
        name_scale,
        name_col,
    );

    if !params.event_mode {
        font.render_text(
            &mut img,
            124,
            62,
            &format!("LEVEL {}", params.level),
            2,
            CYAN,
        );
    }

    let rank_colour = if let Some(rank) = params.rank {
        if rank == 1 {
            GOLD
        } else if rank <= 3 {
            GREEN
        } else {
            MUTED
        }
    } else {
        MUTED
    };

    if !params.event_mode {
        if let Some(rank) = params.rank {
            font.render_text(
                &mut img,
                124,
                92,
                &format!("RANK #{}", rank),
                2,
                rank_colour,
            );
        }
    } else {
        if let Some(rank) = params.rank {
            font.render_text(
                &mut img,
                124,
                77,
                &format!("RANK #{}", rank),
                2,
                rank_colour,
            );
        }
    }

    // == PROGRESS BAR / EVENT XP =============================================
    if params.event_mode {
        // In event mode: show total event XP as a label instead of a progress bar.
        let xp_label = format!("TOTAL EVENT XP: {:.0}", params.total_xp);
        font.render_text(&mut img, 28, 136, &xp_label, 2, CYAN);
    } else {
        fill_rounded_rect(&mut img, 28, 120, 944, 18, 9, BAR_BG);

        let pct = if params.xp_for_next_level > 0.0 {
            (params.xp_this_level / params.xp_for_next_level).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let fill_w = (944.0 * pct).round() as u32;
        if fill_w > 0 {
            fill_rounded_rect(&mut img, 28, 120, fill_w.max(18), 18, 9, CYAN);
        }

        let percentage_complete = params.xp_this_level / params.xp_for_next_level * 100.0;

        font.render_text(
            &mut img,
            28,
            146,
            &format!(
                "{:.0} / {:.0} XP ({:.1}%)",
                params.xp_this_level, params.xp_for_next_level, percentage_complete
            ),
            2,
            MUTED,
        );
    }

    // == DIVIDER =============================================================
    fill_rect(&mut img, 28, 172, 944, 2, DIVIDER);

    // == BOTTOM SECTION (Stat Changes) =======================================
    font.render_text(&mut img, 28, 188, "STAT CHANGES", 2, CYAN);

    if params.stat_deltas.is_empty() {
        font.render_text(&mut img, 28, 214, "No changes yet", 2, MUTED);
    } else {
        let col1_x: u32 = 28;
        let col2_x: u32 = 260;
        let base_y: u32 = 214;
        let step: u32 = 24;

        for (i, (name, delta)) in params.stat_deltas.iter().take(8).enumerate() {
            let col_x = if i < 4 { col1_x } else { col2_x };
            let row = (i % 4) as u32;
            let y = base_y + row * step;
            let line = format!("+{:.0} {}", delta, name);
            font.render_text(&mut img, col_x, y, &line, 2, GREEN);
        }
    }

    // == MILESTONE BADGES ====================================================
    if !params.event_mode {
        let milestones_x = 540;
        let milestones_y = 188;
        font.render_text(&mut img, milestones_x, milestones_y, "MILESTONES", 2, CYAN);

        let max_milestones = 8;
        let col1_x = milestones_x;
        let col2_x = milestones_x + 200;
        let base_y = milestones_y + 26;
        let row_step = 22;

        let xp_pct = if params.xp_for_next_level > 0.0 {
            (params.xp_this_level / params.xp_for_next_level).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let current_fractional_level = params.level as f64 + xp_pct;

        for (i, (m_level, reached)) in params
            .milestone_progress
            .iter()
            .take(max_milestones)
            .enumerate()
        {
            // m_level is the milestone level
            let column = i / 4;
            let row = i % 4;
            let x = if column == 0 { col1_x } else { col2_x };
            let y = base_y + (row as u32) * row_step;

            let m_level_f = *m_level as f64;

            let percentage = if i == 0 {
                ((current_fractional_level / m_level_f) * 100.0)
                    .clamp(0.0, 100.0)
                    .round() as i32
            } else if let Some((prev_m_tuple, _)) = params.milestone_progress.get(i - 1) {
                let prev_m_f = *prev_m_tuple as f64;

                if current_fractional_level >= m_level_f {
                    100
                } else if current_fractional_level < prev_m_f {
                    0
                } else {
                    (((current_fractional_level - prev_m_f) / (m_level_f - prev_m_f)) * 100.0)
                        .round() as i32
                }
            } else {
                if params.level >= *m_level { 100 } else { 0 }
            };

            let color = if percentage > 0 {
                if *reached { GREEN } else { WHITE }
            } else {
                MUTED
            };

            font.render_text(
                &mut img,
                x,
                y,
                &format!("Level {} ({}%)", m_level, percentage),
                2,
                color,
            );
        }
    }

    // == XP GAINED (right-aligned to x=972) ==================================
    if !params.event_mode {
        let xp_text = format!("+{:.0} XP GAINED", params.xp_gained);
        let text_w = font.measure_text(&xp_text, 2);
        let xp_x = 972u32.saturating_sub(text_w);
        font.render_text(&mut img, xp_x, 146, &xp_text, 2, MUTED);
    }

    // == ENCODE PNG ===========================================================
    let mut buf: Vec<u8> = Vec::new();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .expect("PNG encoding should not fail");
    debug!(
        "level_card::render: finished encoding PNG (bytes={})",
        buf.len()
    );
    buf
}

// ---------------------------------------------------------------------------
// Drawing primitives
// ---------------------------------------------------------------------------

fn fill_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
    debug!("level_card::fill_rect: x={}, y={}, w={}, h={}", x, y, w, h);
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
    debug!(
        "level_card::fill_rounded_rect: x={}, y={}, w={}, h={}, r={}",
        x, y, w, h, r
    );
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

// Added tests for the milestones because there is not really any easy way to test it
#[cfg(test)]
mod tests {
    fn compute_fractional_level(level: i32, xp_this: f64, xp_next: f64) -> f64 {
        let pct = if xp_next > 0.0 {
            (xp_this / xp_next).clamp(0.0, 1.0)
        } else {
            0.0
        };
        level as f64 + pct
    }

    #[test]
    fn milestone_progress_between_levels() {
        let milestones = vec![(5, false), (10, false), (15, false)];

        let level = 10;
        let xp_this = 35.7;
        let xp_next = 100.0;

        let fractional = compute_fractional_level(level, xp_this, xp_next);

        let mut percentages = Vec::new();

        for (i, (m_level, _)) in milestones.iter().enumerate() {
            let m_level_f = *m_level as f64;

            let pct = if i == 0 {
                ((fractional / m_level_f) * 100.0).clamp(0.0, 100.0).round() as i32
            } else {
                let prev = milestones[i - 1].0 as f64;

                if fractional >= m_level_f {
                    100
                } else if fractional < prev {
                    0
                } else {
                    (((fractional - prev) / (m_level_f - prev)) * 100.0).round() as i32
                }
            };

            percentages.push(pct);
        }

        assert_eq!(percentages[0], 100); // level 5
        assert_eq!(percentages[1], 100); // level 10
        assert!(percentages[2] > 0 && percentages[2] < 10); // level 15 ≈ 7%
    }
}
