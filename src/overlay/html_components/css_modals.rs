pub fn get() -> String {
    r###"
        /* TTS Settings Modal */
        #tts-modal {
            display: none;
            position: fixed !important;
            top: 50% !important;
            left: 50% !important;
            transform: translate(-50%, -50%) !important;
            background: rgba(30, 30, 30, 0.98);
            border: 1px solid #ff963380;
            border-radius: 12px;
            padding: 16px 20px;
            z-index: 2147483647 !important; /* Max Element Z-Index */
            min-width: 200px;
            box-shadow: 0 8px 32px rgba(0,0,0,0.5), 0 0 20px #ff963330;
        }
        #tts-modal.show {
            display: block !important;
            animation: modal-appear 0.2s ease-out;
        }
        @keyframes modal-appear {
            from { opacity: 0; transform: translate(-50%, -50%) scale(0.9); }
            to { opacity: 1; transform: translate(-50%, -50%) scale(1); }
        }
        #tts-modal-overlay {
            display: none;
            position: fixed !important;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgba(0,0,0,0.3);
            z-index: 2147483646 !important;
        }
        #tts-modal-overlay.show {
            display: block !important;
        }
        .tts-modal-title {
            font-size: 13px;
            font-weight: bold;
            color: #ff9633;
            margin-bottom: 12px;
            display: flex;
            align-items: center;
            gap: 6px;
        }
        .tts-modal-row {
            display: flex;
            align-items: center;
            justify-content: space-between;
            margin-bottom: 12px;
            gap: 12px;
        }
        .tts-modal-row:last-child {
            margin-bottom: 0;
        }
        .tts-modal-label {
            font-size: 12px;
            color: #aaa;
            white-space: nowrap;
        }
        /* Toggle Switch */
        .toggle-switch {
            position: relative;
            width: 40px;
            height: 22px;
            background: #444;
            border-radius: 11px;
            cursor: pointer;
            transition: background 0.2s;
        }
        .toggle-switch.on {
            background: #4caf50;
        }
        .toggle-switch::after {
            content: '';
            position: absolute;
            top: 2px;
            left: 2px;
            width: 18px;
            height: 18px;
            background: #fff;
            border-radius: 50%;
            transition: transform 0.2s;
        }
        .toggle-switch.on::after {
            transform: translateX(18px);
        }
        /* Speed Slider */
        .speed-slider-container {
            display: flex;
            align-items: center;
            gap: 8px;
        }
        .speed-slider {
            -webkit-appearance: none;
            width: 100px;
            height: 6px;
            background: #444;
            border-radius: 3px;
            outline: none;
        }
        .speed-slider::-webkit-slider-thumb {
            -webkit-appearance: none;
            width: 14px;
            height: 14px;
            background: #ff9633;
            border-radius: 50%;
            cursor: pointer;
            transition: transform 0.1s;
        }
        .speed-slider::-webkit-slider-thumb:hover {
            transform: scale(1.2);
        }
        .speed-value {
            font-size: 11px;
            color: #ff9633;
            font-weight: bold;
            min-width: 32px;
            text-align: right;
        }
        .auto-toggle {
            padding: 4px 10px;
            font-size: 10px;
            font-weight: bold;
            border: 1px solid #555;
            border-radius: 12px;
            background: transparent;
            color: #888;
            cursor: pointer;
            transition: all 0.2s;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }
        .auto-toggle:hover {
            border-color: #ff9633;
            color: #ff9633;
        }
        .auto-toggle.on {
            background: linear-gradient(135deg, #ff9633 0%, #ff6b00 100%);
            border-color: #ff9633;
            color: #fff;
            white-space: nowrap;
        }
        
        /* App Selection Modal */
        #app-modal {
            display: none;
            position: fixed !important;
            top: 50% !important;
            left: 50% !important;
            transform: translate(-50%, -50%) !important;
            background: rgba(30, 30, 30, 0.98);
            border: 1px solid #00c8ff80;
            border-radius: 12px;
            padding: 16px 20px;
            z-index: 2000 !important;
            min-width: 280px;
            max-width: 400px;
            max-height: 70vh;
            box-shadow: 0 8px 32px rgba(0,0,0,0.5), 0 0 20px #00c8ff30;
            overflow: hidden;
        }
        #app-modal.show {
            display: block !important;
            animation: modal-appear 0.2s ease-out;
        }
        #app-modal-overlay {
            display: none;
            position: fixed !important;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgba(0,0,0,0.3);
            z-index: 1999 !important;
        }
        #app-modal-overlay.show {
            display: block !important;
        }
        .app-modal-title {
            font-size: 13px;
            font-weight: bold;
            color: #00c8ff;
            margin-bottom: 8px;
            display: flex;
            align-items: center;
            gap: 6px;
        }
        .app-modal-hint {
            font-size: 10px;
            color: #888;
            margin-bottom: 12px;
        }
        .app-list {
            max-height: 300px;
            overflow-y: auto;
            scrollbar-width: thin;
            scrollbar-color: #555 #2a2a2a;
        }
        .app-list::-webkit-scrollbar {
            width: 6px;
        }
        .app-list::-webkit-scrollbar-track {
            background: #2a2a2a;
            border-radius: 3px;
        }
        .app-list::-webkit-scrollbar-thumb {
            background: #555;
            border-radius: 3px;
        }
        .app-item {
            display: flex;
            align-items: center;
            gap: 8px;
            padding: 8px 10px;
            border-radius: 6px;
            cursor: pointer;
            transition: all 0.15s;
            margin-bottom: 4px;
        }
        .app-item:hover {
            background: rgba(0, 200, 255, 0.15);
        }
        .app-item .app-icon {
            font-size: 18px;
            color: #00c8ff;
        }
        .app-item .app-title {
            font-size: 12px;
            color: #ccc;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
            flex: 1;
        }
        .app-item .app-pid {
            font-size: 9px;
            color: #666;
        }
        .app-loading {
            font-size: 12px;
            color: #888;
            text-align: center;
            padding: 20px;
        }
        .app-name-badge {
            font-size: 10px;
            color: #00c8ff;
            background: rgba(0, 200, 255, 0.1);
            padding: 2px 6px;
            border-radius: 10px;
            max-width: 80px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }
        /* Download Modal */
        #download-modal {
            display: none;
            position: fixed !important;
            top: 50% !important;
            left: 50% !important;
            transform: translate(-50%, -50%) !important;
            background: rgba(30, 30, 30, 0.98);
            border: 1px solid #00c8ff80;
            border-radius: 12px;
            padding: 12px 16px;
            z-index: 2147483647 !important;
            min-width: 320px;
            max-width: 90vw;
            box-shadow: 0 8px 32px rgba(0,0,0,0.5), 0 0 20px #00c8ff30;
            text-align: center;
        }
        #download-modal.show {
            display: block !important;
            animation: modal-appear 0.2s ease-out;
        }
        #download-modal-overlay {
            display: none;
            position: fixed !important;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgba(0,0,0,0.5);
            z-index: 2147483646 !important;
        }
        #download-modal-overlay.show {
            display: block !important;
        }
        .download-modal-title {
            font-size: 13px;
            font-weight: bold;
            color: #00c8ff;
            margin-bottom: 8px;
            display: flex;
            align-items: center;
            justify-content: center;
            gap: 6px;
            line-height: 1.2;
        }
        .download-modal-title .material-symbols-rounded {
            font-size: 18px;
            width: 18px;
            height: 18px;
            flex-shrink: 0;
        }
        .download-modal-msg {
            font-size: 11px;
            color: #ccc;
            margin-bottom: 12px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
            max-width: 100%;
        }
        .download-progress-bar {
            width: 100%;
            height: 6px;
            background: #444;
            border-radius: 3px;
            overflow: hidden;
            margin-bottom: 8px;
        }
        .download-progress-fill {
            height: 100%;
            background: linear-gradient(90deg, #00c8ff, #0080ff);
            width: 0%;
            transition: width 0.2s;
        }
        .download-modal-footnote {
            font-size: 10px;
            color: #888;
            font-style: italic;
        }
"###
    .to_string()
}
