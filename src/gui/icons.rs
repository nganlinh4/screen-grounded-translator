// --- ENHANCED ICON PAINTER MODULE ---
// Programmatic vector icons using egui Painter with Bezier curves
// Eliminates dependency on font files, saves ~2-4MB in binary size

use eframe::egui;

#[derive(Clone, Copy)]
pub enum Icon {
    Settings,
    Moon,
    Sun,
    EyeOpen,
    EyeClosed,
    Microphone,
    Image,
    Video,
    Delete,
    Info,
    Statistics,
}

// Math helper for smooth quadratic Bezier interpolation
fn lerp_quadratic(p0: egui::Pos2, p1: egui::Pos2, p2: egui::Pos2, t: f32) -> egui::Pos2 {
    let l1 = p0.lerp(p1, t);
    let l2 = p1.lerp(p2, t);
    l1.lerp(l2, t)
}

/// Draw an interactive icon button with hover effects
pub fn icon_button(ui: &mut egui::Ui, icon: Icon) -> egui::Response {
    let size = egui::vec2(20.0, 20.0); // Slightly larger touch target
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    // 1. Hover Effect (Subtle rounded square)
    if response.hovered() {
        ui.painter().rect_filled(
            rect.shrink(1.0),
            4.0,
            ui.visuals().widgets.hovered.bg_fill,
        );
    }

    // 2. Setup Painter
    let painter = ui.painter();
    // Use slightly thinner lines for elegance
    let stroke_width = 1.3;

    let color = if response.hovered() {
        ui.visuals().widgets.hovered.fg_stroke.color
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    let stroke = egui::Stroke::new(stroke_width, color);
    let center = rect.center();

    // 3. Draw High-Fidelity Icons
    match icon {
        Icon::Settings => {
            // Gear: Draw circle + 6 teeth
            painter.circle_stroke(center, 4.5, stroke);
            for i in 0..6 {
                let angle = (i as f32 * 60.0).to_radians();
                let dir = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [center + dir * 4.5, center + dir * 7.5],
                    egui::Stroke::new(2.5, color),
                );
            }
        }
        Icon::Moon => {
            // Crescent
            let r = 7.0;
            painter.circle_filled(center, r, color);
            painter.circle_filled(
                center + egui::vec2(4.0, -3.0),
                r * 0.85,
                ui.visuals().panel_fill, // Mask
            );
        }
        Icon::Sun => {
            painter.circle_stroke(center, 3.5, stroke);
            for i in 0..8 {
                let angle = (i as f32 * 45.0).to_radians();
                let dir = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [center + dir * 5.5, center + dir * 8.5],
                    egui::Stroke::new(1.0, color),
                );
            }
        }
        Icon::EyeOpen => {
            // Almond shape using Bezier curves
            let w = 8.0;
            let h = 4.5;
            let left = center - egui::vec2(w, 0.0);
            let right = center + egui::vec2(w, 0.0);
            let top_ctrl = center - egui::vec2(0.0, h * 1.8);
            let bot_ctrl = center + egui::vec2(0.0, h * 1.8);

            // Top curve
            let mut path_points = Vec::new();
            let steps = 10;
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let p = lerp_quadratic(left, top_ctrl, right, t);
                path_points.push(p);
            }
            // Bottom curve
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let p = lerp_quadratic(right, bot_ctrl, left, t);
                path_points.push(p);
            }
            painter.add(egui::Shape::line(path_points, stroke));

            // Pupil
            painter.circle_filled(center, 2.5, color);
        }
        Icon::EyeClosed => {
            let w = 8.0;
            let h = 4.5;
            let left = center - egui::vec2(w, 0.0);
            let right = center + egui::vec2(w, 0.0);
            let top_ctrl = center - egui::vec2(0.0, h * 1.8);

            let mut path_points = Vec::new();
            let steps = 10;
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let p = lerp_quadratic(left, top_ctrl, right, t);
                path_points.push(p);
            }
            painter.add(egui::Shape::line(path_points, stroke));

            // Lashes (3 small lines)
            let lash_y = center.y - 1.0;
            painter.line_segment(
                [egui::pos2(center.x, lash_y), egui::pos2(center.x, lash_y + 3.0)],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(center.x - 4.0, lash_y - 1.0), egui::pos2(center.x - 5.5, lash_y + 2.0)],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(center.x + 4.0, lash_y - 1.0), egui::pos2(center.x + 5.5, lash_y + 2.0)],
                stroke,
            );
        }
        Icon::Microphone => {
            // Mic Body (Capsule)
            let w = 5.0;
            let h = 9.0;
            let top = center - egui::vec2(0.0, h / 2.0 + 1.0);
            let bot = center + egui::vec2(0.0, h / 2.0 - 2.0);

            painter.rect_stroke(
                egui::Rect::from_min_max(top - egui::vec2(w / 2.0, 0.0), bot + egui::vec2(w / 2.0, 0.0)),
                w / 2.0, // Full rounding = Capsule
                stroke,
            );

            // Stand (Smooth U-Curve)
            let u_start = egui::pos2(center.x - 3.5, center.y);
            let u_ctrl = egui::pos2(center.x, center.y + 7.0);
            let u_end = egui::pos2(center.x + 3.5, center.y);

            let mut u_path = Vec::new();
            for i in 0..=10 {
                let t = i as f32 / 10.0;
                u_path.push(lerp_quadratic(u_start, u_ctrl, u_end, t));
            }
            painter.add(egui::Shape::line(u_path, stroke));

            // Base Stem
            painter.line_segment(
                [egui::pos2(center.x, center.y + 3.5), egui::pos2(center.x, center.y + 7.0)],
                stroke,
            );
            // Base Foot
            painter.line_segment(
                [egui::pos2(center.x - 3.0, center.y + 7.0), egui::pos2(center.x + 3.0, center.y + 7.0)],
                stroke,
            );
        }
        Icon::Image => {
            let img_rect = rect.shrink(3.0);
            painter.rect_stroke(img_rect, 2.0, stroke);
            // Mountains (Clean peaks)
            let p1 = img_rect.left_bottom() - egui::vec2(-1.5, 1.5);
            let p2 = img_rect.left_bottom() + egui::vec2(4.0, -7.0);
            let p3 = img_rect.left_bottom() + egui::vec2(7.0, -3.0);
            let p4 = img_rect.left_bottom() + egui::vec2(10.0, -8.0);
            let p5 = img_rect.right_bottom() - egui::vec2(0.5, 1.5);

            painter.add(egui::Shape::line(vec![p1, p2, p3, p4, p5], stroke));

            // Sun (filled small dot)
            painter.circle_filled(img_rect.left_top() + egui::vec2(4.0, 4.0), 1.5, color);
        }
        Icon::Video => {
            // Modern Cinema Camera
            // Main body
            let w = 11.0;
            let h = 7.5;
            let cam_rect = egui::Rect::from_center_size(center - egui::vec2(1.5, 0.0), egui::vec2(w, h));
            painter.rect_stroke(cam_rect, 2.0, stroke);

            // Trapezoid lens
            let lens_start = cam_rect.right_center();
            let lens_top = lens_start + egui::vec2(4.0, -3.0);
            let lens_bot = lens_start + egui::vec2(4.0, 3.0);

            painter.add(egui::Shape::closed_line(vec![lens_start, lens_top, lens_bot], stroke));

            // Tape reels (decoration on top)
            painter.circle_stroke(cam_rect.center_top() - egui::vec2(2.5, 0.5), 1.5, stroke);
            painter.circle_stroke(cam_rect.center_top() + egui::vec2(2.5, -0.5), 1.5, stroke);
        }
        Icon::Delete => {
            // Elegant small X
            let r = 3.5; // Radius from center (Total size ~7px)
            let p1 = center - egui::vec2(r, r);
            let p2 = center + egui::vec2(r, r);
            let p3 = center + egui::vec2(-r, r);
            let p4 = center - egui::vec2(-r, r);
            painter.line_segment([p1, p2], stroke);
            painter.line_segment([p3, p4], stroke);
        }
        Icon::Info => {
            // Geometric 'i' - No fonts allowed!

            // 1. Circle Border
            painter.circle_stroke(center, 7.0, stroke);

            // 2. The Dot (Filled circle)
            painter.circle_filled(center - egui::vec2(0.0, 2.5), 1.2, color);

            // 3. The Stem (Rounded Line)
            painter.rect_filled(
                egui::Rect::from_center_size(center + egui::vec2(0.0, 2.0), egui::vec2(1.5, 4.5)),
                0.5,
                color,
            );
        }
        Icon::Statistics => {
            // Bar chart: 3 vertical bars of increasing height
            let bar_width = 1.8;
            let spacing = 2.0;
            let base_y = center.y + 5.0;
            
            // Left bar (short)
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(center.x - spacing - bar_width, base_y - 3.0),
                    egui::pos2(center.x - spacing, base_y),
                ),
                0.3,
                color,
            );
            
            // Middle bar (medium)
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(center.x - bar_width / 2.0, base_y - 5.0),
                    egui::pos2(center.x + bar_width / 2.0, base_y),
                ),
                0.3,
                color,
            );
            
            // Right bar (tall)
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(center.x + spacing, base_y - 7.0),
                    egui::pos2(center.x + spacing + bar_width, base_y),
                ),
                0.3,
                color,
            );
        }
    }

    response
}

