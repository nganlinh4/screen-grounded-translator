// --- ICON PAINTER MODULE ---
// Programmatic vector icons using egui Painter
// Eliminates dependency on font files, saves ~1-2MB in binary size

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
}

/// Draw an interactive icon button with hover effects
pub fn icon_button(ui: &mut egui::Ui, icon: Icon) -> egui::Response {
    let size = egui::vec2(18.0, 18.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    // Draw Button Background (Hover Effect)
    if response.hovered() {
        ui.painter().rect_filled(
            rect.expand(2.0),
            4.0,
            ui.visuals().widgets.hovered.bg_fill,
        );
    }

    // Setup Painter
    let painter = ui.painter();
    let color = if response.hovered() {
        ui.visuals().widgets.hovered.fg_stroke.color
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    let stroke = egui::Stroke::new(1.5, color);
    let center = rect.center();

    // Draw Specific Icon
    match icon {
        Icon::Settings => {
            // Gear
            painter.circle_stroke(center, 5.0, stroke);
            for i in 0..8 {
                let angle = (i as f32 * 45.0).to_radians();
                let dir = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [center + dir * 5.0, center + dir * 8.5],
                    egui::Stroke::new(2.5, color),
                );
            }
        }
        Icon::Moon => {
            // Crescent
            let c = center;
            let r = 7.0;
            // Draw full circle
            painter.circle_filled(c, r, color);
            // Mask with background color circle shifted slightly
            painter.circle_filled(
                c + egui::vec2(3.0, -2.0),
                r * 0.9,
                ui.visuals().panel_fill,
            );
        }
        Icon::Sun => {
            painter.circle_stroke(center, 4.0, stroke);
            for i in 0..8 {
                let angle = (i as f32 * 45.0).to_radians();
                let dir = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [center + dir * 6.0, center + dir * 9.0],
                    egui::Stroke::new(1.2, color),
                );
            }
        }
        Icon::EyeOpen => {
            // Eye shape
            let left = rect.left_center() + egui::vec2(1.0, 0.0);
            let right = rect.right_center() - egui::vec2(1.0, 0.0);
            let top = rect.center_top() + egui::vec2(0.0, 3.0);
            let bottom = rect.center_bottom() - egui::vec2(0.0, 3.0);

            let path = vec![left, top, right, bottom, left];
            painter.add(egui::Shape::closed_line(path, stroke));

            // Pupil
            painter.circle_filled(center, 2.5, color);
        }
        Icon::EyeClosed => {
            // Closed eye with lash
            let left = rect.left_center() + egui::vec2(1.0, 0.0);
            let right = rect.right_center() - egui::vec2(1.0, 0.0);

            // Lash line (straight line)
            painter.line_segment([left, right], stroke);

            // Diagonal slash
            painter.line_segment(
                [rect.right_top() - egui::vec2(2.0, 1.0), rect.left_bottom() + egui::vec2(2.0, -1.0)],
                egui::Stroke::new(1.2, color),
            );
        }
        Icon::Microphone => {
            // Mic Body (rounded rectangle)
            let w = 5.0;
            let h = 9.0;
            let mic_rect = egui::Rect::from_center_size(center - egui::vec2(0.0, 2.0), egui::vec2(w, h));
            painter.rect_stroke(mic_rect, 2.5, stroke);

            // Stand (U shape)
            let u_rect = mic_rect.expand(2.0);
            painter.line_segment([u_rect.left_center(), u_rect.left_bottom()], stroke);
            painter.line_segment([u_rect.left_bottom(), u_rect.right_bottom()], stroke);
            painter.line_segment([u_rect.right_bottom(), u_rect.right_center()], stroke);

            // Base
            painter.line_segment([center + egui::vec2(0.0, 4.0), center + egui::vec2(0.0, 8.0)], stroke);
            painter.line_segment(
                [center + egui::vec2(-4.0, 8.0), center + egui::vec2(4.0, 8.0)],
                stroke,
            );
        }
        Icon::Image => {
            let img_rect = rect.shrink(2.0);
            painter.rect_stroke(img_rect, 2.0, stroke);

            // Mountain shape
            let p1 = img_rect.left_bottom() - egui::vec2(-2.0, 2.0);
            let p2 = img_rect.left_bottom() + egui::vec2(4.0, -6.0);
            let p3 = img_rect.left_bottom() + egui::vec2(8.0, -2.0);
            let p4 = img_rect.left_bottom() + egui::vec2(12.0, -8.0);
            let p5 = img_rect.right_bottom() - egui::vec2(0.0, 2.0);

            let points = vec![p1, p2, p3, p4, p5];
            painter.add(egui::Shape::line(points, stroke));

            // Sun
            painter.circle_stroke(img_rect.left_top() + egui::vec2(4.0, 4.0), 1.5, stroke);
        }
        Icon::Video => {
            let cam_w = 10.0;
            let cam_h = 8.0;
            let cam_rect = egui::Rect::from_center_size(center - egui::vec2(2.0, 0.0), egui::vec2(cam_w, cam_h));
            painter.rect_stroke(cam_rect, 1.5, stroke);

            // Triangle lens (play button style)
            let p1 = cam_rect.right_center();
            let p2 = p1 + egui::vec2(5.0, -4.0);
            let p3 = p1 + egui::vec2(5.0, 4.0);
            painter.add(egui::Shape::closed_line(vec![p1, p2, p3], stroke));
        }
        Icon::Delete => {
            // X mark
            let p1 = rect.min + egui::vec2(4.0, 4.0);
            let p2 = rect.max - egui::vec2(4.0, 4.0);
            let p3 = egui::pos2(rect.max.x - 4.0, rect.min.y + 4.0);
            let p4 = egui::pos2(rect.min.x + 4.0, rect.max.y - 4.0);
            painter.line_segment([p1, p2], stroke);
            painter.line_segment([p3, p4], stroke);
        }
        Icon::Info => {
            // Circle with 'i'
            painter.circle_stroke(center, 8.0, stroke);
            painter.text(
                center + egui::vec2(0.0, 1.0),
                egui::Align2::CENTER_CENTER,
                "i",
                egui::FontId::monospace(12.0),
                color,
            );
        }
    }

    response
}

