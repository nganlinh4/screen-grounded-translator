use eframe::egui;
use eframe::egui::{Color32, Pos2, Rect, Vec2, FontId, Align2, Stroke};
use std::f32::consts::PI;

// --- CONFIGURATION ---
const ANIMATION_DURATION: f32 = 6.0;
const FADE_OUT_START: f32 = 5.2;

// --- PALETTE (Optical Clarity Theme) ---
const C_BG: Color32 = Color32::from_rgb(8, 10, 15);           // Deep Void
const C_LENS_RING: Color32 = Color32::from_rgb(0, 255, 200);  // Teal Focus Ring
const C_LOGO: Color32 = Color32::from_rgb(220, 240, 255);     // Crystalline White
const C_ABERRATION_R: Color32 = Color32::from_rgb(255, 50, 50); // Glitch Red
const C_ABERRATION_B: Color32 = Color32::from_rgb(50, 50, 255); // Glitch Blue
const C_SCAN_BEAM: Color32 = Color32::from_rgb(100, 255, 150); // Grounding Beam

// --- 3D MATH KERNEL ---
#[derive(Clone, Copy, Debug, PartialEq)]
struct Vec3 { x: f32, y: f32, z: f32 }

impl Vec3 {
    const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    fn new(x: f32, y: f32, z: f32) -> Self { Self { x, y, z } }

    fn add(self, v: Vec3) -> Self { Self::new(self.x + v.x, self.y + v.y, self.z + v.z) }
    fn mul(self, s: f32) -> Self { Self::new(self.x * s, self.y * s, self.z * s) }

    fn rotate_y(self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x * c + self.z * s, self.y, -self.x * s + self.z * c)
    }

    fn rotate_x(self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x, self.y * c - self.z * s, self.y * s + self.z * c)
    }

    fn rotate_z(self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x * c - self.y * s, self.x * s + self.y * c, self.z)
    }

    // Returns (ScreenPos, Scale, Z-Depth)
    fn project(self, center: Pos2, fov: f32, cam_z: f32) -> Option<(Pos2, f32, f32)> {
        let z_depth = cam_z - self.z;
        if z_depth <= 1.0 { return None; } 
        let scale = fov / z_depth;
        let x = center.x + self.x * scale;
        let y = center.y - self.y * scale; 
        Some((Pos2::new(x, y), scale, z_depth))
    }
}

// --- EASING ---
fn ease_out_elastic(x: f32) -> f32 {
    let c4 = (2.0 * PI) / 3.0;
    if x == 0.0 { 0.0 } else if x == 1.0 { 1.0 }
    else { 2.0f32.powf(-10.0 * x) * ((x * 10.0 - 0.75) * c4).sin() + 1.0 }
}

fn ease_in_out_cubic(x: f32) -> f32 {
    if x < 0.5 { 4.0 * x * x * x } else { 1.0 - (-2.0 * x + 2.0).powi(3) / 2.0 }
}

// --- PARTICLE SYSTEM ---
#[derive(PartialEq)]
enum PType {
    LogoVoxel,  // Part of the SGT text
    LensRing,   // Part of the spinning focus ring
    Dust,       // Background ambience
}

struct Particle {
    // Spatial
    target_pos: Vec3, // Where it belongs in the formed logo/ring
    start_pos: Vec3,  // Where it spawns (chaos)
    current_pos: Vec3,
    
    // Properties
    ptype: PType,
    size: f32,
    phase: f32, // For individual noise
}

pub struct SplashScreen {
    start_time: f64,
    particles: Vec<Particle>,
    init_done: bool,
    
    // Camera Animation State
    cam_dist: f32,
    cam_rot: Vec3,
    aberration_strength: f32, // How much RGB split
    focus_val: f32,           // 0.0 = blurry chaos, 1.0 = sharp lock
}

pub enum SplashStatus {
    Ongoing,
    Finished,
}

