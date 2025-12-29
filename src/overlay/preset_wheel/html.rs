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

/// Helper to generate just the items HTML (used for dynamic updates)
pub fn generate_items_html(presets: &[(usize, Preset)], ui_lang: &str) -> String {
    presets
        .iter()
        .enumerate()
        .map(|(i, (idx, preset))| {
            let name = escape_html(&get_localized_preset_name(&preset.id, ui_lang));
            let color_class = format!("color-{}", i % 12);
            format!(
                r#"<div class="preset-item {}" data-idx="{}" onclick="select({})">{}</div>"#,
                color_class, idx, idx, name
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Returns the static HTML skeleton with CSS and JS (loaded once)
pub fn get_wheel_template() -> String {
    let font_css = crate::overlay::html_components::font_manager::get_font_css();
    let css = get_css();
    let js = get_js();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
{font_css}
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

fn get_css() -> &'static str {
    r#"
* { margin: 0; padding: 0; box-sizing: border-box; }
html, body {
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
    font-family: 'Google Sans Flex', 'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif;
    font-variation-settings: 'wght' 500, 'wdth' 100, 'ROND' 100;
    user-select: none;
    color: #fff;
}

.container {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 100%;
    padding: 20px;
    gap: 10px;
}

/* Cancel button */
.dismiss-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 8px 22px;
    margin-bottom: 6px;
    background: rgba(85, 34, 34, 0.95);
    backdrop-filter: blur(12px);
    border: 1px solid rgba(255, 100, 100, 0.3);
    border-radius: 16px;
    cursor: pointer;
    font-size: 13px;
    font-variation-settings: 'wght' 600, 'wdth' 100, 'ROND' 100;
    color: rgba(255, 200, 200, 0.9);
    
    opacity: 0;
    transform: scale(0.5);
    transition: 
        transform 0.2s cubic-bezier(0.22, 1, 0.36, 1),
        opacity 0.15s ease-out,
        background 0.1s ease,
        box-shadow 0.1s ease,
        font-variation-settings 0.15s ease;
}

.dismiss-btn.visible {
    opacity: 1;
    transform: scale(1);
}

.dismiss-btn:hover {
    background: rgba(170, 51, 51, 0.95);
    border-color: rgba(255, 150, 150, 0.5);
    box-shadow: 0 4px 12px rgba(200, 50, 50, 0.4);
    font-variation-settings: 'wght' 700, 'wdth' 105, 'ROND' 100;
}

.dismiss-btn:active {
    transform: scale(0.92) !important;
}

/* Flexbox grid - natural brick-like centered clump layout */
.presets-grid {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    align-content: center;
    gap: 8px;
    max-width: 560px;
    padding: 16px;
}

.preset-item {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 9px 14px;
    min-width: 85px;
    backdrop-filter: blur(12px);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 15px;
    cursor: pointer;
    font-size: 12px;
    white-space: nowrap;
    
    /* Base state */
    opacity: 0;
    transform: scale(0.5);
    
    /* Smooth transitions */
    transition: 
        transform 0.1s cubic-bezier(0.22, 1, 0.36, 1),
        opacity 0.15s ease-out,
        background 0.1s ease,
        box-shadow 0.1s ease,
        border-color 0.1s ease,
        font-variation-settings 0.1s ease;
}

.preset-item.visible {
    opacity: 1;
    transform: scale(1);
}

/* Color palette */
.color-0  { background: rgba(46, 74, 111, 0.92); }
.color-1  { background: rgba(61, 90, 50, 0.92); }
.color-2  { background: rgba(90, 60, 60, 0.92); }
.color-3  { background: rgba(77, 59, 90, 0.92); }
.color-4  { background: rgba(90, 75, 50, 0.92); }
.color-5  { background: rgba(42, 80, 80, 0.92); }
.color-6  { background: rgba(75, 50, 84, 0.92); }
.color-7  { background: rgba(59, 77, 90, 0.92); }
.color-8  { background: rgba(77, 77, 50, 0.92); }
.color-9  { background: rgba(90, 50, 84, 0.92); }
.color-10 { background: rgba(50, 84, 80, 0.92); }
.color-11 { background: rgba(84, 67, 59, 0.92); }

.preset-item.hovered {
    border-color: rgba(255, 255, 255, 0.5);
    box-shadow: 0 5px 18px rgba(0, 0, 0, 0.35);
}

.color-0.hovered  { background: rgba(51, 102, 204, 0.95); }
.color-1.hovered  { background: rgba(76, 175, 80, 0.95); }
.color-2.hovered  { background: rgba(229, 57, 53, 0.95); }
.color-3.hovered  { background: rgba(126, 87, 194, 0.95); }
.color-4.hovered  { background: rgba(255, 143, 0, 0.95); }
.color-5.hovered  { background: rgba(0, 172, 193, 0.95); }
.color-6.hovered  { background: rgba(171, 71, 188, 0.95); }
.color-7.hovered  { background: rgba(66, 165, 245, 0.95); }
.color-8.hovered  { background: rgba(156, 204, 101, 0.95); }
.color-9.hovered  { background: rgba(236, 64, 122, 0.95); }
.color-10.hovered { background: rgba(38, 198, 218, 0.95); }
.color-11.hovered { background: rgba(255, 112, 67, 0.95); }

.preset-item:active {
    transform: scale(0.88) !important;
    transition: transform 0.05s ease !important;
}
    "#
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

function getItemCenter(item) {
    const rect = item.getBoundingClientRect();
    return {
        x: rect.left + rect.width / 2,
        y: rect.top + rect.height / 2
    };
}

function updateFisheye() {
    items.forEach(item => {
        if (!item.classList.contains('visible')) return;
        
        const center = getItemCenter(item);
        const dx = mouseX - center.x;
        const dy = mouseY - center.y;
        const distance = Math.sqrt(dx * dx + dy * dy);
        
        let influence = isMouseInGrid ? Math.max(0, 1 - distance / EFFECT_RADIUS) : 0;
        influence = influence * influence * (3 - 2 * influence); // smoothstep
        
        // Only scale UP - never below 1.0
        const scale = MIN_SCALE + (MAX_SCALE - MIN_SCALE) * influence;
        
        const weight = BASE_WEIGHT + (MAX_WEIGHT - BASE_WEIGHT) * influence;
        const width = BASE_WIDTH + (MAX_WIDTH - BASE_WIDTH) * influence;
        
        item.style.transform = `scale(${scale.toFixed(3)})`;
        item.style.fontVariationSettings = `'wght' ${weight.toFixed(0)}, 'wdth' ${width.toFixed(0)}, 'ROND' 100`;
        
        if (influence > 0.5) {
            item.classList.add('hovered');
        } else {
            item.classList.remove('hovered');
        }
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
    
    // Reset/Setup items
    dismissBtn.classList.remove('visible');
    items.forEach(item => item.classList.remove('visible'));

    // Dismiss button first (it's at top center)
    requestAnimationFrame(() => {
        setTimeout(() => dismissBtn.classList.add('visible'), 0);
        
        // Then items in ripple order from center out - fast stagger
        itemsWithDistance.forEach(({ item }, i) => {
            setTimeout(() => item.classList.add('visible'), i * 12);
        });
    });
}

// Function called by Rust to update content and trigger animation
window.updateContent = function(itemsHtml, dismissLabel) {
    grid.innerHTML = itemsHtml;
    dismissBtn.innerText = dismissLabel;
    
    // Re-query items
    items = Array.from(document.querySelectorAll('.preset-item'));
    
    // Notify Rust we are ready to be visible immediately (robust against rAF throttling)
    setTimeout(() => {
        window.ipc.postMessage('ready_to_show');
        // Start animation frame loop
        requestAnimationFrame(animateIn);
    }, 0);
};

document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') dismiss();
});
    "#
}
