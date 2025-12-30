pub fn get(glow_color: &str, font_size: u32) -> String {
    format!(
        r###"        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        html, body {{
            height: 100%;
            overflow: hidden;
            background: rgba(26, 26, 26, 0.95);
            font-family: 'Google Sans Flex', sans-serif;
            color: #fff;
            border-radius: 8px;
            border: 1px solid {glow_color}40;
            box-shadow: 0 0 20px {glow_color}30;
        }}
        /* Loading overlay - TEMPORARILY DISABLED FOR TESTING */
        #loading-overlay {{
            display: none; /* TEMP: Remove this line to re-enable overlay */
            position: fixed;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgb(26, 26, 26);
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
            background: rgba(26, 26, 26, 0.6);
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
            color: #aaa;
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
            color: #888;
            cursor: pointer;
            padding: 2px;
            border-radius: 50%;
            background: rgba(30,30,30,0.8);
            border: 1px solid rgba(255,255,255,0.1);
            transition: all 0.2s;
            user-select: none;
            width: 26px;
            height: 26px;
            display: flex;
            align-items: center;
            justify-content: center;
        }}
        .ctrl-btn:hover {{
            color: #fff;
            background: rgba(255,255,255,0.15);
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
            background: rgba(30,30,30,0.8);
            border: 1px solid rgba(255,255,255,0.1);
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
            background: rgba(30, 30, 30, 0.9);
            color: #ccc;
            border: 1px solid rgba(255,255,255,0.15);
            border-radius: 50%;
            padding: 0;
            font-size: 10px;
            font-weight: bold;
            cursor: pointer;
            outline: none;
            width: 26px;
            height: 26px;
            scrollbar-width: thin;
            scrollbar-color: #555 #2a2a2a;
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
            background: #2a2a2a;
            color: #ccc;
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
            background: #2a2a2a;
            border-radius: 4px;
        }}
        select::-webkit-scrollbar-thumb {{
            background: #555;
            border-radius: 4px;
        }}
        select::-webkit-scrollbar-thumb:hover {{
            background: #777;
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
            color: #9aa0a6;
            font-variation-settings: 'wght' 300, 'wdth' 100, 'slnt' 0, 'GRAD' 0, 'ROND' 100, 'ROUN' 100, 'RNDS' 100;
        }}
        
        /* New/uncommitted text styling */
        .text-chunk.new {{
            color: #ffffff;
            font-variation-settings: 'wght' 350, 'wdth' 99, 'slnt' 0, 'GRAD' 150, 'ROND' 100, 'ROUN' 100, 'RNDS' 100;
        }}
        
        /* Appearing state - wipe animation */
        .text-chunk.appearing {{
            color: #ffffff;
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
             color: #888;
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
            color: #555;
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
            color: #555;
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
        glow_color = glow_color,
        font_size = font_size
    )
}
