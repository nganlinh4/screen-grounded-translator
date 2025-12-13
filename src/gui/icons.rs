// --- ENHANCED ICON PAINTER MODULE V2 ---
// High-fidelity programmatic vector icons for egui.
// No assets, no fonts, pure math.

use eframe::egui;
use std::f32::consts::PI;

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Settings,
    Moon,
    Sun,
    SystemTheme, // NEW: Computer Monitor for System/Auto theme
    EyeOpen,
    EyeClosed,
    Microphone,
    Image,
    Video,
    Text, // NEW: 'T' icon for text presets
    Delete, // Renders as Trash Can (used for presets)
    DeleteLarge, // NEW: Centered, larger Trash Can (used for history items)
    Info,
    Statistics,
    Refresh,
    Folder, // NEW: For "Open Media"
    Copy,   // NEW: For "Copy Text"
    CopySmall, // NEW: Smaller copy icon for preset buttons
    Close,  // NEW: "X" for clearing search
    ChainArrow, // NEW: Arrow for chain links
    ChainArrowDown, // NEW: Vertical arrow for chain steps
}

/// Main entry point: Draw a clickable icon button (default size 24.0)
pub fn icon_button(ui: &mut egui::Ui, icon: Icon) -> egui::Response {
    icon_button_sized(ui, icon, 24.0)
}

/// Draw a clickable icon button with custom size
pub fn icon_button_sized(ui: &mut egui::Ui, icon: Icon, size_val: f32) -> egui::Response {
    let size = egui::vec2(size_val, size_val); 
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    // 1. Background Hover Effect
    if response.hovered() {
        ui.painter().rect_filled(
            rect.shrink(2.0),
            4.0,
            ui.visuals().widgets.hovered.bg_fill,
        );
    }

    // 2. Determine Style
    let color = if response.hovered() {
        ui.visuals().widgets.hovered.fg_stroke.color
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };

    // 3. Paint
    paint_internal(ui.painter(), rect, icon, color);

    response
}

