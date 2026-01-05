pub fn get(font_size: u32) -> String {
    format!(
        r###"        const container = document.getElementById('container');
        const viewport = document.getElementById('viewport');
        const content = document.getElementById('content');
        const header = document.getElementById('header');
        const headerToggle = document.getElementById('header-toggle');
        const toggleMic = document.getElementById('toggle-mic');
        const toggleTrans = document.getElementById('toggle-trans');
        const fontDecrease = document.getElementById('font-decrease');
        const fontIncrease = document.getElementById('font-increase');
        const resizeHint = document.getElementById('resize-hint');
        const copyBtn = document.getElementById('copy-btn');
        
        let currentFontSize = {font_size};
        let isResizing = false;
        let resizeStartX = 0;
        let resizeStartY = 0;
        let micVisible = true;
        let transVisible = true;
        let headerCollapsed = false;
        
        // TTS Modal elements
        const speakBtn = document.getElementById('speak-btn');
        const ttsModal = document.getElementById('tts-modal');
        const ttsModalOverlay = document.getElementById('tts-modal-overlay');
        const ttsToggle = document.getElementById('tts-toggle');
        const speedSlider = document.getElementById('speed-slider');
        const speedValue = document.getElementById('speed-value');
        let ttsEnabled = false;
        let ttsSpeed = 100;
        
        // TTS Modal Logic
        if (speakBtn && ttsModal && ttsModalOverlay) {{
            speakBtn.addEventListener('click', function(e) {{
                e.stopPropagation();
                ttsModal.classList.toggle('show');
                ttsModalOverlay.classList.toggle('show');
            }});
            
            ttsModalOverlay.addEventListener('click', function() {{
                ttsModal.classList.remove('show');
                ttsModalOverlay.classList.remove('show');
            }});
        }}
        
        if (ttsToggle) {{
            ttsToggle.addEventListener('click', function(e) {{
                e.stopPropagation();
                ttsEnabled = !ttsEnabled;
                this.classList.toggle('on', ttsEnabled);
                if (speakBtn) speakBtn.classList.toggle('active', ttsEnabled);
                window.ipc.postMessage('ttsEnabled:' + (ttsEnabled ? '1' : '0'));
            }});
        }}
        
        if (speedSlider && speedValue) {{
            const autoToggle = document.getElementById('auto-speed-toggle');
            let autoSpeed = true; // Default: auto is on
            
            speedSlider.addEventListener('input', function(e) {{
                e.stopPropagation();
                ttsSpeed = parseInt(this.value);
                speedValue.textContent = (ttsSpeed / 100).toFixed(1) + 'x';
                window.ipc.postMessage('ttsSpeed:' + ttsSpeed);
                // Auto turns off when user manually adjusts slider
                if (autoSpeed && autoToggle) {{
                    autoSpeed = false;
                    autoToggle.classList.remove('on');
                }}
            }});
            
            if (autoToggle) {{
                autoToggle.addEventListener('click', function(e) {{
                    e.stopPropagation();
                    autoSpeed = !autoSpeed;
                    this.classList.toggle('on', autoSpeed);
                    window.ipc.postMessage('ttsAutoSpeed:' + (autoSpeed ? '1' : '0'));
                }});
            }}
        }}
        
        // Header toggle (with null check in case element is commented out)
        if (headerToggle) {{
            headerToggle.addEventListener('click', function(e) {{
                e.stopPropagation();
                headerCollapsed = !headerCollapsed;
                header.classList.toggle('collapsed', headerCollapsed);
                headerToggle.classList.toggle('collapsed', headerCollapsed);
            }});
        }}
        
        // Copy button handler
        if (copyBtn) {{
            copyBtn.addEventListener('click', function(e) {{
                e.stopPropagation();
                // Get all text content (excluding placeholder)
                const textContent = content.textContent.trim();
                if (textContent && !content.querySelector('.placeholder')) {{
                    // Send to Rust via IPC for clipboard (navigator.clipboard not available in WebView2)
                    window.ipc.postMessage('copyText:' + textContent);
                    // Show success feedback
                    copyBtn.classList.add('copied');
                    const icon = copyBtn.querySelector('.material-symbols-rounded');
                    if (icon) icon.innerHTML = '{check_svg}';
                    setTimeout(() => {{
                        copyBtn.classList.remove('copied');
                        if (icon) icon.innerHTML = '{copy_svg}';
                    }}, 1500);
                }}
            }});
        }}
        
        // Drag support (left click for single window)
        container.addEventListener('mousedown', function(e) {{
            if (e.button !== 0) return; // Only left click
            if (e.target.closest('#controls') || e.target.closest('#header-toggle') || e.target.id === 'resize-hint' || isResizing) return;
            window.ipc.postMessage('startDrag');
        }});
        
        // Right-click group drag support (moves both windows together)
        let isGroupDragging = false;
        let groupDragStartX = 0;
        let groupDragStartY = 0;
        
        container.addEventListener('mousedown', function(e) {{
            if (e.button !== 2) return; // Only right click
            // Allow context menu on interactive controls
            if (e.target.closest('#controls') || e.target.closest('select')) return;
            
            e.preventDefault();
            isGroupDragging = true;
            groupDragStartX = e.screenX;
            groupDragStartY = e.screenY;
            window.ipc.postMessage('startGroupDrag');
            document.addEventListener('mousemove', onGroupDragMove);
            document.addEventListener('mouseup', onGroupDragEnd);
        }});
        
        // Prevent context menu when right-click dragging on the window body
        container.addEventListener('contextmenu', function(e) {{
            // Allow context menu on interactive controls and selects
            if (e.target.closest('#controls') || e.target.closest('select')) return;
            e.preventDefault();
        }});
        
        function onGroupDragMove(e) {{
            if (!isGroupDragging) return;
            const dx = e.screenX - groupDragStartX;
            const dy = e.screenY - groupDragStartY;
            if (dx !== 0 || dy !== 0) {{
                window.ipc.postMessage('groupDragMove:' + dx + ',' + dy);
                groupDragStartX = e.screenX;
                groupDragStartY = e.screenY;
            }}
        }}
        
        function onGroupDragEnd(e) {{
            if (isGroupDragging) {{
                isGroupDragging = false;
                document.removeEventListener('mousemove', onGroupDragMove);
                document.removeEventListener('mouseup', onGroupDragEnd);
            }}
        }}
        
        // Resize support
        resizeHint.addEventListener('mousedown', function(e) {{
            e.stopPropagation();
            e.preventDefault();
            isResizing = true;
            resizeStartX = e.screenX;
            resizeStartY = e.screenY;
            document.addEventListener('mousemove', onResizeMove);
            document.addEventListener('mouseup', onResizeEnd);
        }});
        
        function onResizeMove(e) {{
            if (!isResizing) return;
            const dx = e.screenX - resizeStartX;
            const dy = e.screenY - resizeStartY;
            if (Math.abs(dx) > 5 || Math.abs(dy) > 5) {{
                window.ipc.postMessage('resize:' + dx + ',' + dy);
                resizeStartX = e.screenX;
                resizeStartY = e.screenY;
            }}
        }}
        
        function onResizeEnd(e) {{
            isResizing = false;
            document.removeEventListener('mousemove', onResizeMove);
            document.removeEventListener('mouseup', onResizeEnd);
            window.ipc.postMessage('saveResize');
        }}
        
        // Visibility toggle buttons
        toggleMic.addEventListener('click', function(e) {{
            e.stopPropagation();
            micVisible = !micVisible;
            this.classList.toggle('active', micVisible);
            this.classList.toggle('inactive', !micVisible);
            window.ipc.postMessage('toggleMic:' + (micVisible ? '1' : '0'));
        }});
        
        toggleTrans.addEventListener('click', function(e) {{
            e.stopPropagation();
            transVisible = !transVisible;
            this.classList.toggle('active', transVisible);
            this.classList.toggle('inactive', !transVisible);
            window.ipc.postMessage('toggleTrans:' + (transVisible ? '1' : '0'));
        }});
        
        // Function to update visibility state from native side
        window.setVisibility = function(mic, trans) {{
            micVisible = mic;
            transVisible = trans;
            toggleMic.classList.toggle('active', mic);
            toggleMic.classList.toggle('inactive', !mic);
            toggleTrans.classList.toggle('active', trans);
            toggleTrans.classList.toggle('inactive', !trans);
        }};
        
        // Function to update current TTS speed from native side
        window.updateTtsSpeed = function(speed) {{
            ttsSpeed = speed;
            if (speedSlider) speedSlider.value = speed;
            if (speedValue) speedValue.textContent = (speed / 100).toFixed(1) + 'x';
        }};
        
        // Font size controls
        fontDecrease.addEventListener('click', function(e) {{
            e.stopPropagation();
            if (currentFontSize > 10) {{
                currentFontSize -= 2;
                content.style.fontSize = currentFontSize + 'px';
                // Reset min height so text can shrink properly
                minContentHeight = 0;
                content.style.minHeight = '';
                window.ipc.postMessage('fontSize:' + currentFontSize);
            }}
        }});
        
        fontIncrease.addEventListener('click', function(e) {{
            e.stopPropagation();
            if (currentFontSize < 32) {{
                currentFontSize += 2;
                content.style.fontSize = currentFontSize + 'px';
                // Reset min height for fresh calculation
                minContentHeight = 0;
                content.style.minHeight = '';
                window.ipc.postMessage('fontSize:' + currentFontSize);
            }}
        }});
        
        // Audio source toggle buttons
        const micBtn = document.getElementById('mic-btn');
        const deviceBtn = document.getElementById('device-btn');
        
        if (micBtn) {{
            micBtn.addEventListener('click', (e) => {{
                e.stopPropagation();
                e.preventDefault();
                
                // Switch to mic mode
                micBtn.classList.add('active');
                if (deviceBtn) deviceBtn.classList.remove('active');
                
                window.ipc.postMessage('audioSource:mic');
            }});
        }}
        
        if (deviceBtn) {{
            deviceBtn.addEventListener('click', (e) => {{
                e.stopPropagation();
                e.preventDefault();
                
                // Switch to device mode
                if (micBtn) micBtn.classList.remove('active');
                deviceBtn.classList.add('active');
                
                window.ipc.postMessage('audioSource:device');
            }});
        }}



        // Language Select Logic - show short code when collapsed, full name when open
        const langSelect = document.getElementById('language-select');
        if (langSelect) {{
            // Store original full names
            const options = langSelect.querySelectorAll('option');
            options.forEach(opt => {{
                opt.dataset.fullname = opt.textContent;
            }});
            
            // Function to show short codes (when collapsed)
            function showCodes() {{
                options.forEach(opt => {{
                    opt.textContent = opt.dataset.code || opt.dataset.fullname.substring(0, 2).toUpperCase();
                }});
            }}
            
            // Function to show full names (when dropdown open)
            function showFullNames() {{
                options.forEach(opt => {{
                    opt.textContent = opt.dataset.fullname;
                }});
            }}
            
            // Initially show codes
            showCodes();
            
            // Show full names when dropdown opens
            langSelect.addEventListener('focus', showFullNames);
            langSelect.addEventListener('mousedown', function(e) {{ 
                e.stopPropagation();
                showFullNames();
            }});
            
            // Show codes when dropdown closes
            langSelect.addEventListener('blur', showCodes);
            langSelect.addEventListener('change', function(e) {{
                e.stopPropagation();
                window.ipc.postMessage('language:' + this.value);
                // Delay to let the dropdown close animation finish
                setTimeout(showCodes, 100);
            }});
        }}

        // Model Toggle Switch Logic - query all model icons directly
        const modelIcons = document.querySelectorAll('.model-icon');
        if (modelIcons.length) {{
            modelIcons.forEach(icon => {{
                icon.addEventListener('click', (e) => {{
                    e.stopPropagation();
                    e.preventDefault();
                    
                    // Update UI - toggle active class
                    modelIcons.forEach(i => i.classList.remove('active'));
                    icon.classList.add('active');
                    
                    // Send IPC
                    const val = icon.getAttribute('data-value');
                    window.ipc.postMessage('translationModel:' + val);
                }});
            }});
        }}
        
        // Update settings from native side (used when overlay is shown with saved config)
        window.updateSettings = function(settings) {{
            // Update audio source toggle
            if (settings.audioSource && micBtn && deviceBtn) {{
                if (settings.audioSource === 'device') {{
                    micBtn.classList.remove('active');
                    deviceBtn.classList.add('active');
                }} else {{
                    micBtn.classList.add('active');
                    deviceBtn.classList.remove('active');
                }}
            }}
            
            // Update language select
            if (settings.targetLanguage && langSelect) {{
                langSelect.value = settings.targetLanguage;
            }}
            
            // Update translation model
            if (settings.translationModel && modelIcons.length) {{
                modelIcons.forEach(icon => {{
                    const val = icon.getAttribute('data-value');
                    icon.classList.toggle('active', val === settings.translationModel);
                }});
            }}
            
            // Update font size
            if (settings.fontSize && settings.fontSize !== currentFontSize) {{
                currentFontSize = settings.fontSize;
                content.style.fontSize = currentFontSize + 'px';
                minContentHeight = 0;
                content.style.minHeight = '';
            }}
        }};
        
        // Handle resize to keep text at bottom
        let lastWidth = viewport.clientWidth;
        const resizeObserver = new ResizeObserver(entries => {{
            for (let entry of entries) {{
                if (Math.abs(entry.contentRect.width - lastWidth) > 5) {{
                    lastWidth = entry.contentRect.width;
                    // Reset min height on width change (reflow)
                    minContentHeight = 0;
                    content.style.minHeight = '';
                    
                    // Force scroll to bottom immediately to prevent jump
                    if (content.scrollHeight > viewport.clientHeight) {{
                        viewport.scrollTop = content.scrollHeight - viewport.clientHeight;
                    }}
                    targetScrollTop = viewport.scrollTop;
                    currentScrollTop = targetScrollTop;
                }}
            }}
        }});
        resizeObserver.observe(viewport);
        
        let isFirstText = true;
        let currentScrollTop = 0;
        let targetScrollTop = 0;
        let animationFrame = null;
        let minContentHeight = 0;
        
        function animateScroll() {{
            const diff = targetScrollTop - currentScrollTop;
            
            if (Math.abs(diff) > 0.5) {{
                const ease = Math.min(0.08, Math.max(0.02, Math.abs(diff) / 1000));
                currentScrollTop += diff * ease;
                viewport.scrollTop = currentScrollTop;
                animationFrame = requestAnimationFrame(animateScroll);
            }} else {{
                currentScrollTop = targetScrollTop;
                viewport.scrollTop = currentScrollTop;
                animationFrame = null;
            }}
        }}
        
        let currentOldTextLength = 0;
        let previousNewText = '';
"###,
        font_size = font_size,
        check_svg = crate::overlay::html_components::icons::get_icon_svg("check"),
        copy_svg = crate::overlay::html_components::icons::get_icon_svg("content_copy")
    )
}
