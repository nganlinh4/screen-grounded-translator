// Preset Wheel HTML - Apple Watch fisheye with center-out ripple animation

use crate::config::Preset;
use crate::gui::settings_ui::get_localized_preset_name;

pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Calculate balanced row distribution using ratio-based "square-squeeze" algorithm
/// Pills are ~3x wider than tall, so we use sqrt(n/2) for columns to get more rows than columns
/// This creates visually square/rectangular clumps: 5→[3,2], 10→[4,3,3], 25→[5,5,5,5,5]
fn calculate_row_distribution(n: usize) -> Vec<usize> {
    if n == 0 {
        return vec![];
    }

    if n == 1 {
        return vec![1];
    }

    // Ratio-based: pills are ~130px wide, ~40px tall (ratio ~3:1)
    // For a visually square clump, use fewer columns than pure sqrt would give
    // cols = ceil(sqrt(n / squish_factor)) where squish_factor accounts for aspect ratio
    let squish_factor = 1.5; // Balance between rows and columns
    let cols = ((n as f64 / squish_factor).sqrt().ceil() as usize).max(1);

    // Calculate number of rows needed
    let num_rows = (n + cols - 1) / cols;

    // Calculate base items per row and remainder
    let base = n / num_rows;
    let remainder = n % num_rows;

    // Distribute evenly: first 'remainder' rows get base+1
    let mut rows = Vec::with_capacity(num_rows);
    for i in 0..num_rows {
        if i < remainder {
            rows.push(base + 1);
        } else {
            rows.push(base);
        }
    }

    rows
}

/// Helper to generate just the items HTML (used for dynamic updates)
/// Uses fixed row layout to prevent reflow during animations
pub fn generate_items_html(presets: &[(usize, Preset)], ui_lang: &str) -> String {
    let n = presets.len();
    let row_distribution = calculate_row_distribution(n);

    let mut html = String::new();
    let mut item_idx = 0;

    for (row_idx, &items_in_row) in row_distribution.iter().enumerate() {
        html.push_str(&format!(
            r#"<div class="preset-row" data-row="{}">"#,
            row_idx
        ));

        for _ in 0..items_in_row {
            if item_idx < presets.len() {
                let (idx, preset) = &presets[item_idx];
                let name = escape_html(&get_localized_preset_name(&preset.id, ui_lang));
                let color_class = format!("color-{}", item_idx % 12);
                html.push_str(&format!(
                    r#"<div class="preset-item {}" data-idx="{}" data-item="{}" onclick="select({})">{}</div>"#,
                    color_class, idx, item_idx, idx, name
                ));
                item_idx += 1;
            }
        }

        html.push_str("</div>");
    }

    html
}

/// Returns the static HTML skeleton with CSS and JS (loaded once)
pub fn get_wheel_template(is_dark: bool) -> String {
    let font_css = crate::overlay::html_components::font_manager::get_font_css();
    let css = generate_css(is_dark);
    let js = get_js();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style id="font-style">
{font_css}
</style>
<style id="theme-style">
{css}
</style>
</head>
<body>
<div class="container">
    <div class="dismiss-btn" onclick="dismiss()">CANCEL</div>
    <div class="presets-grid" id="grid">
        <!-- Items will be injected here -->
    </div>
</div>
<script>
{js}
</script>
</body>
</html>"#
    )
}

