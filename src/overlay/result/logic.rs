use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use super::state::{WINDOW_STATES, AnimationMode, DustParticle};

fn rand_float(min: f32, max: f32) -> f32 {
    static mut SEED: u32 = 12345;
    unsafe {
        SEED = SEED.wrapping_mul(1103515245).wrapping_add(12345);
        let norm = (SEED as f32) / (u32::MAX as f32);
        min + norm * (max - min)
    }
}

pub fn handle_timer(hwnd: HWND, wparam: WPARAM) {
    unsafe {
        if wparam.0 == 3 { // 60 FPS Physics Loop
            let mut should_close = false;
            
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    let p = &mut state.physics;

                    // --- 1. MOUSE PHYSICS (Spring System) ---
                    // Hooke's Law for Handle Tilt:
                    // Force = -k * x - c * v
                    // k = stiffness, c = damping
                    
                    // Natural wobble rest point is 0.0
                    let spring_stiffness = 0.15;
                    let damping = 0.85;
                    
                    p.tilt_velocity += (0.0 - p.current_tilt) * spring_stiffness;
                    p.tilt_velocity *= damping;
                    p.current_tilt += p.tilt_velocity;

                    // Bristle bend follows tilt but lags slightly
                    p.bristle_bend = p.bristle_bend * 0.8 + (p.current_tilt / 10.0) * 0.2;

                    // --- 2. ANIMATION STATE MACHINE ---
                    match p.mode {
                        AnimationMode::Idle => {
                            p.squish_factor = p.squish_factor * 0.9 + 1.0 * 0.1; // Return to 1.0
                        },
                        AnimationMode::Smashing => {
                             // SIMPLIFIED: Skip the broom animation entirely
                             // Just transition directly to fade-out (DragOut) mode
                             // This eliminates all freezing frame issues
                             p.mode = AnimationMode::DragOut;
                             p.particles.clear(); // No particles
                             p.state_timer = 0.0;
                         },
                        AnimationMode::DragOut => {
                            p.state_timer += 1.0;
                            p.squish_factor = p.squish_factor * 0.8 + 1.2 * 0.2; // Stretch up
                            
                            // FAST fade out - 25 alpha per frame at 60 FPS = ~170ms fade
                            if state.alpha > 25 {
                                state.alpha = state.alpha.saturating_sub(25);
                                SetLayeredWindowAttributes(hwnd, COLORREF(0), state.alpha, LWA_ALPHA);
                            } else {
                                should_close = true;
                            }
                        }
                    }

                    // --- 3. PARTICLE PHYSICS ---
                    let mut keep = Vec::new();
                    for mut pt in p.particles.drain(..) {
                        pt.x += pt.vx;
                        pt.y += pt.vy;
                        pt.vy += 0.5; // Gravity
                        pt.vx *= 0.92; // Air resistance
                        pt.life -= 0.03;
                        if pt.life > 0.0 { keep.push(pt); }
                    }
                    p.particles = keep;

                    // PERFORMANCE FIX: Skip repaints during DragOut EXCEPT for the cleanup repaint
                    // The cleanup repaint clears the broom/particles from the visual
                    let skip_repaint = matches!(p.mode, AnimationMode::DragOut) && !p.needs_cleanup_repaint;
                    if p.needs_cleanup_repaint {
                        p.needs_cleanup_repaint = false; // Consume the flag
                    }
                    if !skip_repaint {
                        InvalidateRect(hwnd, None, false);
                    }
                }
            }

            if should_close {
                // CRITICAL: Set alpha to 0 BEFORE closing to prevent last frame freeze
                SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
                
                let linked_hwnd = {
                    let states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get(&(hwnd.0 as isize)) { state.linked_window } else { None }
                };
                if let Some(linked) = linked_hwnd {
                    if IsWindow(linked).as_bool() { 
                        // Also set linked window to invisible
                        SetLayeredWindowAttributes(linked, COLORREF(0), 0, LWA_ALPHA);
                        PostMessageW(linked, WM_CLOSE, WPARAM(0), LPARAM(0)); 
                    }
                }
                PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        } 
        else if wparam.0 == 1 {
            // Revert Copy Icon
            KillTimer(hwnd, 1);
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) { 
                state.copy_success = false; 
                
                // Spawn sparkles for success
                 let cx = state.physics.x;
                 let cy = state.physics.y;
                 for _ in 0..8 {
                    state.physics.particles.push(DustParticle {
                        x: cx + rand_float(-10.0, 10.0),
                        y: cy,
                        vx: rand_float(-2.0, 2.0),
                        vy: rand_float(-2.0, -5.0),
                        life: 1.0,
                        size: rand_float(1.0, 3.0),
                        color: 0xFF00FF00, // Green sparkles
                    });
                }
            }
            InvalidateRect(hwnd, None, false);
        }
    }
}