/// Draw just the icon shape (static, for labels)
pub fn draw_icon_static(ui: &mut egui::Ui, icon: Icon, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let center = rect.center();
    let painter = ui.painter();
    let color = ui.visuals().text_color();
    let stroke = egui::Stroke::new(1.2, color);

    match icon {
        Icon::Image => {
            let img_rect = rect.shrink(1.0);
            painter.rect_stroke(img_rect, 1.5, stroke);
            let p1 = img_rect.left_bottom() - egui::vec2(-1.0, 1.0);
            let p2 = img_rect.left_bottom() + egui::vec2(3.0, -4.0);
            let p3 = img_rect.left_bottom() + egui::vec2(6.0, -1.0);
            let p4 = img_rect.left_bottom() + egui::vec2(9.0, -5.0);
            let p5 = img_rect.right_bottom() - egui::vec2(0.0, 1.0);
            painter.add(egui::Shape::line(vec![p1, p2, p3, p4, p5], stroke));
        }
        Icon::Microphone => {
            let w = 3.5;
            let h = 6.0;
            let mic_rect = egui::Rect::from_center_size(center - egui::vec2(0.0, 1.5), egui::vec2(w, h));
            painter.rect_stroke(mic_rect, 1.8, stroke);
            painter.line_segment([center + egui::vec2(-2.5, 2.0), center + egui::vec2(0.0, 4.0)], stroke);
            painter.line_segment([center + egui::vec2(2.5, 2.0), center + egui::vec2(0.0, 4.0)], stroke);
            painter.line_segment(
                [center + egui::vec2(0.0, 4.0), center + egui::vec2(0.0, 6.0)],
                stroke,
            );
            painter.line_segment(
                [center + egui::vec2(-2.5, 6.0), center + egui::vec2(2.5, 6.0)],
                stroke,
            );
        }
        Icon::Video => {
            let cam_rect = egui::Rect::from_center_size(center - egui::vec2(1.5, 0.0), egui::vec2(7.0, 5.0));
            painter.rect_stroke(cam_rect, 1.2, stroke);
            let p1 = cam_rect.right_center();
            let p2 = p1 + egui::vec2(3.0, -2.5);
            let p3 = p1 + egui::vec2(3.0, 2.5);
            painter.add(egui::Shape::closed_line(vec![p1, p2, p3], stroke));
        }
        _ => {}
    }
}