/// Generate CSS for the wheel with light/dark theme support
pub fn generate_css(is_dark: bool) -> String {
    // Theme-specific colors
    let (
        text_color,
        dismiss_bg,
        dismiss_border,
        dismiss_color,
        dismiss_hover_bg,
        dismiss_hover_border,
        item_border,
        item_hover_border,
        item_shadow,
    ) = if is_dark {
        (
            "#ffffff",
            "rgba(20, 20, 25, 0.75)",
            "rgba(255, 255, 255, 0.12)",
            "rgba(255, 180, 180, 0.85)",
            "rgba(60, 30, 30, 0.85)",
            "rgba(255, 150, 150, 0.4)",
            "rgba(255, 255, 255, 0.15)",
            "rgba(255, 255, 255, 0.5)",
            "0 5px 18px rgba(0, 0, 0, 0.35)",
        )
    } else {
        // Light mode colors
        (
            "#222222",
            "rgba(255, 255, 255, 0.85)",
            "rgba(0, 0, 0, 0.1)",
            "rgba(180, 60, 60, 0.9)",
            "rgba(255, 220, 220, 0.95)",
            "rgba(200, 100, 100, 0.4)",
            "rgba(0, 0, 0, 0.12)",
            "rgba(0, 0, 0, 0.3)",
            "0 5px 18px rgba(0, 0, 0, 0.15)",
        )
    };

    // Color palettes for dark and light modes
    let color_palette = if is_dark {
        // Dark mode - Deep Glass (Rich, saturated, premium)
        r#"
.color-0  { background: rgba(30, 60, 110, 0.85); border-color: rgba(100, 150, 255, 0.3); } /* Deep Blue */
.color-1  { background: rgba(35, 80, 45, 0.85);  border-color: rgba(100, 255, 120, 0.3); } /* Deep Green */
.color-2  { background: rgba(90, 30, 35, 0.85);  border-color: rgba(255, 100, 110, 0.3); } /* Deep Red */
.color-3  { background: rgba(70, 35, 90, 0.85);  border-color: rgba(200, 120, 255, 0.3); } /* Deep Purple */
.color-4  { background: rgba(90, 60, 20, 0.85);  border-color: rgba(255, 180, 80, 0.3); }  /* Deep Orange */
.color-5  { background: rgba(20, 75, 85, 0.85);  border-color: rgba(80, 230, 255, 0.3); }  /* Deep Teal */
.color-6  { background: rgba(85, 30, 85, 0.85);  border-color: rgba(255, 100, 255, 0.3); } /* Deep Magenta */
.color-7  { background: rgba(30, 70, 100, 0.85); border-color: rgba(100, 200, 255, 0.3); } /* Deep Sky */
.color-8  { background: rgba(65, 80, 20, 0.85);  border-color: rgba(200, 255, 80, 0.3); }  /* Deep Lime */
.color-9  { background: rgba(90, 20, 60, 0.85);  border-color: rgba(255, 80, 150, 0.3); }  /* Deep Pink */
.color-10 { background: rgba(20, 80, 70, 0.85);  border-color: rgba(80, 255, 200, 0.3); }  /* Deep Cyan */
.color-11 { background: rgba(90, 50, 30, 0.85);  border-color: rgba(255, 140, 80, 0.3); }  /* Deep Amber */

.color-0.hovered  { background: rgba(50, 100, 180, 0.95); box-shadow: 0 0 15px rgba(60, 120, 255, 0.4); }
.color-1.hovered  { background: rgba(50, 140, 70, 0.95);  box-shadow: 0 0 15px rgba(80, 255, 100, 0.4); }
.color-2.hovered  { background: rgba(160, 50, 60, 0.95);  box-shadow: 0 0 15px rgba(255, 80, 90, 0.4); }
.color-3.hovered  { background: rgba(120, 60, 160, 0.95); box-shadow: 0 0 15px rgba(180, 100, 255, 0.4); }
.color-4.hovered  { background: rgba(160, 100, 40, 0.95); box-shadow: 0 0 15px rgba(255, 160, 60, 0.4); }
.color-5.hovered  { background: rgba(40, 130, 150, 0.95); box-shadow: 0 0 15px rgba(60, 220, 255, 0.4); }
.color-6.hovered  { background: rgba(150, 50, 150, 0.95); box-shadow: 0 0 15px rgba(255, 80, 255, 0.4); }
.color-7.hovered  { background: rgba(50, 120, 170, 0.95); box-shadow: 0 0 15px rgba(80, 180, 255, 0.4); }
.color-8.hovered  { background: rgba(110, 140, 40, 0.95); box-shadow: 0 0 15px rgba(180, 255, 60, 0.4); }
.color-9.hovered  { background: rgba(160, 40, 100, 0.95); box-shadow: 0 0 15px rgba(255, 60, 140, 0.4); }
.color-10.hovered { background: rgba(40, 140, 120, 0.95); box-shadow: 0 0 15px rgba(60, 255, 200, 0.4); }
.color-11.hovered { background: rgba(160, 80, 50, 0.95);  box-shadow: 0 0 15px rgba(255, 120, 60, 0.4); }"#
    } else {
        // Light mode - softer pastel colors
        r#"
.color-0  { background: rgba(200, 220, 255, 0.95); }
.color-1  { background: rgba(200, 235, 200, 0.95); }
.color-2  { background: rgba(255, 210, 210, 0.95); }
.color-3  { background: rgba(230, 210, 255, 0.95); }
.color-4  { background: rgba(255, 230, 200, 0.95); }
.color-5  { background: rgba(200, 240, 240, 0.95); }
.color-6  { background: rgba(240, 210, 245, 0.95); }
.color-7  { background: rgba(210, 230, 250, 0.95); }
.color-8  { background: rgba(235, 235, 200, 0.95); }
.color-9  { background: rgba(255, 210, 235, 0.95); }
.color-10 { background: rgba(200, 245, 240, 0.95); }
.color-11 { background: rgba(255, 225, 210, 0.95); }

.color-0.hovered  { background: rgba(130, 180, 255, 0.98); }
.color-1.hovered  { background: rgba(130, 200, 130, 0.98); }
.color-2.hovered  { background: rgba(255, 150, 150, 0.98); }
.color-3.hovered  { background: rgba(190, 150, 255, 0.98); }
.color-4.hovered  { background: rgba(255, 190, 120, 0.98); }
.color-5.hovered  { background: rgba(100, 220, 220, 0.98); }
.color-6.hovered  { background: rgba(220, 150, 230, 0.98); }
.color-7.hovered  { background: rgba(140, 190, 255, 0.98); }
.color-8.hovered  { background: rgba(200, 200, 120, 0.98); }
.color-9.hovered  { background: rgba(255, 150, 200, 0.98); }
.color-10.hovered { background: rgba(80, 210, 200, 0.98); }
.color-11.hovered { background: rgba(255, 170, 130, 0.98); }"#
    };

    format!(
        r#"
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
html, body {{
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
    font-family: 'Google Sans Flex', 'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif;
    font-variation-settings: 'wght' 500, 'wdth' 100, 'ROND' 100;
    user-select: none;
    color: {text_color};
}}

.container {{
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 100%;
    padding: 40px;
    gap: 10px;
}}

/* Cancel button - frosted glass, distinct from colorful presets */
.dismiss-btn {{
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 12px 36px;
    margin-bottom: 16px;
    background: {dismiss_bg};
    backdrop-filter: blur(16px);
    border: 1px solid {dismiss_border};
    border-radius: 24px;
    cursor: pointer;
    font-size: 14px;
    letter-spacing: 3px;
    text-transform: uppercase;
    font-variation-settings: 'wght' 600, 'wdth' 125, 'ROND' 100;
    color: {dismiss_color};
    
    opacity: 0;
    transform: scale(0.5);
    transition: 
        transform 0.2s cubic-bezier(0.22, 1, 0.36, 1),
        opacity 0.15s ease-out,
        background 0.1s ease,
        border-color 0.1s ease,
        box-shadow 0.1s ease,
        color 0.1s ease,
        font-variation-settings 0.15s ease;
}}

.dismiss-btn.visible {{
    opacity: 1;
    transform: scale(1);
}}

.dismiss-btn:hover {{
    background: {dismiss_hover_bg};
    border-color: {dismiss_hover_border};
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.2);
    color: {text_color};
    font-variation-settings: 'wght' 700, 'wdth' 105, 'ROND' 100;
}}

.dismiss-btn:active {{
    transform: scale(0.92) !important;
}}

/* Fixed row-based layout - prevents reflow during animations */
.presets-grid {{
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 20px;
}}

/* Each row is a flex container with fixed item count */
.preset-row {{
    display: flex;
    flex-direction: row;
    justify-content: center;
    align-items: center;
    gap: 10px;
    min-height: 40px;
}}

.preset-item {{
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 9px 14px;
    min-width: 85px;
    backdrop-filter: blur(12px);
    border: 1px solid {item_border};
    border-radius: 15px;
    cursor: pointer;
    font-size: 12px;
    white-space: nowrap;
    letter-spacing: 0;
    color: {text_color};
    
    opacity: 0;
    transform: scale(0.8);
    
    transition: 
        transform 0.15s cubic-bezier(0.22, 1, 0.36, 1),
        opacity 0.15s ease-out,
        background 0.1s ease,
        box-shadow 0.1s ease,
        border-color 0.1s ease,
        font-variation-settings 0.1s ease,
        letter-spacing 0.1s ease;
}}

.preset-item.visible {{
    opacity: 1;
    transform: scale(1);
}}

{color_palette}

.preset-item.hovered {{
    border-color: {item_hover_border};
    box-shadow: {item_shadow};
    font-variation-settings: 'wght' 650, 'wdth' 90, 'ROND' 100;
    letter-spacing: 0.5px;
}}

.preset-item:active {{
    transform: scale(0.88) !important;
    transition: transform 0.05s ease !important;
}}
"#,
        text_color = text_color,
        dismiss_bg = dismiss_bg,
        dismiss_border = dismiss_border,
        dismiss_color = dismiss_color,
        dismiss_hover_bg = dismiss_hover_bg,
        dismiss_hover_border = dismiss_hover_border,
        item_border = item_border,
        item_hover_border = item_hover_border,
        item_shadow = item_shadow,
        color_palette = color_palette
    )
}

