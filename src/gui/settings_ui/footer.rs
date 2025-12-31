use eframe::egui;
use crate::gui::locale::LocaleText;
use crate::gui::icons::{Icon, paint_icon};
use egui::text::{LayoutJob, TextFormat};

pub fn render_footer(
    ui: &mut egui::Ui, 
    text: &LocaleText,
    current_tip: String,
    tip_alpha: f32,
    show_modal: &mut bool
) {
    ui.horizontal(|ui| {
        // 1. Left Side: Admin Status
        // Use a fixed width container for left side to ensure stability
        ui.allocate_ui(egui::vec2(180.0, ui.available_height()), |ui| {
            ui.horizontal_centered(|ui| {
                let is_admin = cfg!(target_os = "windows") && crate::gui::utils::is_running_as_admin();
                let footer_text = if is_admin {
                    egui::RichText::new(text.footer_admin_running)
                         .size(11.0)
                         .color(egui::Color32::from_rgb(34, 139, 34))
                } else {
                    egui::RichText::new(text.footer_admin_text)
                         .size(11.0)
                         .color(ui.visuals().weak_text_color())
                };
                ui.label(footer_text);
            });
        });

        // 2. Right Side: Version
        // We use with_layout to pack from right, but we need to reserve space first 
        // or egui might push the center content over it.
        // A better approach in horizontal layout: Left -> Expanded Center -> Right.
        
        // 3. Center: Tips (Takes available space)
        let version_text = format!("{} v{}", text.footer_version, env!("CARGO_PKG_VERSION"));
        let version_galley = ui.painter().layout_no_wrap(
            version_text.clone(), 
            egui::FontId::proportional(11.0), 
            ui.visuals().weak_text_color()
        );
        let version_width = version_galley.rect.width() + 10.0;

        // Allocate center space: Total - Left - Right
        let available_w = ui.available_width() - version_width;
        
        ui.allocate_ui(egui::vec2(available_w, ui.available_height()), |ui| {
            ui.vertical_centered(|ui| {
                let tip_color = ui.visuals().text_color().linear_multiply(tip_alpha);
                let icon_color = egui::Color32::from_rgba_unmultiplied(255, 200, 50, (tip_alpha * 255.0) as u8); // Yellow/gold color for lightbulb
                
                // First, calculate text width to properly center everything
                let icon_size = 14.0;
                let icon_spacing = 4.0;
                
                // Format tip with bold text
                let is_dark_mode = ui.visuals().dark_mode;
                let layout_job = format_footer_tip(&current_tip, tip_color, is_dark_mode, tip_alpha);
                let text_galley = ui.painter().layout_job(layout_job);
                let total_width = icon_size + icon_spacing + text_galley.rect.width();
                
                // Allocate space for icon + text centered
                let (response, painter) = ui.allocate_painter(
                    egui::vec2(total_width + 8.0, ui.available_height().max(18.0)), 
                    egui::Sense::click()
                );
                let rect = response.rect;
                
                // Draw lightbulb icon on the left
                let icon_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left(), rect.center().y - icon_size / 2.0),
                    egui::vec2(icon_size, icon_size)
                );
                paint_icon(&painter, icon_rect, Icon::Lightbulb, icon_color);
                
                // Draw text to the right of icon
                let text_pos = egui::pos2(
                    icon_rect.right() + icon_spacing,
                    rect.center().y - text_galley.rect.height() / 2.0
                );
                painter.galley(text_pos, text_galley, egui::Color32::WHITE);

                if response.on_hover_text(text.tips_click_hint).clicked() {
                    *show_modal = true;
                }
            });
        });

        // 4. Draw Version on the far right
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(version_text).size(11.0).color(ui.visuals().weak_text_color()));
        });
    });
}

// Helper function to format footer tip with bold text
fn format_footer_tip(text: &str, base_color: egui::Color32, is_dark_mode: bool, alpha_factor: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    
    // Color scheme for bold text
    let bold_color = if is_dark_mode {
        egui::Color32::from_rgb(150, 200, 255) // Soft cyan for dark mode
    } else {
        egui::Color32::from_rgb(40, 100, 180) // Dark blue for light mode
    };
    
    // Apply alpha to colors
    let regular_color = egui::Color32::from_rgba_unmultiplied(
        base_color.r(),
        base_color.g(),
        base_color.b(),
        (base_color.a() as f32 * alpha_factor) as u8,
    );
    
    let bold_color_with_alpha = egui::Color32::from_rgba_unmultiplied(
        bold_color.r(),
        bold_color.g(),
        bold_color.b(),
        (255.0 * alpha_factor) as u8,
    );
    
    // Create text format
    let mut text_format = TextFormat::default();
    text_format.font_id = egui::FontId::proportional(11.0);
    text_format.color = regular_color;
    
    // Parse text for **bold** markers
    let mut current_text = String::new();
    let mut chars = text.chars().peekable();
    let mut is_bold = false;
    
    while let Some(ch) = chars.next() {
        if ch == '*' && chars.peek() == Some(&'*') {
            // Found ** marker
            chars.next(); // consume second *
            
            if !current_text.is_empty() {
                // Append accumulated text
                let mut fmt = text_format.clone();
                if is_bold {
                    fmt.color = bold_color_with_alpha;
                }
                job.append(&current_text, 0.0, fmt);
                current_text.clear();
            }
            
            is_bold = !is_bold;
        } else {
            current_text.push(ch);
        }
    }
    
    // Append remaining text
    if !current_text.is_empty() {
        let mut fmt = text_format.clone();
        if is_bold {
            fmt.color = bold_color_with_alpha;
        }
        job.append(&current_text, 0.0, fmt);
    }
    
    job
}
