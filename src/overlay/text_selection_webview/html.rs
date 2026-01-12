pub fn get_html(is_dark: bool, initial_text: &str) -> String {
    let font_css = crate::overlay::html_components::font_manager::get_font_css();

    let (bg_color, text_color, glow_base, glow_active) = if is_dark {
        (
            "rgba(26, 26, 26, 0.95)", // Slightly more opaque
            "#ffffff",
            "#00c8ff",
            "#ff9633",
        )
    } else {
        ("rgba(255, 255, 255, 0.95)", "#202124", "#00c8ff", "#ff9633")
    };

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <style>
        {font_css}
        * {{
            margin: 0;
            padding: 0;
            user-select: none;
            cursor: default;
        }}
        body {{
            background: transparent;
            overflow: hidden;
            display: flex;
            align-items: center;
            justify-content: center;
            height: 100vh;
            width: 100vw;
            font-family: 'Google Sans Flex', 'Segoe UI', system-ui, sans-serif;
            font-weight: 500;
        }}
        
        /* Container for the gradient border */
        .badge-container {{
            position: relative;
            padding: 2px; /* Border thickness */
            border-radius: 22px;
            overflow: hidden;
            /* Entrance Animation */
            opacity: 0;
            transform: translateY(10px);
            animation: fadeIn 0.3s cubic-bezier(0.2, 0, 0, 1) forwards;
            /* Shadow for depth */
            box-shadow: 0 4px 12px rgba(0,0,0,0.15);
        }}

        /* The spinning gradient background (visible via padding) */
        .badge-glow {{
            position: absolute;
            top: -50%;
            left: -50%;
            width: 200%;
            height: 200%;
            background: conic-gradient(
                from 0deg, 
                transparent 0deg, 
                var(--glow-color) 60deg, 
                transparent 120deg,
                transparent 180deg, 
                var(--glow-color) 240deg, 
                transparent 300deg
            );
            animation: spin 3s linear infinite;
            z-index: 1;
        }}

        /* The inner content badge */
        .badge-inner {{
            position: relative;
            background: {bg_color};
            color: {text_color};
            padding: 8px 16px;
            border-radius: 20px;
            font-size: 14px;
            white-space: nowrap;
            z-index: 2;
            display: flex;
            align-items: center;
            gap: 8px;
        }}

        @keyframes fadeIn {{
            to {{ opacity: 1; transform: translateY(0); }}
        }}

        @keyframes spin {{
            from {{ transform: rotate(0deg); }}
            to {{ transform: rotate(360deg); }}
        }}

        /* State: Selecting (Active) */
        body.selecting .badge-glow {{
            --glow-color: {glow_active};
            animation-duration: 1s; /* Faster spin */
            opacity: 1;
        }}
        
        /* State: Idle */
        body:not(.selecting) .badge-glow {{
            --glow-color: {glow_base};
            opacity: 0.6; /* Subtler glow when idle */
        }}

    </style>
</head>
<body>
    <div class="badge-container">
        <div class="badge-glow"></div>
        <div class="badge-inner">
            <span id="text">{text}</span>
        </div>
    </div>

    <script>
        function updateState(isSelecting, newText) {{
            if (isSelecting) {{
                document.body.classList.add('selecting');
            }} else {{
                document.body.classList.remove('selecting');
            }}
            document.getElementById('text').innerText = newText;
        }}
    </script>
</body>
</html>"#,
        font_css = font_css,
        bg_color = bg_color,
        text_color = text_color,
        glow_base = glow_base,
        glow_active = glow_active,
        text = initial_text
    )
}
