pub fn get(is_dark: bool) -> String {
    let (
        bg_color,
        text_color,
        border_color,
        border_focus_color,
        label_color,
        slider_bg,
        switch_bg,
        switch_on_bg,
        slider_thumb,
        hover_bg,
        divider_color,
        shadow_lg,
        shadow_sm,
    ) = if is_dark {
        (
            "rgba(30, 30, 30, 0.98)",  // bg_color
            "#ccc",                    // text_color (general)
            "rgba(255, 150, 51, 0.5)", // border_color (default orange)
            "#00c8ff80",               // border_focus_color (blue)
            "#aaa",                    // label_color
            "#444",                    // slider_bg
            "#444",                    // switch_bg
            "#4caf50",                 // switch_on_bg
            "#ff9633",                 // slider_thumb
            "rgba(0, 200, 255, 0.15)", // hover_bg
            "#555",                    // divider_color
            "rgba(0,0,0,0.5)",         // shadow_lg
            "#ff963330",               // shadow_sm (orange glow)
        )
    } else {
        (
            "rgba(255, 255, 255, 0.98)",
            "#202124",
            "rgba(255, 150, 51, 0.3)", // Lighter orange border
            "#00c8ff50",
            "#5f6368",
            "#e0e0e0",
            "#dadce0",
            "#34a853", // Google green
            "#fa7b17", // Slightly darker orange for visibility
            "rgba(0, 200, 255, 0.08)",
            "#dadce0",
            "rgba(0,0,0,0.15)",
            "#ff963320",
        )
    };

    let title_color_tts = "#ff9633"; // Orange
    let title_color_app = "#00c8ff"; // Blue

    format!(
        r###"
        /* TTS Settings Modal */
        #tts-modal {{
            display: none;
            position: fixed !important;
            top: 50% !important;
            left: 50% !important;
            transform: translate(-50%, -50%) !important;
            background: {bg_color};
            border: 1px solid {border_color};
            border-radius: 12px;
            padding: 16px 20px;
            z-index: 2147483647 !important; /* Max Element Z-Index */
            min-width: 200px;
            box-shadow: 0 8px 32px {shadow_lg}, 0 0 20px {shadow_sm};
            color: {text_color};
        }}
        #tts-modal.show {{
            display: block !important;
            animation: modal-appear 0.2s ease-out;
        }}
        @keyframes modal-appear {{
            from {{ opacity: 0; transform: translate(-50%, -50%) scale(0.9); }}
            to {{ opacity: 1; transform: translate(-50%, -50%) scale(1); }}
        }}
        #tts-modal-overlay {{
            display: none;
            position: fixed !important;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgba(0,0,0,0.3);
            z-index: 2147483646 !important;
        }}
        #tts-modal-overlay.show {{
            display: block !important;
        }}
        .tts-modal-title {{
            font-size: 13px;
            font-weight: bold;
            color: {title_color_tts};
            margin-bottom: 12px;
            display: flex;
            align-items: center;
            gap: 6px;
        }}
        .tts-modal-row {{
            display: flex;
            align-items: center;
            justify-content: space-between;
            margin-bottom: 12px;
            gap: 12px;
        }}
        .tts-modal-row:last-child {{
            margin-bottom: 0;
        }}
        .tts-modal-label {{
            font-size: 12px;
            color: {label_color};
            white-space: nowrap;
        }}
        /* Toggle Switch */
        .toggle-switch {{
            position: relative;
            width: 40px;
            height: 22px;
            background: {switch_bg};
            border-radius: 11px;
            cursor: pointer;
            transition: background 0.2s;
        }}
        .toggle-switch.on {{
            background: {switch_on_bg};
        }}
        .toggle-switch::after {{
            content: '';
            position: absolute;
            top: 2px;
            left: 2px;
            width: 18px;
            height: 18px;
            background: #fff;
            border-radius: 50%;
            transition: transform 0.2s;
            box-shadow: 0 1px 3px rgba(0,0,0,0.2);
        }}
        .toggle-switch.on::after {{
            transform: translateX(18px);
        }}
        /* Speed Slider */
        .speed-slider-container {{
            display: flex;
            align-items: center;
            gap: 8px;
        }}
        .speed-slider {{
            -webkit-appearance: none;
            width: 100px;
            height: 6px;
            background: {slider_bg};
            border-radius: 3px;
            outline: none;
        }}
        .speed-slider::-webkit-slider-thumb {{
            -webkit-appearance: none;
            width: 14px;
            height: 14px;
            background: {slider_thumb};
            border-radius: 50%;
            cursor: pointer;
            transition: transform 0.1s;
            box-shadow: 0 1px 3px rgba(0,0,0,0.2);
        }}
        .speed-slider::-webkit-slider-thumb:hover {{
            transform: scale(1.2);
        }}
        .speed-value {{
            font-size: 11px;
            color: {slider_thumb};
            font-weight: bold;
            min-width: 32px;
            text-align: right;
        }}
        .auto-toggle {{
            padding: 4px 10px;
            font-size: 10px;
            font-weight: bold;
            border: 1px solid {divider_color};
            border-radius: 12px;
            background: transparent;
            color: {label_color};
            cursor: pointer;
            transition: all 0.2s;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }}
        .auto-toggle:hover {{
            border-color: {title_color_tts};
            color: {title_color_tts};
        }}
        .auto-toggle.on {{
            background: linear-gradient(135deg, {title_color_tts} 0%, #ff6b00 100%);
            border-color: {title_color_tts};
            color: #fff;
            white-space: nowrap;
        }}
        
        /* App Selection Modal */
        #app-modal {{
            display: none;
            position: fixed !important;
            top: 50% !important;
            left: 50% !important;
            transform: translate(-50%, -50%) !important;
            background: {bg_color};
            border: 1px solid {border_focus_color};
            border-radius: 12px;
            padding: 16px 20px;
            z-index: 2000 !important;
            min-width: 280px;
            max-width: 400px;
            max-height: 70vh;
            box-shadow: 0 8px 32px {shadow_lg}, 0 0 20px {shadow_sm};
            overflow: hidden;
            color: {text_color};
        }}
        #app-modal.show {{
            display: block !important;
            animation: modal-appear 0.2s ease-out;
        }}
        #app-modal-overlay {{
            display: none;
            position: fixed !important;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgba(0,0,0,0.3);
            z-index: 1999 !important;
        }}
        #app-modal-overlay.show {{
            display: block !important;
        }}
        .app-modal-title {{
            font-size: 13px;
            font-weight: bold;
            color: {title_color_app};
            margin-bottom: 8px;
            display: flex;
            align-items: center;
            gap: 6px;
        }}
        .app-modal-hint {{
            font-size: 10px;
            color: {label_color};
            margin-bottom: 12px;
        }}
        .app-list {{
            max-height: 300px;
            overflow-y: auto;
            scrollbar-width: thin;
            scrollbar-color: {divider_color} {bg_color};
        }}
        .app-list::-webkit-scrollbar {{
            width: 6px;
        }}
        .app-list::-webkit-scrollbar-track {{
            background: {bg_color};
            border-radius: 3px;
        }}
        .app-list::-webkit-scrollbar-thumb {{
            background: {divider_color};
            border-radius: 3px;
        }}
        .app-item {{
            display: flex;
            align-items: center;
            gap: 8px;
            padding: 8px 10px;
            border-radius: 6px;
            cursor: pointer;
            transition: all 0.15s;
            margin-bottom: 4px;
        }}
        .app-item:hover {{
            background: {hover_bg};
        }}
        .app-item .app-icon {{
            font-size: 18px;
            color: {title_color_app};
        }}
        .app-item .app-title {{
            font-size: 12px;
            color: {text_color};
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
            flex: 1;
        }}
        .app-item .app-pid {{
            font-size: 9px;
            color: {label_color};
        }}
        .app-loading {{
            font-size: 12px;
            color: {label_color};
            text-align: center;
            padding: 20px;
        }}
        .app-name-badge {{
            font-size: 10px;
            color: {title_color_app};
            background: {hover_bg};
            padding: 2px 6px;
            border-radius: 10px;
            max-width: 80px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }}
        /* Download Modal */
        #download-modal {{
            display: none;
            position: fixed !important;
            top: 50% !important;
            left: 50% !important;
            transform: translate(-50%, -50%) !important;
            background: {bg_color};
            border: 1px solid {border_focus_color};
            border-radius: 12px;
            padding: 12px 16px;
            z-index: 2147483647 !important;
            min-width: 320px;
            max-width: 90vw;
            box-shadow: 0 8px 32px {shadow_lg}, 0 0 20px {shadow_sm};
            text-align: center;
            color: {text_color};
        }}
        #download-modal.show {{
            display: block !important;
            animation: modal-appear 0.2s ease-out;
        }}
        #download-modal-overlay {{
            display: none;
            position: fixed !important;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgba(0,0,0,0.5);
            z-index: 2147483646 !important;
        }}
        #download-modal-overlay.show {{
            display: block !important;
        }}
        .download-modal-title {{
            font-size: 13px;
            font-weight: bold;
            color: {title_color_app};
            margin-bottom: 8px;
            display: flex;
            align-items: center;
            justify-content: center;
            gap: 6px;
            line-height: 1.2;
        }}
        .download-modal-title .material-symbols-rounded {{
            font-size: 18px;
            width: 18px;
            height: 18px;
            flex-shrink: 0;
        }}
        .download-modal-msg {{
            font-size: 11px;
            color: {text_color};
            margin-bottom: 12px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
            max-width: 100%;
        }}
        .download-progress-bar {{
            width: 100%;
            height: 6px;
            background: {slider_bg};
            border-radius: 3px;
            overflow: hidden;
            margin-bottom: 8px;
        }}
        .download-progress-fill {{
            height: 100%;
            background: linear-gradient(90deg, #00c8ff, #0080ff);
            width: 0%;
            transition: width 0.2s;
        }}
        .download-modal-footnote {{
            font-size: 10px;
            color: {label_color};
            font-style: italic;
        }}
        .download-cancel-btn {{
            display: flex;
            align-items: center;
            justify-content: center;
            gap: 4px;
            margin-top: 12px;
            padding: 8px 16px;
            background: transparent;
            border: 1px solid #ff4444;
            border-radius: 6px;
            color: #ff6666;
            font-size: 11px;
            font-weight: 500;
            cursor: pointer;
            transition: all 0.2s;
            width: 100%;
        }}
        .download-cancel-btn:hover {{
            background: rgba(255, 68, 68, 0.15);
            border-color: #ff6666;
            color: #ff8888;
        }}
        .download-cancel-btn .material-symbols-rounded {{
            font-size: 14px;
        }}
    "###,
        bg_color = bg_color,
        text_color = text_color,
        border_color = border_color,
        border_focus_color = border_focus_color,
        shadow_lg = shadow_lg,
        shadow_sm = shadow_sm,
        title_color_tts = title_color_tts,
        title_color_app = title_color_app,
        label_color = label_color,
        switch_bg = switch_bg,
        switch_on_bg = switch_on_bg,
        slider_bg = slider_bg,
        slider_thumb = slider_thumb,
        divider_color = divider_color,
        hover_bg = hover_bg
    )
}