fn get_js() -> &'static str {
    r#"
function select(idx) {
    window.ipc.postMessage('select:' + idx);
}

function dismiss() {
    window.ipc.postMessage('dismiss');
}

// === Apple Watch Fisheye Effect ===
const grid = document.getElementById('grid');
let items = []; // Will be updated on content load
const dismissBtn = document.querySelector('.dismiss-btn');

// Tuned constants - NO shrinking, only scale up hovered item
const MAX_SCALE = 1.10;
const MIN_SCALE = 1.0;
const EFFECT_RADIUS = 80;
const BASE_WEIGHT = 500;     
const MAX_WEIGHT = 650;      
const BASE_WIDTH = 100;      
const MAX_WIDTH = 104;       

let animationFrame = null;
let mouseX = -1000;
let mouseY = -1000;
let isMouseInGrid = false;

// Cache item positions to avoid getBoundingClientRect returning scaled positions
// This fixes the cursor position vs hover mismatch issue
let itemCenters = new Map();

function cacheItemPositions() {
    // Reset all items to scale(1) to get accurate positions
    items.forEach(item => {
        item.style.transform = 'scale(1)';
    });
    
    // Cache the original center positions (before any scaling)
    itemCenters.clear();
    items.forEach(item => {
        const rect = item.getBoundingClientRect();
        itemCenters.set(item, {
            x: rect.left + rect.width / 2,
            y: rect.top + rect.height / 2
        });
    });
}

