use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use super::state::{WINDOW_STATES, AnimationMode, DustParticle};

// Helper for RNG
fn rand() -> u32 {
    static mut SEED: u32 = 98765;
    unsafe {
        SEED = SEED.wrapping_mul(1103515245).wrapping_add(12345);
        (SEED / 65536) % 32768
    }
}

pub fn handle_timer(hwnd: HWND, wparam: WPARAM) {
    unsafe {
        if wparam.0 == 3 { // Animation Loop (16ms)
            let mut should_close = false;
            
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    state.physics.frame_timer += 1;
                    
                    // 1. Idle Animation (Bob every 20 frames)
                    if state.physics.mode == AnimationMode::Idle {
                        if state.physics.frame_timer > 20 {
                            state.physics.idle_frame = (state.physics.idle_frame + 1) % 2;
                            state.physics.frame_timer = 0;
                        }
                    }

                    // 2. Sweep Animation Controller
                    if state.physics.mode == AnimationMode::Sweeping {
                        // Fade out logic 
                        if state.physics.sweep_stage >= 1 {
                            state.alpha = state.alpha.saturating_sub(15);
                            SetLayeredWindowAttributes(hwnd, COLORREF(0), state.alpha, LWA_ALPHA);
                        }

                        // Frame duration
                        if state.physics.frame_timer > 3 {
                            state.physics.sweep_stage += 1;
                            state.physics.frame_timer = 0;

                            // Trigger Dust on Smash (Stage 1)
                            if state.physics.sweep_stage == 1 {
                                let cx = state.physics.x;
                                let cy = state.physics.y + 26.0; 
                                for _ in 0..12 {
                                    state.physics.particles.push(DustParticle {
                                        x: cx + (rand() % 20) as f32 - 10.0,
                                        y: cy,
                                        vx: (rand() % 10) as f32 - 5.0,
                                        vy: -((rand() % 6) as f32 + 2.0),
                                        life: 1.0,
                                        _color: 0xFFDDDDDD,
                                    });
                                }
                            }
                            
                            if state.physics.sweep_stage > 4 || state.alpha == 0 {
                                should_close = true;
                            }
                        }
                    }

                    // 3. Update Particles
                    let mut keep = Vec::new();
                    for mut p in state.physics.particles.drain(..) {
                        p.x += p.vx;
                        p.y += p.vy;
                        p.vy += 0.4; // Gravity
                        p.life -= 0.1;
                        if p.life > 0.0 { keep.push(p); }
                    }
                    state.physics.particles = keep;

                    InvalidateRect(hwnd, None, false);
                }
            }

            if should_close {
                 let linked_hwnd = {
                    let states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get(&(hwnd.0 as isize)) { state.linked_window } else { None }
                };
                if let Some(linked) = linked_hwnd {
                    if IsWindow(linked).as_bool() { PostMessageW(linked, WM_CLOSE, WPARAM(0), LPARAM(0)); }
                }
                PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        } 
        else if wparam.0 == 1 {
            // Revert Copy Icon
            KillTimer(hwnd, 1);
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) { state.copy_success = false; }
            InvalidateRect(hwnd, None, false);
        }
    }
}
