pub fn get(placeholder_text: &str) -> String { format!(r###"        function updateText(oldText, newText) {{
            const hasContent = oldText || newText;
            
            if (isFirstText && hasContent) {{
                content.innerHTML = '';
                isFirstText = false;
                minContentHeight = 0;
                currentOldTextLength = 0;
                previousNewText = '';
            }}
            
            if (!hasContent) {{
                content.innerHTML = '<span class="placeholder">{placeholder_text}</span>';
                content.style.minHeight = '';
                isFirstText = true;
                minContentHeight = 0;
                targetScrollTop = 0;
                currentScrollTop = 0;
                viewport.scrollTop = 0;
                currentOldTextLength = 0;
                previousNewText = '';
                return;
            }}

            // Detect if newText was REPLACED (not extended)  
            // This happens when new translation starts - must do atomic rebuild
            const isNewTextReplacement = previousNewText.length > 0 && 
                newText.length > 0 && 
                !newText.startsWith(previousNewText);
            
            // 1. Handle history rewrite or shrink
            if (oldText.length < currentOldTextLength) {{
                content.innerHTML = '';
                currentOldTextLength = 0;
                previousNewText = '';
            }}
            
            // Get all existing chunks
            const allChunks = Array.from(content.querySelectorAll('.text-chunk'));
            let totalChunkText = allChunks.map(c => c.textContent).join('');
            const fullText = oldText + newText;
            
            // 2. If old text grew, transition chunks from new to old
            if (oldText.length > currentOldTextLength) {{
                let committedLen = oldText.length;
                let accumulatedLen = 0;
                
                for (const chunk of allChunks) {{
                    const chunkLen = chunk.textContent.length;
                    const chunkEnd = accumulatedLen + chunkLen;
                    
                    // If this chunk falls within committed range, mark as old
                    if (chunkEnd <= committedLen) {{
                        if (!chunk.classList.contains('old')) {{
                            chunk.classList.remove('appearing', 'new');
                            chunk.classList.add('old');
                        }}
                    }}
                    accumulatedLen = chunkEnd;
                }}
            }}
            currentOldTextLength = oldText.length;
            previousNewText = newText;
            
            // 3. Handle text changes
            // Priority: replacement detection > append > general rebuild
            if (isNewTextReplacement) {{
                // Atomic replacement: rebuild with new content immediately
                content.innerHTML = '';
                if (oldText) {{
                    const oldChunk = document.createElement('span');
                    oldChunk.className = 'text-chunk old';
                    oldChunk.textContent = oldText;
                    content.appendChild(oldChunk);
                }}
                if (newText) {{
                    const newChunk = document.createElement('span');
                    newChunk.className = 'text-chunk new';
                    newChunk.textContent = newText;
                    content.appendChild(newChunk);
                }}
            }} else if (fullText.length > totalChunkText.length && fullText.startsWith(totalChunkText)) {{
                // Normal append mode - text grew
                const delta = fullText.substring(totalChunkText.length);
                
                const chunk = document.createElement('span');
                chunk.className = 'text-chunk appearing';
                chunk.textContent = delta;
                content.appendChild(chunk);
                
                // Trigger wipe animation
                requestAnimationFrame(() => {{
                    chunk.classList.add('show');
                    setTimeout(() => {{
                        chunk.classList.remove('appearing', 'show');
                        const chunkStart = totalChunkText.length;
                        if (chunkStart < currentOldTextLength) {{
                            chunk.classList.add('old');
                        }} else {{
                            chunk.classList.add('new');
                        }}
                    }}, 350);
                }});
            }} else if (fullText !== totalChunkText) {{
                // General rebuild for other cases
                content.innerHTML = '';
                if (oldText) {{
                    const oldChunk = document.createElement('span');
                    oldChunk.className = 'text-chunk old';
                    oldChunk.textContent = oldText;
                    content.appendChild(oldChunk);
                }}
                if (newText) {{
                    const newChunk = document.createElement('span');
                    newChunk.className = 'text-chunk new';
                    newChunk.textContent = newText;
                    content.appendChild(newChunk);
                }}
            }}
            
            // Scroll logic
            const naturalHeight = content.offsetHeight;
            if (naturalHeight > minContentHeight) {{
                minContentHeight = naturalHeight;
            }}
            content.style.minHeight = minContentHeight + 'px';
            const viewportHeight = viewport.offsetHeight;
            if (minContentHeight > viewportHeight) {{
                const maxScroll = minContentHeight - viewportHeight;
                if (maxScroll > targetScrollTop) {{
                    targetScrollTop = maxScroll;
                }}
            }}
            if (!animationFrame) {{
                animationFrame = requestAnimationFrame(animateScroll);
            }}
        }}

        window.updateText = updateText;
        
        // Canvas-based volume visualizer - cute pill bars scrolling left
        const volumeCanvas = document.getElementById('volume-canvas');
        const volumeCtx = volumeCanvas ? volumeCanvas.getContext('2d') : null;
        
        // Cute pill configuration
        const BAR_WIDTH = 4;
        const BAR_GAP = 3;
        const BAR_SPACING = BAR_WIDTH + BAR_GAP;
        const VISIBLE_BARS = 12;
        
        // Each bar has its own height that persists as it scrolls
        const barHeights = new Array(VISIBLE_BARS + 2).fill(3);
        let latestRMS = 0;
        let scrollProgress = 0; // 0 to 1, represents progress to next bar shift
        let lastTime = 0;
        
        function updateVolume(rms) {{
            latestRMS = rms;
        }}
        
        function drawWaveform(timestamp) {{
            if (!volumeCtx) return;
            
            // Delta time
            const dt = lastTime ? (timestamp - lastTime) / 1000 : 0.016;
            lastTime = timestamp;
            
            // Scroll progress (one full bar every ~200ms for relaxed look)
            scrollProgress += dt / 0.2;
            
            // When we've scrolled one full bar, shift and add new
            while (scrollProgress >= 1) {{
                scrollProgress -= 1;
                // Shift all bars left (oldest falls off)
                barHeights.shift();
                // Add new bar on right with current RMS
                const h = volumeCanvas.height;
                // RMS typically 0-0.3 for speech, multiply by 180 for better visibility
                const newHeight = Math.max(3, Math.min(h - 2, latestRMS * 180 + 3));
                barHeights.push(newHeight);
            }}
            
            // Clear
            const w = volumeCanvas.width;
            const h = volumeCanvas.height;
            volumeCtx.clearRect(0, 0, w, h);
            
            // Gradient
            const grad = volumeCtx.createLinearGradient(0, h, 0, 0);
            grad.addColorStop(0, '#00a8e0');
            grad.addColorStop(0.5, '#00c8ff');
            grad.addColorStop(1, '#40e0ff');
            volumeCtx.fillStyle = grad;
            
            // Pixel offset for smooth scroll
            const pixelOffset = scrollProgress * BAR_SPACING;
            
            // Draw bars
            for (let i = 0; i < barHeights.length; i++) {{
                const pillHeight = barHeights[i];
                const x = i * BAR_SPACING - pixelOffset;
                const y = (h - pillHeight) / 2;
                
                if (x > -BAR_WIDTH && x < w) {{
                    volumeCtx.beginPath();
                    volumeCtx.roundRect(x, y, BAR_WIDTH, pillHeight, BAR_WIDTH / 2);
                    volumeCtx.fill();
                }}
            }}
            
            requestAnimationFrame(drawWaveform);
        }}
        
        // Start animation
        if (volumeCanvas) {{
            requestAnimationFrame(drawWaveform);
        }}
        
        window.updateVolume = updateVolume;
        
        // Model switch animation (called when 429 fallback switches models)
        function switchModel(modelName) {{
            const icons = document.querySelectorAll('.model-icon');
            if (!icons.length) return;
            
            icons.forEach(icon => {{
                const val = icon.getAttribute('data-value');
                const shouldBeActive = val === modelName;
                
                // Update active state
                icon.classList.remove('active');
                if (shouldBeActive) {{
                    icon.classList.add('active');
                    // Add switching animation
                    icon.classList.add('switching');
                    // Remove animation class after it completes (2s)
                    setTimeout(() => icon.classList.remove('switching'), 2000);
                }}
            }});
        }}
        
        window.switchModel = switchModel;"###, placeholder_text=placeholder_text) }