function getItemCenter(item) {
    // Use cached position if available
    const cached = itemCenters.get(item);
    if (cached) return cached;
    
    // Fallback to live calculation
    const rect = item.getBoundingClientRect();
    return {
        x: rect.left + rect.width / 2,
        y: rect.top + rect.height / 2
    };
}

function isMouseInRect(rect) {
    return mouseX >= rect.left && mouseX <= rect.right && 
           mouseY >= rect.top && mouseY <= rect.bottom;
}

function updateFisheye() {
    items.forEach(item => {
        if (!item.classList.contains('visible')) return;
        
        // For fisheye scaling, use cached centers
        const center = getItemCenter(item);
        const dx = mouseX - center.x;
        const dy = mouseY - center.y;
        const distance = Math.sqrt(dx * dx + dy * dy);
        
        let influence = isMouseInGrid ? Math.max(0, 1 - distance / EFFECT_RADIUS) : 0;
        influence = influence * influence * (3 - 2 * influence); // smoothstep
        
        // Only scale UP - never below 1.0
        const scale = MIN_SCALE + (MAX_SCALE - MIN_SCALE) * influence;
        
        // For hover detection, check if mouse is actually inside this pill
        const rect = item.getBoundingClientRect();
        const isHovered = isMouseInGrid && isMouseInRect(rect);
        
        if (isHovered) {
            item.classList.add('hovered');
            // Let CSS handle font styling for hovered items
            item.style.fontVariationSettings = '';
            item.style.letterSpacing = '';
        } else {
            item.classList.remove('hovered');
            // Apply fisheye font effect for non-hovered items
            const weight = BASE_WEIGHT + (MAX_WEIGHT - BASE_WEIGHT) * influence;
            const width = BASE_WIDTH + (MAX_WIDTH - BASE_WIDTH) * influence;
            item.style.fontVariationSettings = `'wght' ${weight.toFixed(0)}, 'wdth' ${width.toFixed(0)}, 'ROND' 100`;
            item.style.letterSpacing = '0';
        }
        
        item.style.transform = `scale(${scale.toFixed(3)})`;
    });
}

