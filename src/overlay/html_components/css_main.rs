pub fn get(glow_color: &str, font_size: u32, is_dark: bool) -> String {
    let (
        bg_color,
        text_color,
        header_bg,
        border_color,
        ctrl_bg,
        ctrl_hover_bg,
        select_bg,
        select_option_bg,
        placeholder_color,
        resize_hint_color,
        scrollbar_track,
        scrollbar_thumb,
        scrollbar_thumb_hover,
        ctrl_hover_text,
        icon_inactive_color,
    ) = if is_dark {
        (
            "rgba(26, 26, 26, 0.95)",    // bg_color
            "#fff",                      // text_color
            "rgba(26, 26, 26, 0.6)",     // header_bg
            format!("{}40", glow_color), // border_color
            "rgba(30,30,30,0.8)",        // ctrl_bg
            "rgba(255,255,255,0.15)",    // ctrl_hover_bg
            "rgba(30, 30, 30, 0.9)",     // select_bg
            "#2a2a2a",                   // select_option_bg
            "#aaa",                      // placeholder_color
            "#888",                      // resize_hint_color
            "#2a2a2a",                   // scrollbar_track
            "#555",                      // scrollbar_thumb
            "#777",                      // scrollbar_thumb_hover
            "#ffffff",                   // ctrl_hover_text
            "#888",                      // icon_inactive_color
        )
    } else {
        (
            "rgba(255, 255, 255, 0.95)",
            "#202124",
            "rgba(255, 255, 255, 0.8)",
            format!("{}80", glow_color),
            "rgba(240, 240, 245, 0.8)",
            "rgba(0, 0, 0, 0.05)",
            "rgba(255, 255, 255, 0.9)",
            "#ffffff",
            "#80868b",
            "#9aa0a6",
            "#f1f3f4",
            "#dadce0",
            "#bdc1c6",
            "#202124",
            "#dadce0", // icon_inactive_color
        )
    };

    let box_shadow = if is_dark {
        format!("0 0 20px {}30", glow_color)
    } else {
        format!("0 0 20px {}20", glow_color)
    };

    let ctrl_border = if is_dark {
        "rgba(255,255,255,0.1)"
    } else {
        "rgba(0,0,0,0.1)"
    };

    format!(
        r###"        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        html, body {{
            height: 100%;
            overflow: hidden;
            background: {bg_color};
            font-family: 'Google Sans Flex', sans-serif;
            color: {text_color};
            border-radius: 8px;
            border: 1px solid {border_color};
            box-shadow: {box_shadow};
        }}
        /* Loading overlay - TEMPORARILY DISABLED FOR TESTING */
        #loading-overlay {{
            display: none; /* TEMP: Remove this line to re-enable overlay */
            position: fixed;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: {bg_color};
            z-index: 9999;
            pointer-events: none;
            justify-content: center;
            align-items: center;
            animation: fadeOut 0.4s ease-out 0.9s forwards;
        }}
        .loading-svg {{
            width: 72px;
            height: 72px;
            filter: drop-shadow(0 0 12px {glow_color}90);
            animation: breathe 2.5s ease-in-out infinite;
        }}
        @keyframes breathe {{
            0%, 100% {{ 
                transform: scale(1); 
                opacity: 0.85;
                filter: drop-shadow(0 0 8px {glow_color}60);
            }}
            50% {{ 
                transform: scale(1.08); 
                opacity: 1;
                filter: drop-shadow(0 0 20px {glow_color});
            }}
        }}
        @keyframes fadeOut {{
            from {{ opacity: 1; }}
            to {{ opacity: 0; }}
        }}
        .material-symbols-rounded {{
            font-family: 'Material Symbols Rounded'; /* Fallback */
            font-weight: normal;
            font-style: normal;
            font-size: 24px;
            line-height: 1;
            letter-spacing: normal;
            text-transform: none;
            display: inline-flex; /* Center SVG */
            align-items: center;
            justify-content: center;
            white-space: nowrap;
            word-wrap: normal;
            direction: ltr;
            vertical-align: middle;
            
            /* SVG container sizing */
            width: 1em;
            height: 1em;
        }}
        .material-symbols-rounded svg {{
            width: 100%;
            height: 100%;
            fill: currentColor;
            display: block;
        }}
        #container {{
            display: flex;
            flex-direction: column;
            height: 100%;
            padding: 8px 12px;
            cursor: grab;
            position: relative;
        }}
        #container:active {{
            cursor: grabbing;
        }}
        #header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 6px;
            flex-shrink: 0;
            gap: 8px;
            transition: all 0.25s ease-out;
            overflow: hidden;
            max-height: 40px;
            background: {header_bg};
            backdrop-filter: blur(8px);
            border-radius: 6px;
        }}
        #header.collapsed {{
            max-height: 0;
            margin-bottom: 0;
            opacity: 0;
        }}
        @keyframes pulse {{
            0%, 100% {{ transform: translateX(-50%) scale(1); opacity: 0.7; }}
            50% {{ transform: translateX(-50%) scale(1.2); opacity: 1; }}
        }}
        #header-toggle {{
            position: absolute;
            left: 50%;
            transform: translateX(-50%);
            display: flex;
            justify-content: center;
            align-items: center;
            cursor: pointer;
            padding: 2px 6px;
            color: #666;
            transition: all 0.25s ease-out;
            z-index: 10;
            top: 32px;
            opacity: 0.4;
        }}
        #header:hover ~ #header-toggle {{
            color: #00c8ff;
            opacity: 1;
            animation: pulse 1s ease-in-out infinite;
        }}
        #header-toggle:hover {{
            color: #fff;
            opacity: 1;
            animation: pulse 0.8s ease-in-out infinite;
        }}
        #header-toggle.collapsed {{
            top: 4px;
            opacity: 0.3;
            animation: none;
        }}
        #header-toggle.collapsed:hover {{
            opacity: 0.8;
        }}
        #header-toggle .material-symbols-rounded {{
            font-size: 14px;
            transition: transform 0.25s ease-out;
        }}
        #header-toggle.collapsed .material-symbols-rounded {{
            transform: rotate(180deg);
        }}
        #title {{
            font-size: 12px;
            font-weight: bold;
            color: {placeholder_color};
            flex-shrink: 0;
            display: flex;
            align-items: center;
            gap: 6px;
        }}
        #volume-canvas {{
            height: 24px;
            width: 90px;
            border-radius: 2px;
        }}
        #controls {{
            position: relative;
            z-index: 50;
            display: flex;
            gap: 8px;
            align-items: center;
            flex: 1;
            justify-content: flex-end;
        }}
        .btn-group {{
            display: flex;
            gap: 1px;
            align-items: center;
        }}
        .ctrl-btn {{
            font-size: 20px;
            color: {resize_hint_color};
            cursor: pointer;
            padding: 2px;
            border-radius: 50%;
            background: {ctrl_bg};
            border: 1px solid {ctrl_border};
            transition: all 0.2s;
            user-select: none;
            width: 26px;
            height: 26px;
            display: flex;
            align-items: center;
            justify-content: center;
        }}
        .ctrl-btn:hover {{
            color: {ctrl_hover_text};
            background: {ctrl_hover_bg};
            border-color: {glow_color};
            box-shadow: 0 0 8px {glow_color}40;
        }}
        .ctrl-btn.copied {{
            color: #4caf50 !important;
            border-color: #4caf50;
            box-shadow: 0 0 8px #4caf5040;
        }}
        .pill-group {{
            display: flex;
            align-items: center;
            background: {ctrl_bg};
            border: 1px solid {ctrl_border};
            border-radius: 20px;
            padding: 2px;
            gap: 1px;
            transition: all 0.2s;
        }}
        .pill-group:hover {{
            border-color: {glow_color}60;
            box-shadow: 0 0 10px {glow_color}20;
        }}
        .pill-group .ctrl-btn {{
            background: transparent;
            border: none;
            width: 22px;
            height: 22px;
        }}
        .pill-group .ctrl-btn:hover {{
            background: rgba(255, 255, 255, 0.1);
            box-shadow: none;
        }}
        .vis-btn {{
            font-size: 20px;
            cursor: pointer;
            padding: 2px;
            border-radius: 4px;
            transition: all 0.2s;
            user-select: none;
            background: transparent;
            border: none;
        }}
        .vis-btn.active {{
            opacity: 1;
        }}
        .vis-btn.inactive {{
            opacity: 0.3;
        }}
        .vis-btn:hover {{
            opacity: 0.7;
        }}
        .vis-btn.mic {{
            color: #00c8ff;
        }}
        .vis-btn.trans {{
            color: #ff9633;
        }}
        select {{
            font-family: 'Google Sans Flex', sans-serif;
            font-variation-settings: 'wght' 600, 'ROND' 100;
            background: {select_bg};
            color: {text_color};
            border: 1px solid {ctrl_border};
            border-radius: 50%;
            padding: 0;
            font-size: 10px;
            font-weight: bold;
            cursor: pointer;
            outline: none;
            width: 26px;
            height: 26px;
            scrollbar-width: thin;
            scrollbar-color: {scrollbar_thumb} {scrollbar_track};
            transition: all 0.2s;
            -webkit-appearance: none;
            -moz-appearance: none;
            appearance: none;
            text-align: center;
            text-align-last: center;
        }}
        select:hover {{
            border-color: {glow_color};
            box-shadow: 0 0 6px {glow_color}30;
        }}
        select option {{
            font-family: 'Google Sans Flex', sans-serif;
            background: {select_option_bg};
            color: {text_color};
            padding: 4px 8px;
        }}
        select option:checked {{
            background: linear-gradient(0deg, {glow_color}40, {glow_color}40);
        }}
        /* Custom scrollbar for WebKit browsers */
        select::-webkit-scrollbar {{
            width: 8px;
        }}
        select::-webkit-scrollbar-track {{
            background: {scrollbar_track};
            border-radius: 4px;
        }}
        select::-webkit-scrollbar-thumb {{
            background: {scrollbar_thumb};
            border-radius: 4px;
        }}
        select::-webkit-scrollbar-thumb:hover {{
            background: {scrollbar_thumb_hover};
        }}
        #viewport {{
            flex: 1;
            overflow: hidden;
            position: relative;
        }}
        #content {{
            font-size: {font_size}px;
            line-height: 1.5;
            padding-bottom: 5px;
        }}
        @keyframes wipe-in {{
            from {{
                -webkit-mask-position: 100% 0;
                mask-position: 100% 0;
                transform: translateX(-4px);
                opacity: 0;
                filter: blur(2px);
            }}
            to {{
                -webkit-mask-position: 0% 0;
                mask-position: 0% 0;
                transform: translateX(0);
                opacity: 1;
                filter: blur(0);
            }}
        }}

        /* Base styling for all text chunks */
        .text-chunk {{
            font-family: 'Google Sans Flex', sans-serif !important;
            font-optical-sizing: auto;
            display: inline;
            transition: 
                color 0.6s cubic-bezier(0.2, 0, 0.2, 1),
                font-variation-settings 0.6s cubic-bezier(0.2, 0, 0.2, 1),
                -webkit-mask-position 0.35s cubic-bezier(0.2, 0, 0.2, 1),
                mask-position 0.35s cubic-bezier(0.2, 0, 0.2, 1),
                opacity 0.35s ease-out,
                filter 0.35s ease-out;
        }}
        
        /* Old/committed text styling */
        .text-chunk.old {{
            color: {placeholder_color};
            font-variation-settings: 'wght' 300, 'wdth' 100, 'slnt' 0, 'GRAD' 0, 'ROND' 100, 'ROUN' 100, 'RNDS' 100;
        }}
        
        /* New/uncommitted text styling */
        .text-chunk.new {{
            color: {text_color};
            font-variation-settings: 'wght' 350, 'wdth' 99, 'slnt' 0, 'GRAD' 150, 'ROND' 100, 'ROUN' 100, 'RNDS' 100;
        }}
        
        /* Appearing state - wipe animation */
        .text-chunk.appearing {{
            color: {text_color};
            font-variation-settings: 'wght' 350, 'wdth' 99, 'slnt' 0, 'GRAD' 150, 'ROND' 100, 'ROUN' 100, 'RNDS' 100;
            
            -webkit-mask-image: linear-gradient(to right, black 50%, transparent 100%);
            mask-image: linear-gradient(to right, black 50%, transparent 100%);
            -webkit-mask-size: 200% 100%;
            mask-size: 200% 100%;
            -webkit-mask-position: 100% 0;
            mask-position: 100% 0;
            opacity: 0;
            filter: blur(2px);
        }}
        
        /* Appearing -> visible */
        .text-chunk.appearing.show {{
            -webkit-mask-position: 0% 0;
            mask-position: 0% 0;
            opacity: 1;
            filter: blur(0);
        }}
        .placeholder {{
            color: #666;
            font-style: italic;
        }}
        /* Resize handle - visible grip in corner */
         #resize-hint {{
             position: absolute;
             bottom: 0;
             right: 0;
             width: 16px;
             height: 16px;
             cursor: se-resize;
             opacity: 0.2;
             display: flex;
             align-items: flex-end;
             justify-content: flex-end;
             padding: 2px;
             font-size: 10px;
             color: {resize_hint_color};
             user-select: none;
         }}
        #resize-hint:hover {{
             opacity: 1;
             color: {glow_color};
         }}
        .audio-icon {{
            font-size: 22px;
            padding: 0;
            cursor: pointer;
            color: {icon_inactive_color};
            transition: all 0.2s;
            background: transparent;
            border: none;
        }}
        .audio-icon:hover {{
            color: #aaa;
        }}
        .audio-icon.active {{
            color: #00c8ff;
        }}
        .model-icon {{
            font-size: 22px;
            padding: 0;
            cursor: pointer;
            color: {icon_inactive_color};
            transition: all 0.2s;
            background: transparent;
            border: none;
        }}
        .model-icon:hover {{
            color: #aaa;
        }}
        .model-icon.active {{
            color: #ff9633;
        }}
        @keyframes model-switch-pulse {{
            0% {{ transform: scale(1); box-shadow: 0 0 0 0 rgba(255,150,51,0.7); }}
            25% {{ transform: scale(1.3); box-shadow: 0 0 15px 5px rgba(255,150,51,0.5); }}
            50% {{ transform: scale(1.1); box-shadow: 0 0 10px 3px rgba(255,150,51,0.3); }}
            75% {{ transform: scale(1.2); box-shadow: 0 0 12px 4px rgba(255,150,51,0.4); }}
            100% {{ transform: scale(1); box-shadow: 0 0 0 0 rgba(255,150,51,0); }}
        }}
        .model-icon.switching {{
            animation: model-switch-pulse 2s ease-out;
            color: #ff9633 !important;
            background: rgba(255,150,51,0.3) !important;
        }}
        
        /* Transcription Model Icons */
        .trans-model-icon {{
            font-size: 22px;
            padding: 0;
            cursor: pointer;
            color: {icon_inactive_color};
            transition: all 0.2s;
            background: transparent;
            border: none;
        }}
        .trans-model-icon:hover {{
            color: #aaa;
        }}
        .trans-model-icon.active[data-value="gemini"] {{
            color: #00c8ff;
        }}
        .trans-model-icon.active[data-value="parakeet"] {{
            color: #ff9633;
        }}

        /* Waveform animation for listening state */
        .wave-line {{
             transform-box: fill-box;
             transform-origin: center;
             animation: wave-animation 1.2s ease-in-out infinite;
        }}
        .wave-line.delay-1 {{ animation-delay: 0s; }}
        .wave-line.delay-2 {{ animation-delay: 0.15s; }}
        .wave-line.delay-3 {{ animation-delay: 0.3s; }}
        .wave-line.delay-4 {{ animation-delay: 0.1s; }}
        
        @keyframes wave-animation {{
            0%, 100% {{
                transform: scaleY(1);
            }}
            50% {{
                transform: scaleY(1.8);
            }}
        }}

        /* Translation animation */
        .trans-part-1 {{
            animation: lang-bounce 2s ease-in-out infinite;
        }}
        .trans-part-2 {{
            animation: lang-bounce 2s ease-in-out infinite;
            animation-delay: 1s;
        }}
        @keyframes lang-bounce {{
            0%, 100% {{ transform: translateY(0); opacity: 0.8; }}
            50% {{ transform: translateY(-3px); opacity: 1; }}
        }}
        
        /* Speak button styling */
        .speak-btn {{
            position: relative;
        }}
        .speak-btn.active {{
            color: #4caf50 !important;
            border-color: #4caf50;
            box-shadow: 0 0 8px #4caf5040;
        }}
        .speak-btn.active .material-symbols-rounded {{
            animation: speak-pulse 1.5s ease-in-out infinite;
        }}
        @keyframes speak-pulse {{
            0%, 100% {{ opacity: 1; }}
            50% {{ opacity: 0.5; }}
        }}
        "###,
        bg_color = bg_color,
        text_color = text_color,
        header_bg = header_bg,
        border_color = border_color,
        box_shadow = box_shadow,
        glow_color = glow_color,
        font_size = font_size,
        ctrl_bg = ctrl_bg,
        ctrl_border = ctrl_border,
        select_bg = select_bg,
        select_option_bg = select_option_bg,
        scrollbar_thumb = scrollbar_thumb,
        scrollbar_track = scrollbar_track,
        scrollbar_thumb_hover = scrollbar_thumb_hover,
        placeholder_color = placeholder_color,
        resize_hint_color = resize_hint_color,
        ctrl_hover_bg = ctrl_hover_bg,
        ctrl_hover_text = ctrl_hover_text,
        icon_inactive_color = icon_inactive_color,
    )
}