/// Draw a static icon (for labels/headers)
pub fn draw_icon_static(ui: &mut egui::Ui, icon: Icon, size_override: Option<f32>) {
    let side = size_override.unwrap_or(16.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let color = ui.visuals().text_color();
    paint_internal(ui.painter(), rect, icon, color);
}

// --- INTERNAL PAINTER ENGINE ---

fn paint_internal(painter: &egui::Painter, rect: egui::Rect, icon: Icon, color: egui::Color32) {
    let center = rect.center();
    // Base scale on a 20x20 reference grid, scaled to actual rect
    let scale = rect.width().min(rect.height()) / 22.0;
    let stroke = egui::Stroke::new(1.5 * scale, color); // Consistent line weight

    match icon {
        Icon::Settings => {
            // Modern Cogwheel
            let teeth = 8;
            let outer_r = 9.0 * scale;
            let inner_r = 6.5 * scale;
            let hole_r = 2.5 * scale;

            let mut points = Vec::new();
            for i in 0..(teeth * 2) {
                let theta = (i as f32 * PI) / teeth as f32;
                let r = if i % 2 == 0 { outer_r } else { inner_r };

                let bevel_angle = (PI / teeth as f32) * 0.25;
                let theta_a = theta - bevel_angle;
                let theta_b = theta + bevel_angle;

                points.push(center + egui::vec2(theta_a.cos() * r, theta_a.sin() * r));
                points.push(center + egui::vec2(theta_b.cos() * r, theta_b.sin() * r));
            }
            points.push(points[0]); 

            painter.add(egui::Shape::line(points, stroke));
            painter.circle_stroke(center, hole_r, stroke);
        }

        Icon::Moon => {
            let r = 7.0 * scale;
            let offset = 3.5 * scale;
            painter.circle_filled(center, r, color);
            painter.circle_filled(
                center + egui::vec2(offset, -offset * 0.8),
                r * 0.85,
                painter.ctx().style().visuals.panel_fill, 
            );
        }

        Icon::Sun => {
            painter.circle_stroke(center, 4.0 * scale, stroke);
            for i in 0..8 {
                let angle = (i as f32 * 45.0).to_radians();
                let dir = egui::vec2(angle.cos(), angle.sin());
                let start = center + dir * 6.5 * scale;
                let end = center + dir * 9.0 * scale;
                painter.line_segment([start, end], stroke);
            }
        }

        Icon::SystemTheme => {
            // Computer Monitor Icon
            let screen_w = 14.0 * scale;
            let screen_h = 10.0 * scale;
            let screen_rect = egui::Rect::from_center_size(center - egui::vec2(0.0, 2.0 * scale), egui::vec2(screen_w, screen_h));
            
            // Screen Frame
            painter.rect_stroke(screen_rect, 2.0 * scale, stroke);
            
            // Stand
            let stand_top = egui::pos2(screen_rect.center().x, screen_rect.bottom());
            let stand_bot = stand_top + egui::vec2(0.0, 3.0 * scale);
            painter.line_segment([stand_top, stand_bot], stroke);
            
            // Base
            let base_w = 8.0 * scale;
            painter.line_segment(
                [stand_bot - egui::vec2(base_w/2.0, 0.0), stand_bot + egui::vec2(base_w/2.0, 0.0)], 
                stroke
            );
            
            // "Auto" glare inside screen (diagonal line)
            painter.line_segment(
                [screen_rect.left_bottom() + egui::vec2(3.0*scale, -3.0*scale), screen_rect.right_top() + egui::vec2(-3.0*scale, 3.0*scale)], 
                egui::Stroke::new(1.0 * scale, stroke.color.linear_multiply(0.5))
            );
        }

        Icon::EyeOpen => {
            let w = 9.0 * scale;
            let h = 5.0 * scale;
            let p_left = center - egui::vec2(w, 0.0);
            let p_right = center + egui::vec2(w, 0.0);
            let p_top = center - egui::vec2(0.0, h * 1.5);
            let p_bot = center + egui::vec2(0.0, h * 1.5);

            let pts_top = bezier_points(p_left, p_top, p_right, 10);
            let pts_bot = bezier_points(p_right, p_bot, p_left, 10);

            let mut full_eye = pts_top;
            full_eye.extend(pts_bot);

            painter.add(egui::Shape::line(full_eye, stroke));
            painter.circle_filled(center, 2.5 * scale, color);
        }

        Icon::EyeClosed => {
            let w = 9.0 * scale;
            let h = 5.0 * scale;
            let p_left = center - egui::vec2(w, 0.0);
            let p_right = center + egui::vec2(w, 0.0);
            let p_top = center - egui::vec2(0.0, h * 1.5);
            let pts = bezier_points(p_left, p_top, p_right, 12);
            painter.add(egui::Shape::line(pts, stroke));

            let lash_y = center.y + 1.0 * scale;
            let l_len = 3.5 * scale;
            painter.line_segment([egui::pos2(center.x, lash_y), egui::pos2(center.x, lash_y + l_len)], stroke);
            painter.line_segment([egui::pos2(center.x - 3.0*scale, lash_y - 1.0*scale), egui::pos2(center.x - 5.0*scale, lash_y + l_len*0.8)], stroke);
            painter.line_segment([egui::pos2(center.x + 3.0*scale, lash_y - 1.0*scale), egui::pos2(center.x + 5.0*scale, lash_y + l_len*0.8)], stroke);
        }

        Icon::Microphone => {
            // Larger Microphone icon
            let w = 6.5 * scale;
            let h = 12.0 * scale;
            let caps_rect = egui::Rect::from_center_size(center - egui::vec2(0.0, 1.5*scale), egui::vec2(w, h));
            painter.rect_stroke(caps_rect, w/2.0, stroke);
            
            // Horizontal lines on mic head
            let y_start = caps_rect.top() + 3.5 * scale;
            painter.line_segment([egui::pos2(center.x - 2.0*scale, y_start), egui::pos2(center.x + 2.0*scale, y_start)], stroke);
            painter.line_segment([egui::pos2(center.x - 2.0*scale, y_start + 3.0*scale), egui::pos2(center.x + 2.0*scale, y_start + 3.0*scale)], stroke);

            // U-shaped holder
            let u_left = egui::pos2(center.x - 5.5*scale, center.y);
            let u_right = egui::pos2(center.x + 5.5*scale, center.y);
            let u_bot = egui::pos2(center.x, center.y + 7.0*scale);
            let u_path = bezier_points(u_left, u_bot, u_right, 10);
            painter.add(egui::Shape::line(u_path, stroke));
            
            // Stand
            painter.line_segment([egui::pos2(center.x, center.y + 4.5*scale), egui::pos2(center.x, center.y + 9.0*scale)], stroke);
            painter.line_segment([egui::pos2(center.x - 4.0*scale, center.y + 9.0*scale), egui::pos2(center.x + 4.0*scale, center.y + 9.0*scale)], stroke);
        }

        Icon::Image => {
            let img_rect = rect.shrink(3.0 * scale);
            painter.rect_stroke(img_rect, 2.0 * scale, stroke);
            let p1 = img_rect.left_bottom() - egui::vec2(-1.0, 2.0)*scale;
            let p2 = img_rect.left_bottom() + egui::vec2(3.0, -6.0)*scale; 
            let p3 = img_rect.left_bottom() + egui::vec2(6.0, -3.0)*scale; 
            let p4 = img_rect.left_bottom() + egui::vec2(9.0, -7.0)*scale; 
            let p5 = img_rect.right_bottom() - egui::vec2(1.0, 2.0)*scale;
            painter.add(egui::Shape::line(vec![p1, p2, p3, p4, p5], stroke));
            painter.circle_filled(img_rect.left_top() + egui::vec2(3.5, 3.5)*scale, 1.5*scale, color);
        }

        Icon::Video => {
            // Larger Video camera icon
            let body_w = 14.0 * scale;
            let body_h = 10.0 * scale;
            let body_rect = egui::Rect::from_center_size(center - egui::vec2(1.5*scale, 0.0), egui::vec2(body_w, body_h));
            painter.rect_stroke(body_rect, 2.0 * scale, stroke);
            
            // Lens (triangle on right side)
            let l_x = body_rect.right();
            let l_y = center.y;
            let lens_pts = vec![
                egui::pos2(l_x, l_y - 3.0*scale),
                egui::pos2(l_x + 5.0*scale, l_y - 4.5*scale),
                egui::pos2(l_x + 5.0*scale, l_y + 4.5*scale),
                egui::pos2(l_x, l_y + 3.0*scale),
            ];
            painter.add(egui::Shape::closed_line(lens_pts, stroke));
            
            // Film reels on top
            painter.circle_stroke(body_rect.left_top() + egui::vec2(3.5, 0.0)*scale, 2.0*scale, stroke);
            painter.circle_stroke(body_rect.right_top() + egui::vec2(-3.5, 0.0)*scale, 2.0*scale, stroke);
        }

        Icon::Text => {
            // Larger Elegant Serif 'T' Icon
            let top_y = center.y - 8.0 * scale;
            let bot_y = center.y + 8.0 * scale;
            let left_x = center.x - 7.0 * scale;
            let right_x = center.x + 7.0 * scale;
            let serif_h = 2.0 * scale; // Height of serifs
            let stem_w = 2.5 * scale;  // Half-width of stem base serif
            
            // Top horizontal bar (thicker)
            let bar_stroke = egui::Stroke::new(2.5 * scale, color);
            painter.line_segment([egui::pos2(left_x, top_y), egui::pos2(right_x, top_y)], bar_stroke);
            
            // Left serif (small vertical line at top-left)
            painter.line_segment([egui::pos2(left_x, top_y), egui::pos2(left_x, top_y + serif_h)], stroke);
            
            // Right serif (small vertical line at top-right)
            painter.line_segment([egui::pos2(right_x, top_y), egui::pos2(right_x, top_y + serif_h)], stroke);
            
            // Vertical stem (thicker)
            let stem_stroke = egui::Stroke::new(2.0 * scale, color);
            painter.line_segment([egui::pos2(center.x, top_y), egui::pos2(center.x, bot_y)], stem_stroke);
            
            // Bottom serif (horizontal line at base of stem)
            painter.line_segment([egui::pos2(center.x - stem_w, bot_y), egui::pos2(center.x + stem_w, bot_y)], stroke);
        }

        Icon::Delete => {
            // Trash Can (original, for presets) - centered in hitbox
            let c = center;
            let lid_y = c.y - 3.2 * scale;
            let w_lid = 8.0 * scale; 
            let w_can_top = 6.0 * scale;
            let w_can_bot = 4.5 * scale;
            let h_can = 7.0 * scale;

            painter.line_segment([egui::pos2(c.x - w_lid/2.0, lid_y), egui::pos2(c.x + w_lid/2.0, lid_y)], stroke);
            painter.line_segment([egui::pos2(c.x - 1.0*scale, lid_y), egui::pos2(c.x - 1.0*scale, lid_y - 1.0*scale)], stroke);
            painter.line_segment([egui::pos2(c.x - 1.0*scale, lid_y - 1.0*scale), egui::pos2(c.x + 1.0*scale, lid_y - 1.0*scale)], stroke);
            painter.line_segment([egui::pos2(c.x + 1.0*scale, lid_y - 1.0*scale), egui::pos2(c.x + 1.0*scale, lid_y)], stroke);

            let p1 = egui::pos2(c.x - w_can_top/2.0, lid_y);
            let p2 = egui::pos2(c.x - w_can_bot/2.0, lid_y + h_can);
            let p3 = egui::pos2(c.x + w_can_bot/2.0, lid_y + h_can);
            let p4 = egui::pos2(c.x + w_can_top/2.0, lid_y);
            painter.add(egui::Shape::line(vec![p1, p2, p3, p4], stroke));
        }

        Icon::DeleteLarge => {
            // Trash Can (centered and larger, for history items)
            let c = center; // Removed manual offset
            let lid_y = c.y - 4.0 * scale; // Lid line position
            let w_lid = 10.0 * scale;      // Wider lid
            let w_can_top = 8.0 * scale;   // Wider body top
            let w_can_bot = 6.0 * scale;   // Wider body bottom
            let h_can = 9.0 * scale;       // Taller body

            // Lid line
            painter.line_segment([egui::pos2(c.x - w_lid/2.0, lid_y), egui::pos2(c.x + w_lid/2.0, lid_y)], stroke);
            
            // Handle (small loop above lid)
            painter.line_segment([egui::pos2(c.x - 1.0*scale, lid_y), egui::pos2(c.x - 1.0*scale, lid_y - 1.0*scale)], stroke);
            painter.line_segment([egui::pos2(c.x - 1.0*scale, lid_y - 1.0*scale), egui::pos2(c.x + 1.0*scale, lid_y - 1.0*scale)], stroke);
            painter.line_segment([egui::pos2(c.x + 1.0*scale, lid_y - 1.0*scale), egui::pos2(c.x + 1.0*scale, lid_y)], stroke);

            // Can Body (Trapezoid)
            let p1 = egui::pos2(c.x - w_can_top/2.0, lid_y);
            let p2 = egui::pos2(c.x - w_can_bot/2.0, lid_y + h_can);
            let p3 = egui::pos2(c.x + w_can_bot/2.0, lid_y + h_can);
            let p4 = egui::pos2(c.x + w_can_top/2.0, lid_y);
            painter.add(egui::Shape::line(vec![p1, p2, p3, p4], stroke));
        }

        Icon::Info => {
            let c = center - egui::vec2(0.0, 1.0 * scale);
            painter.circle_stroke(c, 5.0 * scale, stroke);
            painter.circle_filled(c - egui::vec2(0.0, 1.8 * scale), 0.6 * scale, color);
            painter.rect_filled(
                egui::Rect::from_center_size(c + egui::vec2(0.0, 1.0 * scale), egui::vec2(1.0 * scale, 2.5 * scale)),
                0.4 * scale, color,
            );
        }

        Icon::Statistics => {
            let base_y = center.y + 6.0 * scale;
            let bar_w = 2.5 * scale;
            let gap = 1.5 * scale;
            let h1 = 4.0 * scale;
            let h2 = 7.0 * scale;
            let h3 = 10.0 * scale;
            let x1 = center.x - bar_w - gap;
            let x2 = center.x;
            let x3 = center.x + bar_w + gap;

            painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x1 - bar_w/2.0, base_y - h1), egui::pos2(x1 + bar_w/2.0, base_y)), 1.0, color);
            painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x2 - bar_w/2.0, base_y - h2), egui::pos2(x2 + bar_w/2.0, base_y)), 1.0, color);
            painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x3 - bar_w/2.0, base_y - h3), egui::pos2(x3 + bar_w/2.0, base_y)), 1.0, color);
            
            let t_offset = 3.0 * scale; 
            let points = vec![
                egui::pos2(x1 - bar_w, base_y - h1 - t_offset + 2.0*scale), 
                egui::pos2(x1, base_y - h1 - t_offset),
                egui::pos2(x2, base_y - h2 - t_offset),
                egui::pos2(x3, base_y - h3 - t_offset),
                egui::pos2(x3 + bar_w, base_y - h3 - t_offset - 2.0*scale),
            ];
            painter.add(egui::Shape::line(points, egui::Stroke::new(1.2 * scale, color)));
        }

        Icon::Refresh => {
            let r = 6.0 * scale;
            let refresh_stroke = egui::Stroke::new(1.2 * scale, color);
            let segments = 30;
            let start_angle = -PI / 2.0 + 0.6;
            let sweep = 2.0 * PI - 1.2;
            let mut points = Vec::new();
            for i in 0..=segments {
                let t = i as f32 / segments as f32;
                let angle = start_angle + sweep * t;
                points.push(center + egui::vec2(angle.cos() * r, angle.sin() * r));
            }
            painter.add(egui::Shape::line(points.clone(), refresh_stroke));
            
            if let Some(tip) = points.last() {
                let end_angle = start_angle + sweep;
                let arrow_len = 3.5 * scale;
                let tangent = end_angle + PI / 2.0;
                let wing_offset = 0.6; 
                let back_angle1 = tangent - PI + wing_offset;
                let back_angle2 = tangent - PI - wing_offset;
                let p1 = *tip + egui::vec2(back_angle1.cos() * arrow_len, back_angle1.sin() * arrow_len);
                let p2 = *tip + egui::vec2(back_angle2.cos() * arrow_len, back_angle2.sin() * arrow_len);
                painter.add(egui::Shape::line(vec![p1, *tip, p2], refresh_stroke));
            }
        }

        Icon::Folder => {
            // Folder Icon
            let w = 14.0 * scale;
            let h = 10.0 * scale;
            let body_rect = egui::Rect::from_center_size(center + egui::vec2(0.0, 1.0*scale), egui::vec2(w, h));
            
            // Tab (top left)
            let tab_w = 6.0 * scale;
            let tab_h = 2.0 * scale;
            
            // Draw Outline
            // Manual path to make it look joined
            let p1 = body_rect.left_top();
            let p2 = body_rect.left_bottom();
            let p3 = body_rect.right_bottom();
            let p4 = body_rect.right_top();
            let p5 = body_rect.left_top() + egui::vec2(tab_w, 0.0);
            let p6 = body_rect.left_top() + egui::vec2(tab_w, -tab_h);
            let p7 = body_rect.left_top() + egui::vec2(0.0, -tab_h);

            painter.add(egui::Shape::line(vec![p7, p1, p2, p3, p4, p5, p6, p7], stroke));
        }

        Icon::Copy => {
            // Two overlapping rectangles - REDUCED SIZE to match Trashcan
            let w = 7.0 * scale; // Reduced from 8.0
            let h = 9.0 * scale; // Reduced from 10.0
            let offset = 2.0 * scale; // Reduced from 2.5

            // Back rect (Top Left)
            let back_rect = egui::Rect::from_center_size(center - egui::vec2(offset/2.0, offset/2.0), egui::vec2(w, h));
            painter.rect_stroke(back_rect, 1.0 * scale, stroke);

            // Front rect (Bottom Right) - Filled to cover back lines
            let front_rect = egui::Rect::from_center_size(center + egui::vec2(offset, offset), egui::vec2(w, h));
            painter.rect_filled(front_rect, 1.0 * scale, painter.ctx().style().visuals.panel_fill); // Mask
            painter.rect_stroke(front_rect, 1.0 * scale, stroke);
        }

        Icon::CopySmall => {
            // Two overlapping rectangles - MINI SIZE for preset buttons
            let w = 5.0 * scale;
            let h = 6.5 * scale;
            let offset = 1.2 * scale;

            // Back rect (Top Left)
            let back_rect = egui::Rect::from_center_size(center - egui::vec2(offset/2.0, offset/2.0), egui::vec2(w, h));
            painter.rect_stroke(back_rect, 0.8 * scale, stroke);

            // Front rect (Bottom Right) - Filled to cover back lines
            let front_rect = egui::Rect::from_center_size(center + egui::vec2(offset, offset), egui::vec2(w, h));
            painter.rect_filled(front_rect, 0.8 * scale, painter.ctx().style().visuals.panel_fill); // Mask
            painter.rect_stroke(front_rect, 0.8 * scale, stroke);
        }

        Icon::Close => {
            // 'X' Icon
            let sz = 5.0 * scale;
            let p1 = center - egui::vec2(sz, sz);
            let p2 = center + egui::vec2(sz, sz);
            let p3 = center - egui::vec2(sz, -sz);
            let p4 = center + egui::vec2(sz, -sz);
            
            painter.line_segment([p1, p2], stroke);
            painter.line_segment([p3, p4], stroke);
        }

        Icon::ChainArrow => {
            // Horizontal right arrow (->) for chain links in header
            let arrow_len = 6.0 * scale;
            let head_sz = 3.0 * scale;
            
            // Shaft
            let start = center - egui::vec2(arrow_len, 0.0);
            let end = center + egui::vec2(arrow_len, 0.0);
            painter.line_segment([start, end], stroke);
            
            // Arrowhead
            painter.line_segment([end, end - egui::vec2(head_sz, head_sz)], stroke);
            painter.line_segment([end, end - egui::vec2(head_sz, -head_sz)], stroke);
        }

        Icon::ChainArrowDown => {
            // Vertical down arrow for chain steps
            let arrow_len = 5.0 * scale;
            let head_sz = 3.0 * scale;
            
            // Shaft
            let start = center - egui::vec2(0.0, arrow_len);
            let end = center + egui::vec2(0.0, arrow_len);
            painter.line_segment([start, end], stroke);
            
            // Arrowhead
            painter.line_segment([end, end - egui::vec2(head_sz, head_sz)], stroke);
            painter.line_segment([end, end - egui::vec2(-head_sz, head_sz)], stroke);
        }
    }
}

// --- MATH HELPERS ---

fn lerp(a: egui::Pos2, b: egui::Pos2, t: f32) -> egui::Pos2 {
    egui::pos2(
        a.x + (b.x - a.x) * t,
        a.y + (b.y - a.y) * t,
    )
}

fn lerp_quadratic(p0: egui::Pos2, p1: egui::Pos2, p2: egui::Pos2, t: f32) -> egui::Pos2 {
    let l1 = lerp(p0, p1, t);
    let l2 = lerp(p1, p2, t);
    lerp(l1, l2, t)
}

fn bezier_points(p0: egui::Pos2, p1: egui::Pos2, p2: egui::Pos2, segments: usize) -> Vec<egui::Pos2> {
    let mut points = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        points.push(lerp_quadratic(p0, p1, p2, t));
    }
    points
}