function onMouseMove(e) {
    mouseX = e.clientX;
    mouseY = e.clientY;
    
    if (!animationFrame) {
        animationFrame = requestAnimationFrame(() => {
            updateFisheye();
            animationFrame = null;
        });
    }
}

function onMouseEnter() {
    isMouseInGrid = true;
}

function onMouseLeave() {
    isMouseInGrid = false;
    mouseX = -1000;
    mouseY = -1000;
    
    items.forEach(item => {
        item.style.transform = 'scale(1)';
        item.style.fontVariationSettings = `'wght' ${BASE_WEIGHT}, 'wdth' ${BASE_WIDTH}, 'ROND' 100`;
        item.classList.remove('hovered');
    });
}

grid.addEventListener('mousemove', onMouseMove);
grid.addEventListener('mouseenter', onMouseEnter);
grid.addEventListener('mouseleave', onMouseLeave);

document.querySelector('.container').addEventListener('mousemove', (e) => {
    const gridRect = grid.getBoundingClientRect();
    const padding = 35;
    if (e.clientX >= gridRect.left - padding && 
        e.clientX <= gridRect.right + padding &&
        e.clientY >= gridRect.top - padding && 
        e.clientY <= gridRect.bottom + padding) {
        onMouseMove(e);
    }
});

// === Animate in from CENTER outward (ripple effect) ===
function animateIn() {
    // Get window center (cursor should be near center when wheel opens)
    const windowCenterX = window.innerWidth / 2;
    const windowCenterY = window.innerHeight / 2;
    
    // Calculate distance of each item from center
    const itemsWithDistance = items.map(item => {
        const rect = item.getBoundingClientRect();
        const itemCenterX = rect.left + rect.width / 2;
        const itemCenterY = rect.top + rect.height / 2;
        const dx = itemCenterX - windowCenterX;
        const dy = itemCenterY - windowCenterY;
        const distance = Math.sqrt(dx * dx + dy * dy);
        return { item, distance };
    });
    
    // Sort by distance (closest to center first)
    itemsWithDistance.sort((a, b) => a.distance - b.distance);

    // Dismiss button first (it's at top center)
    setTimeout(() => dismissBtn.classList.add('visible'), 0);
    
    // Then items in ripple order from center out - fast stagger
    itemsWithDistance.forEach(({ item }, i) => {
        setTimeout(() => item.classList.add('visible'), i * 12);
    });
    
    // Cache positions AFTER animation completes (when items are at scale(1))
    // Wait for all items to animate in + some buffer
    const totalAnimationTime = itemsWithDistance.length * 12 + 150;
    setTimeout(() => cacheItemPositions(), totalAnimationTime);
}

// Function called by Rust to update content and trigger animation
window.updateContent = function(itemsHtml, dismissLabel) {
    grid.innerHTML = itemsHtml;
    dismissBtn.innerText = dismissLabel;
    
    // Re-query items - now nested in .preset-row divs
    items = Array.from(document.querySelectorAll('.preset-item'));
    
    // Clear cached positions
    itemCenters.clear();
    
    // Reset visibility state BEFORE window becomes visible
    dismissBtn.classList.remove('visible');
    items.forEach(item => item.classList.remove('visible'));
    
    // Notify Rust we are ready to be visible
    setTimeout(() => {
        window.ipc.postMessage('ready_to_show');
        // Start animation after a tiny delay to ensure window is shown
        setTimeout(() => requestAnimationFrame(animateIn), 16);
        
        // Fallback: force visible after 300ms if animation didn't work
        setTimeout(() => {
            dismissBtn.classList.add('visible');
            items.forEach(item => item.classList.add('visible'));
        }, 300);
    }, 0);
};

document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') dismiss();
});
    "#
}
