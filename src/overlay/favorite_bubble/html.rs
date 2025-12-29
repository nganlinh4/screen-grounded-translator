use crate::config::Preset;
use crate::gui::settings_ui::get_localized_preset_name;

pub fn generate_panel_html(presets: &[Preset], lang: &str) -> String {
    let favorites_html = get_favorite_presets_html(presets, lang);
    let font_css = crate::overlay::html_components::font_manager::get_font_css();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
{font_css}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
html, body {{
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
    font-family: 'Google Sans Flex', 'Segoe UI', system-ui, sans-serif;
    user-select: none;
}}

.container {{
    display: flex;
    flex-direction: column;
    padding: 30px 20px; /* Default padding, will be overridden by side class */
}}

/* Bubble on right = panel on left */
.container.side-right {{
    padding-left: 30px;
    padding-right: 10px;
}}

/* Bubble on left = panel on right */
.container.side-left {{
    padding-left: 10px;
    padding-right: 30px;
}}

.list {{
    display: block;
    column-gap: 8px;
}}

.preset-item, .empty {{
    display: flex;
    align-items: center;
    padding: 8px 12px;
    border-radius: 12px;
    cursor: pointer;
    color: #eeeeee;
    font-size: 13px;
    font-variation-settings: 'wght' 500, 'wdth' 100, 'ROND' 100;
    background: rgba(20, 20, 30, 0.85);
    backdrop-filter: blur(12px);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.2);
    margin-bottom: 4px;
    break-inside: avoid;
    page-break-inside: avoid;
    
    /* Animation state */
    opacity: 0;
    pointer-events: none;
    transform: scale(0.01);
    transition: 
        transform 0.3s cubic-bezier(0.22, 1, 0.36, 1),
        opacity 0.25s ease-out,
        background 0.2s ease,
        box-shadow 0.2s ease,
        font-variation-settings 0.2s ease;
    will-change: transform, opacity;
}}

.preset-item.visible, .empty.visible {{
    opacity: 1;
    transform: scale(1) translate(0px, 0px);
    pointer-events: auto;
}}

.preset-item.visible:hover {{
    background: rgba(40, 40, 55, 0.95);
    border-color: rgba(255, 255, 255, 0.25);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    font-variation-settings: 'wght' 650, 'wdth' 105, 'ROND' 100;
    /* !important to override any lingering inline styles from bloom animation */
    transform: scale(1.05) translate(0px, 0px) !important;
    /* Fast hover-in for snappy response */
    transition: 
        transform 0.08s cubic-bezier(0.34, 1.2, 0.64, 1),
        background 0.05s ease,
        box-shadow 0.05s ease,
        font-variation-settings 0.08s ease,
        border-color 0.05s ease;
}}

.preset-item.visible:active {{
    transform: scale(0.98) translate(0px, 0px) !important;
}}

.icon {{
    display: flex;
    align-items: center;
    justify-content: center;
    margin-right: 10px;
    opacity: 0.9;
}}

.name {{
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}}

.empty {{
    color: rgba(255,255,255,0.6);
    text-align: center;
    padding: 12px;
    font-size: 12px;
    background: rgba(20, 20, 30, 0.85);
    border-radius: 12px;
    border: 1px solid rgba(255, 255, 255, 0.1);
}}

.condense {{ letter-spacing: -0.5px; }}
.condense-more {{ letter-spacing: -1px; }}
</style>
</head>
<body>
<div class="container">
    <div class="list">{favorites}</div>
</div>
<script>
function fitText() {{
    requestAnimationFrame(() => {{
        document.querySelectorAll('.name').forEach(el => {{
            el.className = 'name';
            if (el.scrollWidth > el.clientWidth) {{
                el.classList.add('condense');
                if (el.scrollWidth > el.clientWidth) {{
                    el.classList.remove('condense');
                    el.classList.add('condense-more');
                }}
            }}
        }});
    }});
}}
window.onload = fitText;

function startDrag(e) {{
    if (e.button === 0) window.ipc.postMessage('drag');
}}

function trigger(idx) {{
    closePanel();
    window.ipc.postMessage('trigger:' + idx);
}}

let currentTimeout = null;
let currentSide = 'right';
let lastBubblePos = {{ x: 0, y: 0 }};

function animateIn(bx, by) {{
    if (currentTimeout) {{
        clearTimeout(currentTimeout);
        currentTimeout = null;
    }}
    lastBubblePos = {{ x: bx, y: by }};
    
    const items = document.querySelectorAll('.preset-item, .empty');
    if (items.length === 0) return;

    items.forEach((item, i) => {{
        const rect = item.getBoundingClientRect();
        if (rect.width === 0) return; // Not rendered yet?

        // Target center
        const iy = rect.top + rect.height / 2;
        const ix = rect.left + rect.width / 2;
        
        // Offset TO move item TO bubble center
        const dx = bx - ix;
        const dy = by - iy;

        // 1. Force initial state (at bubble, invisible)
        item.style.transition = 'none';
        item.style.opacity = '0';
        item.style.transform = `translate(${{dx}}px, ${{dy}}px) scale(0.01)`;
        item.classList.remove('visible');
        
        item.offsetHeight; // Flush
        
        // 2. Set transition and target state
        item.style.transition = ''; 
        item.style.transitionDelay = `${{i * 15}}ms`;
        
        requestAnimationFrame(() => {{
            // Add class and set target state explicitly
            item.classList.add('visible');
            item.style.opacity = '1';
            item.style.transform = 'translate(0px, 0px) scale(1)';
            
            // Cleanup: remove inline styles after animation finishes to let CSS hover work
            setTimeout(() => {{
                if (item.classList.contains('visible')) {{
                    item.style.opacity = '';
                    item.style.transform = '';
                    item.style.transition = '';
                    item.style.transitionDelay = '';
                }}
            }}, 300 + (i * 15));
        }});
    }});
}}

