// --- ENHANCED ICON PAINTER MODULE V2 ---
// High-fidelity programmatic vector icons for egui.
// No assets, no fonts, pure math.

use eframe::egui;
use std::f32::consts::PI;

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Settings,


    EyeOpen,
    EyeClosed,
    Microphone,
    Image,

    Text, // NEW: 'T' icon for text presets
    Delete, // Renders as Trash Can (used for presets)
    DeleteLarge, // NEW: Centered, larger Trash Can (used for history items)

    Folder, // NEW: For "Open Media"
    Copy,   // NEW: For "Copy Text"
    CopySmall, // NEW: Smaller copy icon for preset buttons
    Close,  // NEW: "X" for clearing search

    TextSelect, // NEW: Text with selection cursor for text selection mode
    Speaker, // NEW: Speaker icon for device audio source
    Lightbulb, // NEW: Lightbulb icon for tips
    Realtime, // NEW: Streaming waves icon for realtime audio processing
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

/// Public function to paint an icon directly (for custom layouts where icon_button isn't suitable)
pub fn paint_icon(painter: &egui::Painter, rect: egui::Rect, icon: Icon, color: egui::Color32) {
    paint_internal(painter, rect, icon, color);
}

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
            painter.rect_stroke(caps_rect, w/2.0, stroke, egui::StrokeKind::Middle);
            
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
            painter.rect_stroke(img_rect, 2.0 * scale, stroke, egui::StrokeKind::Middle);
            let p1 = img_rect.left_bottom() - egui::vec2(-1.0, 2.0)*scale;
            let p2 = img_rect.left_bottom() + egui::vec2(3.0, -6.0)*scale; 
            let p3 = img_rect.left_bottom() + egui::vec2(6.0, -3.0)*scale; 
            let p4 = img_rect.left_bottom() + egui::vec2(9.0, -7.0)*scale; 
            let p5 = img_rect.right_bottom() - egui::vec2(1.0, 2.0)*scale;
            painter.add(egui::Shape::line(vec![p1, p2, p3, p4, p5], stroke));
            painter.circle_filled(img_rect.left_top() + egui::vec2(3.5, 3.5)*scale, 1.5*scale, color);
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
            painter.rect_stroke(back_rect, 1.0 * scale, stroke, egui::StrokeKind::Middle);

            // Front rect (Bottom Right) - Filled to cover back lines
            let front_rect = egui::Rect::from_center_size(center + egui::vec2(offset, offset), egui::vec2(w, h));
            painter.rect_filled(front_rect, 1.0 * scale, painter.ctx().style().visuals.panel_fill); // Mask
            painter.rect_stroke(front_rect, 1.0 * scale, stroke, egui::StrokeKind::Middle);
        }

        Icon::CopySmall => {
            // Two overlapping rectangles - MINI SIZE for preset buttons
            let w = 5.0 * scale;
            let h = 6.5 * scale;
            let offset = 1.2 * scale;

            // Back rect (Top Left)
            let back_rect = egui::Rect::from_center_size(center - egui::vec2(offset/2.0, offset/2.0), egui::vec2(w, h));
            painter.rect_stroke(back_rect, 0.8 * scale, stroke, egui::StrokeKind::Middle);

            // Front rect (Bottom Right) - Filled to cover back lines
            let front_rect = egui::Rect::from_center_size(center + egui::vec2(offset, offset), egui::vec2(w, h));
            painter.rect_filled(front_rect, 0.8 * scale, painter.ctx().style().visuals.panel_fill); // Mask
            painter.rect_stroke(front_rect, 0.8 * scale, stroke, egui::StrokeKind::Middle);
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





        Icon::TextSelect => {
            // Text with selection highlight/cursor - represents "select text" mode
            // Draw 3 horizontal lines (text lines) with middle one highlighted
            let line_w = 12.0 * scale;
            let line_gap = 4.0 * scale;
            let line_y1 = center.y - line_gap;
            let line_y2 = center.y;
            let line_y3 = center.y + line_gap;
            
            // Text lines
            painter.line_segment([egui::pos2(center.x - line_w/2.0, line_y1), egui::pos2(center.x + line_w/2.0, line_y1)], stroke);
            painter.line_segment([egui::pos2(center.x - line_w/2.0, line_y3), egui::pos2(center.x + line_w/2.0, line_y3)], stroke);
            
            // Highlighted middle line (thicker, representing selection)
            let highlight_stroke = egui::Stroke::new(3.0 * scale, color);
            painter.line_segment([egui::pos2(center.x - line_w/2.0, line_y2), egui::pos2(center.x + line_w/2.0, line_y2)], highlight_stroke);
            
            // Cursor (vertical line with serifs at ends)
            let cursor_x = center.x + line_w/2.0 + 2.0 * scale;
            let cursor_top = center.y - 5.0 * scale;
            let cursor_bot = center.y + 5.0 * scale;
            let serif_w = 1.5 * scale;
            painter.line_segment([egui::pos2(cursor_x, cursor_top), egui::pos2(cursor_x, cursor_bot)], stroke);
            painter.line_segment([egui::pos2(cursor_x - serif_w, cursor_top), egui::pos2(cursor_x + serif_w, cursor_top)], stroke);
            painter.line_segment([egui::pos2(cursor_x - serif_w, cursor_bot), egui::pos2(cursor_x + serif_w, cursor_bot)], stroke);
        }

        Icon::Speaker => {
            // Speaker with sound waves - for device audio (system sound)
            // Speaker body (trapezoid + rectangle)
            let body_x = center.x - 3.0 * scale;
            let body_w = 4.0 * scale;
            let body_h = 6.0 * scale;
            let cone_w = 5.0 * scale;
            let cone_h = 10.0 * scale;
            
            // Rectangle (back of speaker)
            let rect = egui::Rect::from_center_size(
                egui::pos2(body_x - body_w/2.0, center.y),
                egui::vec2(body_w, body_h)
            );
            painter.rect_stroke(rect, 0.5 * scale, stroke, egui::StrokeKind::Middle);
            
            // Cone (trapezoid)
            let cone_pts = vec![
                egui::pos2(body_x, center.y - body_h/2.0),           // top-left
                egui::pos2(body_x + cone_w, center.y - cone_h/2.0), // top-right
                egui::pos2(body_x + cone_w, center.y + cone_h/2.0), // bottom-right
                egui::pos2(body_x, center.y + body_h/2.0),           // bottom-left
            ];
            painter.add(egui::Shape::closed_line(cone_pts, stroke));
            
            // Sound waves (arcs)
            let wave_x = center.x + 4.0 * scale;
            let wave_r1 = 3.0 * scale;
            let wave_r2 = 5.5 * scale;
            
            // First wave
            let wave_segments = 8;
            let wave_angle = PI / 3.0;
            let mut wave1_pts = Vec::new();
            for i in 0..=wave_segments {
                let t = i as f32 / wave_segments as f32;
                let angle = -wave_angle + 2.0 * wave_angle * t;
                wave1_pts.push(egui::pos2(wave_x + wave_r1 * angle.cos(), center.y + wave_r1 * angle.sin()));
            }
            painter.add(egui::Shape::line(wave1_pts, stroke));
            
            // Second wave
            let mut wave2_pts = Vec::new();
            for i in 0..=wave_segments {
                let t = i as f32 / wave_segments as f32;
                let angle = -wave_angle + 2.0 * wave_angle * t;
                wave2_pts.push(egui::pos2(wave_x + wave_r2 * angle.cos(), center.y + wave_r2 * angle.sin()));
            }
            painter.add(egui::Shape::line(wave2_pts, stroke));
        }

        Icon::Lightbulb => {
            // Simple lightbulb icon using explicit coordinates
            // The bulb consists of: circle top + tapered neck + base + rays
            
            let bulb_r = 4.5 * scale;
            let bulb_cy = center.y - 2.0 * scale; // Center of bulb circle (shifted up)
            
            // 1. Draw bulb circle (full circle)
            painter.circle_stroke(egui::pos2(center.x, bulb_cy), bulb_r, stroke);
            
            // 2. Draw neck (two converging lines from bulb bottom to base)
            let neck_top_w = 3.0 * scale;  // Width at top of neck
            let neck_bot_w = 2.0 * scale;  // Width at bottom of neck
            let neck_top_y = bulb_cy + bulb_r;
            let neck_bot_y = neck_top_y + 3.0 * scale;
            
            // Left neck line
            painter.line_segment(
                [egui::pos2(center.x - neck_top_w, neck_top_y), 
                 egui::pos2(center.x - neck_bot_w, neck_bot_y)],
                stroke
            );
            // Right neck line
            painter.line_segment(
                [egui::pos2(center.x + neck_top_w, neck_top_y), 
                 egui::pos2(center.x + neck_bot_w, neck_bot_y)],
                stroke
            );
            
            // 3. Draw base (two horizontal lines)
            painter.line_segment(
                [egui::pos2(center.x - neck_bot_w, neck_bot_y), 
                 egui::pos2(center.x + neck_bot_w, neck_bot_y)],
                stroke
            );
            painter.line_segment(
                [egui::pos2(center.x - neck_bot_w * 0.7, neck_bot_y + 1.5 * scale), 
                 egui::pos2(center.x + neck_bot_w * 0.7, neck_bot_y + 1.5 * scale)],
                stroke
            );
            
            // 4. Draw rays (3 lines going up from top of bulb)
            let ray_start_y = bulb_cy - bulb_r - 1.5 * scale; // Start above the bulb with gap
            let ray_len = 2.5 * scale;
            
            // Center ray (straight up)
            painter.line_segment(
                [egui::pos2(center.x, ray_start_y), 
                 egui::pos2(center.x, ray_start_y - ray_len)],
                stroke
            );
            // Left ray (diagonal)
            painter.line_segment(
                [egui::pos2(center.x - 2.5 * scale, ray_start_y + 1.0 * scale), 
                 egui::pos2(center.x - 4.0 * scale, ray_start_y - ray_len + 1.5 * scale)],
                stroke
            );
            // Right ray (diagonal)
            painter.line_segment(
                [egui::pos2(center.x + 2.5 * scale, ray_start_y + 1.0 * scale), 
                 egui::pos2(center.x + 4.0 * scale, ray_start_y - ray_len + 1.5 * scale)],
                stroke
            );
        }

        Icon::Realtime => {
            // Realtime waveform icon - audio oscilloscope pattern
            // Horizontal line with peaks and valleys representing live audio
            
            let y_center = center.y;
            let wave_stroke = egui::Stroke::new(2.0 * scale, color);
            
            // Left flat segment
            let left_start = center.x - 10.0 * scale;
            let left_end = center.x - 7.0 * scale;
            painter.line_segment(
                [egui::pos2(left_start, y_center), egui::pos2(left_end, y_center)],
                wave_stroke
            );
            
            // Waveform points - small peak, big valley, big peak, small valley pattern
            let wave_pts = vec![
                egui::pos2(left_end, y_center),                          // start
                egui::pos2(center.x - 5.5 * scale, y_center - 3.0 * scale), // small peak
                egui::pos2(center.x - 3.5 * scale, y_center + 7.0 * scale), // big valley
                egui::pos2(center.x, y_center - 7.0 * scale),              // big peak
                egui::pos2(center.x + 3.5 * scale, y_center + 3.0 * scale), // small valley
                egui::pos2(center.x + 5.5 * scale, y_center),              // return to center
            ];
            painter.add(egui::Shape::line(wave_pts, wave_stroke));
            
            // Right flat segment
            let right_start = center.x + 5.5 * scale;
            let right_end = center.x + 10.0 * scale;
            painter.line_segment(
                [egui::pos2(right_start, y_center), egui::pos2(right_end, y_center)],
                wave_stroke
            );
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