impl SplashScreen {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            start_time: ctx.input(|i| i.time),
            particles: Vec::with_capacity(1500),
            init_done: false,
            cam_dist: 1500.0,
            cam_rot: Vec3::ZERO,
            aberration_strength: 20.0,
            focus_val: 0.0,
        }
    }

    fn init_scene(&mut self) {
        let mut rng_state = 8888u64;
        let mut rng = || {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (rng_state >> 32) as f32 / 4294967295.0
        };

        // 1. GENERATE "SGT" VOXELS (Perfectly Centered)
        let spacing = 14.0;
        
        // 5x7 Font Maps (Height 7, Width 5)
        let s_map = [
            0,1,1,1,0,
            1,0,0,0,1,
            1,0,0,0,0,
            0,1,1,1,0,
            0,0,0,0,1,
            1,0,0,0,1,
            0,1,1,1,0
        ];
        let g_map = [
            0,1,1,1,0,
            1,0,0,0,1,
            1,0,0,0,0,
            1,0,1,1,1,
            1,0,0,0,1,
            1,0,0,0,1,
            0,1,1,1,0
        ];
        let t_map = [
            1,1,1,1,1,
            0,0,1,0,0,
            0,0,1,0,0,
            0,0,1,0,0,
            0,0,1,0,0,
            0,0,1,0,0,
            0,0,1,0,0
        ];

        let mut logo_voxels = Vec::new();
        let total_w_units = 19.0;
        let total_h_units = 7.0;
        
        let offset_x = -(total_w_units * spacing) / 2.0; 
        let offset_y = (total_h_units * spacing) / 2.0;

        // Helper to spawn voxels inline
        let spawn_letter = |map: &[i32], start_col_idx: f32, rng_fn: &mut dyn FnMut() -> f32, out: &mut Vec<Vec3>| {
            for row in 0..7 {
                for col in 0..5 {
                    if map[row * 5 + col] == 1 {
                        for _ in 0..2 {
                            let tx = offset_x + ((start_col_idx + col as f32) * spacing) + rng_fn() * 2.0 - 1.0;
                            let ty = offset_y - (row as f32 * spacing) + rng_fn() * 2.0 - 1.0;
                            let tz = rng_fn() * 8.0 - 4.0;
                            
                            out.push(Vec3::new(tx, ty, tz));
                        }
                    }
                }
            }
        };

        spawn_letter(&s_map, 0.0, &mut rng, &mut logo_voxels);
        spawn_letter(&g_map, 7.0, &mut rng, &mut logo_voxels);
        spawn_letter(&t_map, 14.0, &mut rng, &mut logo_voxels);

        for target in logo_voxels {
            let ex_x = rng() * 2.0 - 1.0;
            let ex_y = rng() * 2.0 - 1.0;
            let ex_z = rng() * 2.0 - 1.0;
            let explode_dir = Vec3::new(ex_x, ex_y, ex_z);
            self.particles.push(Particle {
                target_pos: target,
                start_pos: target.add(explode_dir.mul(800.0)),
                current_pos: Vec3::ZERO,
                ptype: PType::LogoVoxel,
                size: 2.0 + rng() * 1.5,
                phase: rng(),
            });
        }

        // 2. GENERATE FOCUS RING
        let ring_count = 300;
        for i in 0..ring_count {
            let angle = (i as f32 / ring_count as f32) * PI * 2.0;
            let r = 220.0;
            let x = r * angle.cos();
            let y = r * angle.sin();
            
            self.particles.push(Particle {
                target_pos: Vec3::new(x, y, 0.0),
                start_pos: Vec3::new(x * 5.0, y * 5.0, rng() * 2000.0 - 1000.0),
                current_pos: Vec3::ZERO,
                ptype: PType::LensRing,
                size: 1.0 + rng() * 1.5,
                phase: rng(),
            });
        }

        // 3. AMBIENT DUST
        for _ in 0..400 {
            let x = rng() * 2000.0 - 1000.0;
            let y = rng() * 1600.0 - 800.0;
            let z = rng() * 1500.0 - 1000.0;
            let pos = Vec3::new(x, y, z);
            self.particles.push(Particle {
                target_pos: pos, 
                start_pos: pos,
                current_pos: pos,
                ptype: PType::Dust,
                size: 0.5 + rng() * 1.0,
                phase: rng(),
            });
        }

        self.init_done = true;
    }

    pub fn update(&mut self, ctx: &egui::Context) -> SplashStatus {
        if !self.init_done { self.init_scene(); }

        let now = ctx.input(|i| i.time);
        let t = (now - self.start_time) as f32;
        
        if t > ANIMATION_DURATION {
            return SplashStatus::Finished;
        }
        ctx.request_repaint();

        // --- TIMELINE ---
        // 0.0 - 1.5: Gathering. Lens spins up. Logo is dispersed cloud. High Aberration.
        // 1.5 - 2.5: The Focus. Ring snaps tight. Logo flies to center. Aberration reduces.
        // 2.5 - 3.0: The Lock. Logo solidifies. Aberration -> 0.
        // 3.0 - 5.0: The Scan & Hover. Gentle movement.

        // 1. Calculate Global Animation Values
        if t < 2.5 {
            let progress = (t / 2.5).clamp(0.0, 1.0);
            self.focus_val = ease_in_out_cubic(progress);
            self.aberration_strength = 20.0 * (1.0 - self.focus_val);
            self.cam_dist = 1500.0 - (self.focus_val * 600.0); // Zoom in: 1500 -> 900
            
            // Camera chaotic shake before lock
            let shake = (1.0 - self.focus_val) * 0.3;
            self.cam_rot.z = (t * 10.0).sin() * shake;
            self.cam_rot.y = (t * 7.0).cos() * shake;
        } else {
            // Stabilized
            self.focus_val = 1.0;
            self.aberration_strength = 0.0;
            // Gentle hover
            self.cam_rot.z = 0.0;
            self.cam_rot.y = ((t - 2.5) * 0.5).sin() * 0.05;
            self.cam_rot.x = ((t - 2.5) * 0.3).cos() * 0.05;
        }

        // 2. Update Particles
        for p in &mut self.particles {
            match p.ptype {
                PType::LogoVoxel => {
                    if t < 2.5 {
                        // Lerp from start to target
                        let local_t = (t / 2.5).clamp(0.0, 1.0);
                        let ease = ease_out_elastic(local_t);
                        
                        // Add some spiral motion while converging
                        let spiral_radius = 200.0 * (1.0 - ease);
                        let spiral_angle = t * 5.0 + p.phase * 10.0;
                        let spiral_offset = Vec3::new(spiral_angle.cos() * spiral_radius, spiral_angle.sin() * spiral_radius, 0.0);
                        
                        p.current_pos = p.start_pos.mul(1.0 - ease).add(p.target_pos.mul(ease)).add(spiral_offset);
                    } else {
                        // Locked in place + subtle float
                        p.current_pos = p.target_pos;
                    }
                },
                PType::LensRing => {
                    // Ring spins constantly, but radius shrinks to target
                    let current_radius_factor = if t < 2.5 {
                         2.0 - ease_in_out_cubic(t / 2.5)
                    } else {
                        1.0
                    };
                    
                    let spin_speed = if t < 2.5 { 5.0 } else { 1.0 };
                    let angle = t * spin_speed + p.phase * 10.0;
                    
                    // Rotate the target position around Z axis
                    let x = p.target_pos.x * angle.cos() - p.target_pos.y * angle.sin();
                    let y = p.target_pos.x * angle.sin() + p.target_pos.y * angle.cos();
                    
                    p.current_pos = Vec3::new(x * current_radius_factor, y * current_radius_factor, p.target_pos.z);
                    
                    // Tilt the ring slightly in 3D
                    p.current_pos = p.current_pos.rotate_x(0.3).rotate_y(0.2);
                },
                PType::Dust => {
                    // Just drift z-wards
                    let speed = 50.0;
                    let z = p.start_pos.z + (t * speed);
                    // Wrap around
                    let z_mod = ((z + 1000.0) % 2000.0) - 1000.0;
                    p.current_pos = Vec3::new(p.start_pos.x, p.start_pos.y, z_mod);
                }
            }
        }

        // Render
        egui::CentralPanel::default().show(ctx, |ui| {
            self.paint(ui, t);
        });

        SplashStatus::Ongoing
    }

    fn paint(&self, ui: &mut egui::Ui, t: f32) {
        let painter = ui.painter();
        let rect = ui.max_rect();
        let center = rect.center();

        // 1. Background (Fade out logic)
        let alpha_mult = if t > FADE_OUT_START {
            1.0 - ((t - FADE_OUT_START) * 2.0).clamp(0.0, 1.0)
        } else { 1.0 };
        
        // Deep space background
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha((255.0 * alpha_mult) as u8));
        let bg_vignette = C_BG.linear_multiply(0.5 * alpha_mult);
        painter.rect_filled(rect, 0.0, bg_vignette);

        if alpha_mult <= 0.01 { return; }

        // 2. Projection & Z-Sort
        let mut draw_list: Vec<(f32, Pos2, f32, Color32, &Particle)> = Vec::with_capacity(self.particles.len());

        for p in &self.particles {
            // Apply Camera
            let view_pos = p.current_pos.rotate_y(self.cam_rot.y).rotate_x(self.cam_rot.x).rotate_z(self.cam_rot.z);
            
            if let Some((screen_pos, scale, z)) = view_pos.project(center, 900.0, self.cam_dist) {
                // Determine Color & Alpha
                let mut col = match p.ptype {
                    PType::LogoVoxel => C_LOGO,
                    PType::LensRing => C_LENS_RING,
                    PType::Dust => Color32::from_gray(100),
                };

                let mut alpha = match p.ptype {
                    PType::LogoVoxel => 1.0,
                    PType::LensRing => 0.6,
                    PType::Dust => 0.3,
                };

                // Distance fog
                let fog = (1.0 - (z / 2000.0).abs()).clamp(0.0, 1.0);
                alpha *= fog * alpha_mult;

                col = col.linear_multiply(alpha);
                draw_list.push((z, screen_pos, scale, col, p));
            }
        }

        // Sort (Back to Front)
        draw_list.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // 3. Draw Loop
        for (_, pos, scale, col, p) in draw_list {
            let size = p.size * scale;
            if size < 0.3 { continue; }

            // CHROMATIC ABERRATION EFFECT (Simulating the blur/focus)
            if self.aberration_strength > 0.5 && p.ptype != PType::Dust {
                let offset = self.aberration_strength * scale;
                // Red Channel
                painter.circle_filled(pos + Vec2::new(offset, 0.0), size, C_ABERRATION_R.linear_multiply(0.5));
                // Blue Channel
                painter.circle_filled(pos - Vec2::new(offset, 0.0), size, C_ABERRATION_B.linear_multiply(0.5));
            }

            match p.ptype {
                PType::LogoVoxel => {
                    // Sharp squares for digital voxel look
                    painter.rect_filled(Rect::from_center_size(pos, Vec2::splat(size * 1.8)), 1.0, col);
                },
                PType::LensRing => {
                    // Glowing dots
                    painter.circle_filled(pos, size, col);
                    // Bloom
                    painter.circle_filled(pos, size * 3.0, col.linear_multiply(0.1));
                },
                PType::Dust => {
                    painter.circle_filled(pos, size, col);
                }
            }
        }

        // 4. Grounding Scan Effect (Post-Lock)
        if t > 2.8 && t < 4.5 {
            let scan_t = ((t - 2.8) / 1.0).clamp(0.0, 1.0); // 0 to 1 over 1 sec
            if scan_t > 0.0 {
                let y_level = rect.top() + (rect.height() * scan_t);
                
                // Draw Horizontal Beam
                painter.line_segment(
                    [Pos2::new(rect.left(), y_level), Pos2::new(rect.right(), y_level)],
                    Stroke::new(2.0, C_SCAN_BEAM.linear_multiply(0.5 * alpha_mult))
                );
                
                // Draw "Grounding Grid" appearing below beam
                if scan_t > 0.5 {
                    let opacity = (scan_t - 0.5) * 2.0 * alpha_mult;
                    let grid_col = C_SCAN_BEAM.linear_multiply(0.1 * opacity);
                    let floor_y = center.y + 100.0;
                    
                    if y_level > floor_y {
                        painter.line_segment(
                            [Pos2::new(center.x - 100.0, floor_y), Pos2::new(center.x + 100.0, floor_y)],
                            Stroke::new(1.0, grid_col)
                        );
                        painter.text(
                            Pos2::new(center.x, floor_y + 10.0),
                            Align2::CENTER_TOP,
                            "SGT - nganlinh4",
                            FontId::monospace(10.0),
                            grid_col.linear_multiply(2.0) // Brighter text
                        );
                    }
                }
            }
        }

        // 5. Title Text
        if t > 3.0 {
            let opacity = ((t - 3.0) * 2.0).clamp(0.0, 1.0) * alpha_mult;
            if opacity > 0.0 {
                painter.text(
                    center + Vec2::new(0.0, 140.0),
                    Align2::CENTER_CENTER,
                    "Screen Grounded Translator",
                    FontId::proportional(20.0),
                    Color32::WHITE.linear_multiply(opacity)
                );
            }
        }
    }
}