function closePanel() {{
    if (currentTimeout) clearTimeout(currentTimeout);
    
    const items = Array.from(document.querySelectorAll('.preset-item, .empty'));
    const {{ x: bx, y: by }} = lastBubblePos;

    items.forEach((item, i) => {{
        const rect = item.getBoundingClientRect();
        const iy = rect.top + rect.height / 2;
        const ix = rect.left + rect.width / 2;
        
        const dx = bx - ix;
        const dy = by - iy;

        // Animate back to bubble with fade out
        item.style.transitionDelay = `${{(items.length - 1 - i) * 8}}ms`;
        item.classList.remove('visible');
        item.style.opacity = '0';
        item.style.transform = `translate(${{dx}}px, ${{dy}}px) scale(0.01)`;
    }});

    currentTimeout = setTimeout(() => {{
        window.ipc.postMessage('close_now');
        currentTimeout = null;
    }}, items.length * 8 + 300);
}}

window.setSide = (side) => {{ 
    currentSide = side;
    const container = document.querySelector('.container');
    container.classList.remove('side-left', 'side-right');
    container.classList.add('side-' + side);
}};
</script>
</body>
</html>"#,
        font_css = font_css,
        favorites = favorites_html
    )
}

pub fn get_favorite_presets_html(presets: &[Preset], lang: &str) -> String {
    let mut html_items = String::new();

    let icon_image = r#"<svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M12 8.8a3.2 3.2 0 1 0 0 6.4 3.2 3.2 0 0 0 0-6.4z"/><path d="M9 2L7.17 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2h-3.17L15 2H9zm3 15c-2.76 0-5-2.24-5-5s2.24-5 5-5 5 2.24 5 5-2.24 5-5 5z"/></svg>"#;
    let icon_text_type = r#"<svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M5 5h14v3h-2v-1h-3v10h2.5v2h-9v-2h2.5v-10h-3v1h-2z"/></svg>"#;
    let icon_text_select = r#"<svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M4 7h11v1.5H4z M4 11h11v2.5H4z M4 15.5h11v1.5H4z M19 6h-2v1.5h0.5v9H17v1.5h2v-1.5h-0.5v-9H19z"/></svg>"#;
    let icon_mic = r#"<svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M12 14c1.66 0 3-1.34 3-3V5c0-1.66-1.34-3-3-3S9 3.34 9 5v6c0 1.66 1.34 3 3 3zM17 11c0 2.76-2.24 5-5 5s-5-2.24-5-5H5c0 3.53 2.61 6.43 6 6.92V21h2v-3.08c3.39-.49 6-3.39 6-6.92h-2z"/></svg>"#;
    let icon_device = r#"<svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"/></svg>"#;
    let icon_realtime = r#"<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12h3 l1.5-3 l2 10 l3.5-14 l3.5 10 l2-3 h4.5"/></svg>"#;

    for (idx, preset) in presets.iter().enumerate() {
        if preset.is_favorite && !preset.is_upcoming && !preset.is_master {
            let name = if preset.id.starts_with("preset_") {
                get_localized_preset_name(&preset.id, lang)
            } else {
                preset.name.clone()
            };

            let (icon_svg, color_hex) = match preset.preset_type.as_str() {
                "audio" => {
                    if preset.audio_processing_mode == "realtime" {
                        (icon_realtime, "#ff5555")
                    } else if preset.audio_source == "device" {
                        (icon_device, "#ffaa33")
                    } else {
                        (icon_mic, "#ffaa33")
                    }
                }
                "text" => {
                    let c = "#55ff88"; // Green
                    if preset.text_input_mode == "select" {
                        (icon_text_select, c)
                    } else {
                        (icon_text_type, c)
                    }
                }
                _ => (icon_image, "#44ccff"),
            };

            let item = format!(
                r#"<div class="preset-item" onclick="trigger({})"><span class="icon" style="color: {};">{}</span><span class="name">{}</span></div>"#,
                idx,
                color_hex,
                icon_svg,
                html_escape(&name)
            );

            html_items.push_str(&item);
        }
    }

    if html_items.is_empty() {
        let locale = crate::gui::locale::LocaleText::get(lang);
        html_items = format!(
            r#"<div class="empty">{}</div>"#,
            html_escape(locale.favorites_empty)
        );
    }

    html_items
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn escape_js(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "")
}