/// Draw just the icon shape (static, for labels)
pub fn draw_icon_static(ui: &mut egui::Ui, icon: Icon) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());

    let center = rect.center();
    let painter = ui.painter();
    let color = ui.visuals().text_color();
    let stroke = egui::Stroke::new(1.3, color);

    match icon {
        Icon::Image => {
            let img_rect = rect.shrink(2.0);
            painter.rect_stroke(img_rect, 1.0, stroke);
            let p1 = img_rect.left_bottom() - egui::vec2(-1.0, 1.0);
            let p2 = img_rect.left_bottom() + egui::vec2(3.0, -5.0);
            let p3 = img_rect.left_bottom() + egui::vec2(6.0, -2.0);
            let p4 = img_rect.left_bottom() + egui::vec2(9.0, -6.0);
            let p5 = img_rect.right_bottom() - egui::vec2(0.0, 1.0);
            painter.add(egui::Shape::line(vec![p1, p2, p3, p4, p5], stroke));
            painter.circle_filled(img_rect.left_top() + egui::vec2(3.0, 3.0), 1.0, color);
        }
        Icon::Microphone => {
            // Scale down version of the button mic
            let w = 4.0;
            let h = 7.0;
            let top = center - egui::vec2(0.0, h / 2.0 + 1.0);
            let bot = center + egui::vec2(0.0, h / 2.0 - 1.0);
            painter.rect_stroke(
                egui::Rect::from_min_max(top - egui::vec2(w / 2.0, 0.0), bot + egui::vec2(w / 2.0, 0.0)),
                w / 2.0,
                stroke,
            );

            // Stand
            let u_start = egui::pos2(center.x - 3.0, center.y);
            let u_ctrl = egui::pos2(center.x, center.y + 6.0);
            let u_end = egui::pos2(center.x + 3.0, center.y);
            let mut u_path = Vec::new();
            for i in 0..=8 {
                u_path.push(lerp_quadratic(u_start, u_ctrl, u_end, i as f32 / 8.0));
            }
            painter.add(egui::Shape::line(u_path, stroke));

            painter.line_segment(
                [egui::pos2(center.x, center.y + 3.0), egui::pos2(center.x, center.y + 6.0)],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(center.x - 2.5, center.y + 6.0), egui::pos2(center.x + 2.5, center.y + 6.0)],
                stroke,
            );
        }
        Icon::Video => {
            let cam_rect = egui::Rect::from_center_size(center - egui::vec2(1.5, 0.0), egui::vec2(9.0, 6.0));
            painter.rect_stroke(cam_rect, 1.5, stroke);
            let p1 = cam_rect.right_center();
            let p2 = p1 + egui::vec2(3.5, -2.5);
            let p3 = p1 + egui::vec2(3.5, 2.5);
            painter.add(egui::Shape::closed_line(vec![p1, p2, p3], stroke));
        }
        Icon::Settings => {
            painter.circle_stroke(center, 3.5, stroke);
            for i in 0..6 {
                let angle = (i as f32 * 60.0).to_radians();
                let dir = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [center + dir * 3.5, center + dir * 6.0],
                    egui::Stroke::new(2.0, color),
                );
            }
        }
        Icon::Info => {
            // Geometric 'i' - No fonts allowed!
            painter.circle_stroke(center, 5.5, stroke);
            painter.circle_filled(center - egui::vec2(0.0, 2.0), 0.9, color);
            painter.rect_filled(
                egui::Rect::from_center_size(center + egui::vec2(0.0, 1.5), egui::vec2(1.2, 3.5)),
                0.4,
                color,
            );
        }
        Icon::Statistics => {
            // Bar chart (scaled down for static)
            let bar_width = 1.4;
            let spacing = 1.6;
            let base_y = center.y + 3.5;
            
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(center.x - spacing - bar_width, base_y - 2.0),
                    egui::pos2(center.x - spacing, base_y),
                ),
                0.2,
                color,
            );
            
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(center.x - bar_width / 2.0, base_y - 3.5),
                    egui::pos2(center.x + bar_width / 2.0, base_y),
                ),
                0.2,
                color,
            );
            
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(center.x + spacing, base_y - 5.0),
                    egui::pos2(center.x + spacing + bar_width, base_y),
                ),
                0.2,
                color,
            );
        }
        _ => {}
    }
}
