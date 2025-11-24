use eframe::egui;
use eframe::egui::{Color32, Pos2, Rect, Stroke, Vec2, FontId, Align2};

// --- CONFIGURATION ---
const ANIMATION_DURATION: f32 = 5.5;
const FADE_OUT_START: f32 = 4.8;

// Palette (High Contrast Tech)
const COL_RAW_DATA: Color32 = Color32::from_rgb(60, 70, 80);      // Gray noise
const COL_SCAN_BEAM: Color32 = Color32::from_rgb(0, 255, 180);    // Cyan beam
const COL_ANALYZED: Color32 = Color32::from_rgb(0, 200, 255);     // Blue processed
const COL_FINAL_TEXT: Color32 = Color32::from_rgb(255, 255, 255); // White result
const COL_GLOW: Color32 = Color32::from_rgb(0, 100, 150);

// --- 3D MATH ENGINE ---
#[derive(Clone, Copy, Debug, PartialEq)]
struct Vec3 { x: f32, y: f32, z: f32 }

impl Vec3 {
    const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    fn new(x: f32, y: f32, z: f32) -> Self { Self { x, y, z } }
    
    fn add(self, other: Self) -> Self { Self::new(self.x + other.x, self.y + other.y, self.z + other.z) }
    fn sub(self, other: Self) -> Self { Self::new(self.x - other.x, self.y - other.y, self.z - other.z) }
    fn mul(self, s: f32) -> Self { Self::new(self.x * s, self.y * s, self.z * s) }
    
    fn rotate_y(self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x * c + self.z * s, self.y, -self.x * s + self.z * c)
    }
    
    fn rotate_x(self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x, self.y * c - self.z * s, self.y * s + self.z * c)
    }

    // 3D to 2D Projection
    fn project(self, screen_center: Pos2, fov_scale: f32) -> (Pos2, f32) {
        // Simple weak perspective: z=0 is screen plane. Negative Z is far.
        // We push Z back by fixed distance to simulate camera offset
        let cam_dist = 1000.0;
        let z_depth = cam_dist - self.z;
        if z_depth <= 1.0 { return (screen_center, 0.0); } // Clip behind camera
        
        let scale = fov_scale / z_depth;
        let x = screen_center.x + self.x * scale;
        let y = screen_center.y - self.y * scale; // Y-up in 3D, Y-down in screen
        (Pos2::new(x, y), scale)
    }
}

// --- DATA STRUCTURES ---
#[derive(Clone, Copy)]
enum ParticleType {
    Pixel,  // Part of the raw screen data
    Glyph,  // Part of the final text
}

struct Particle {
    current_pos: Vec3,
    target_pos: Vec3,  // Where it wants to be (based on phase)
    velocity: Vec3,
    
    p_type: ParticleType,
    processed: bool,   // Has the scanner hit it?
    
    // For text reconstruction
    text_char: char,   // If Glyph
    grid_idx: usize,   // If Pixel
}

pub struct SplashScreen {
    start_time: f64,
    particles: Vec<Particle>,
    init_done: bool,
    
    // Camera State
    cam_rot_y: f32,
    cam_zoom: f32,
}

pub enum SplashStatus {
    Ongoing,
    Finished,
}

impl SplashScreen {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            start_time: ctx.input(|i| i.time),
            particles: Vec::new(),
            init_done: false,
            cam_rot_y: 0.0,
            cam_zoom: 0.0,
        }
    }

    fn init_scene(&mut self) {
        let mut rng_seed = 12345u64;
        let mut rand = || {
            rng_seed = rng_seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (rng_seed >> 32) as f32 / 4294967295.0
        };
        let mut rand_range = |min: f32, max: f32| min + rand() * (max - min);

        // 1. Generate "Screen Grid" Particles (The Raw Data)
        // A 3D plane of dots representing pixels
        let rows = 20;
        let cols = 40;
        let spacing = 12.0;
        let offset_x = (cols as f32 * spacing) / 2.0;
        let offset_y = (rows as f32 * spacing) / 2.0;

        for y in 0..rows {
            for x in 0..cols {
                // Target position in the "Screen" formation
                let tx = (x as f32 * spacing) - offset_x;
                let ty = (y as f32 * spacing) - offset_y;
                let tz = 0.0;

                // Start position: Random chaos far away
                let sx = rand_range(-800.0, 800.0);
                let sy = rand_range(-800.0, 800.0);
                let sz = rand_range(-1000.0, -500.0);

                self.particles.push(Particle {
                    current_pos: Vec3::new(sx, sy, sz),
                    target_pos: Vec3::new(tx, ty, tz),
                    velocity: Vec3::ZERO,
                    p_type: ParticleType::Pixel,
                    processed: false,
                    text_char: ' ',
                    grid_idx: y * cols + x,
                });
            }
        }

        // 2. Generate "Text" Particles (The Result)
        // We map "SCREEN GROUNDED TRANSLATOR" to 3D positions
        // Convert some Pixel particles to Glyph type
        
        let mut p_idx = 0;
        let text_assignments = [
            ('S', 0), ('C', 1), ('R', 2), ('E', 3), ('E', 4), ('N', 5),
            ('G', 8), ('R', 9), ('O', 10), ('U', 11), ('N', 12), ('D', 13), ('E', 14), ('D', 15),
            ('T', 18), ('R', 19), ('A', 20), ('N', 21), ('S', 22), ('L', 23), ('A', 24), ('T', 25), ('O', 26), ('R', 27),
        ];

        for &(ch, _) in &text_assignments {
            if p_idx < self.particles.len() {
                self.particles[p_idx].text_char = ch;
                self.particles[p_idx].p_type = ParticleType::Glyph;
                p_idx += 1;
            }
        }

        self.init_done = true;
    }

    pub fn update(&mut self, ctx: &egui::Context) -> SplashStatus {
        if !self.init_done { self.init_scene(); }

        let now = ctx.input(|i| i.time);
        let t = (now - self.start_time) as f32;
        let dt = ctx.input(|i| i.stable_dt).min(0.05);

        if t > ANIMATION_DURATION {
            return SplashStatus::Finished;
        }
        ctx.request_repaint();

        // --- PHASE TIMELINE ---
        // 0.0 - 1.5: CAPTURE (Implosion)
        // 1.5 - 3.0: SCAN (Beam sweep)
        // 3.0 - 4.5: TRANSLATION (Morph to Text)
        // 4.5+: GROUNDED (Stabilize)

        // Camera Animation
        if t < 1.5 {
            // Spin fast during capture
            self.cam_rot_y = (1.5 - t).powf(2.0) * 0.5;
            self.cam_zoom = 800.0; // Wide fov
        } else if t < 3.0 {
            // Stabilize for scan
            self.cam_rot_y = self.cam_rot_y * 0.95; // Dampen to 0
            self.cam_zoom = 800.0 + (t - 1.5) * 100.0; // Zoom in slightly
        } else {
            // Lock on for text
            self.cam_rot_y = 0.0;
            self.cam_zoom = 950.0; // Close up
        }

        // Scanner Line Position (Top to Bottom)
        let scan_y = if t > 1.5 && t < 3.0 {
            let st = (t - 1.5) / 1.5; // 0.0 to 1.0
            150.0 - (st * 300.0) // From y=150 down to y=-150
        } else {
            -9999.0
        };

        // Update Particles
        for p in &mut self.particles {
            let mut active_target = p.target_pos;
            let mut spring_strength = 3.0;
            let mut friction = 0.85;

            // Phase Logic
            if t < 1.5 {
                // PHASE 1: IMPLOSION
                spring_strength = 2.0;
                
            } else if t < 3.0 {
                // PHASE 2: SCANNING
                if !p.processed && p.current_pos.y > scan_y {
                    p.processed = true;
                    // Jolt up slightly when scanned
                    p.velocity.z += 20.0;
                }
                
                // Add jitter to unprocessed particles (noise)
                if !p.processed {
                    active_target.z += (t * 20.0 + p.grid_idx as f32).sin() * 5.0;
                }

            } else {
                // PHASE 3: MORPHING
                match p.p_type {
                    ParticleType::Glyph => {
                        // Map to text formation based on grid position
                        let row = p.grid_idx / 40;
                        let col = p.grid_idx % 40;
                        
                        // Form 3 dense bars representing text lines
                        let line_idx = row % 3;
                        let y_target = 40.0 - (line_idx as f32 * 50.0);
                        let x_target = (col as f32 * 10.0) - 200.0;
                        
                        active_target = Vec3::new(x_target, y_target, 50.0);
                        spring_strength = 5.0; // Snap hard
                        friction = 0.6; // Wobble a bit on arrival
                    },
                    ParticleType::Pixel => {
                        // Pixels that aren't text fade/fall back
                        active_target.z = -500.0; // Push back
                        active_target.y -= 100.0; // Fall down
                    }
                }
            }

            // Physics Integration (Spring)
            let diff = active_target.sub(p.current_pos);
            let force = diff.mul(spring_strength * dt);
            
            p.velocity = p.velocity.add(force).mul(friction);
            p.current_pos = p.current_pos.add(p.velocity.mul(dt));
        }

        // Render
        egui::CentralPanel::default().show(ctx, |ui| {
            self.paint(ui, t, scan_y);
        });

        SplashStatus::Ongoing
    }

    fn paint(&self, ui: &mut egui::Ui, t: f32, scan_y: f32) {
        let painter = ui.painter();
        let rect = ui.max_rect();
        let center = rect.center();

        // 1. Background
        let bg_alpha = if t > FADE_OUT_START {
            1.0 - ((t - FADE_OUT_START) * 2.0).clamp(0.0, 1.0)
        } else { 1.0 };
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha((255.0 * bg_alpha) as u8));
        
        if t > FADE_OUT_START { return; } // Don't draw scene if fading out

        // 2. Draw Wireframe Box (The "Capture" Zone)
        if t < 2.5 {
            let box_size = 200.0;
            // A simple 3D bounding box that snaps in
            let expansion = if t < 1.0 { (1.0 - t) * 500.0 } else { 0.0 };
            let s = box_size + expansion;
            let corners = [
                Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0),
                Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0)
            ];
            
            let mut proj_corners = Vec::new();
            for c in corners {
                let rot_c = c.rotate_y(self.cam_rot_y).rotate_x(0.2);
                let (p, _) = rot_c.project(center, self.cam_zoom);
                proj_corners.push(p);
            }
            
            // Draw rect
            let alpha = ((1.0 - (t / 2.5)) * 100.0) as u8;
            let stroke = Stroke::new(1.0, Color32::from_rgba_premultiplied(0, 255, 255, alpha));
            painter.line_segment([proj_corners[0], proj_corners[1]], stroke);
            painter.line_segment([proj_corners[1], proj_corners[2]], stroke);
            painter.line_segment([proj_corners[2], proj_corners[3]], stroke);
            painter.line_segment([proj_corners[3], proj_corners[0]], stroke);
        }

        // 3. Render Particles
        // Z-sort for correct occlusion
        let mut render_list: Vec<(f32, Pos2, f32, Color32, ParticleType, char)> = Vec::with_capacity(self.particles.len());

        for p in &self.particles {
            // Apply Camera Rotation
            let rot_pos = p.current_pos.rotate_y(self.cam_rot_y).rotate_x(0.2); // slight tilt
            let (pos_2d, scale) = rot_pos.project(center, self.cam_zoom);
            
            if scale <= 0.0 { continue; }

            // Determine Color
            let mut col = COL_RAW_DATA;
            if p.processed { col = COL_ANALYZED; }
            
            // Scanner highlight
            if t > 1.5 && t < 3.0 {
                let dist_to_scan = (p.current_pos.y - scan_y).abs();
                if dist_to_scan < 20.0 {
                    col = COL_SCAN_BEAM;
                }
            }
            
            if t > 3.0 && matches!(p.p_type, ParticleType::Glyph) {
                col = COL_FINAL_TEXT;
            }

            // Alpha fade based on Z
            let depth_alpha = (scale * 2.0).clamp(0.2, 1.0);
            col = col.linear_multiply(depth_alpha);

            render_list.push((rot_pos.z, pos_2d, scale, col, p.p_type, p.text_char));
        }

        // Sort: Farthest (lowest Z) first
        render_list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        for (_, pos, scale, col, p_type, char_code) in render_list {
            match p_type {
                ParticleType::Pixel => {
                    let size = 2.0 * scale;
                    if size > 0.5 {
                        painter.rect_filled(Rect::from_center_size(pos, Vec2::splat(size)), 0.0, col);
                    }
                },
                ParticleType::Glyph => {
                    if t > 3.0 {
                        // In Phase 3, draw actual text or symbols
                        let font_size = 14.0 * scale;
                        painter.text(pos, Align2::CENTER_CENTER, &char_code.to_string(), FontId::monospace(font_size), col);
                        
                        // Glow
                        if t > 3.5 {
                             painter.circle_filled(pos, 20.0 * scale, COL_GLOW.linear_multiply(0.1));
                        }
                    } else {
                        // Still a pixel before morph
                         let size = 3.0 * scale;
                         painter.circle_filled(pos, size, col);
                    }
                }
            }
        }

        // 4. Draw Scan Beam Plane
        if t > 1.5 && t < 3.0 {
            // Visualize the beam plane
            let beam_w = 400.0;
            let p1 = Vec3::new(-beam_w, scan_y, 0.0).rotate_y(self.cam_rot_y).rotate_x(0.2).project(center, self.cam_zoom).0;
            let p2 = Vec3::new(beam_w, scan_y, 0.0).rotate_y(self.cam_rot_y).rotate_x(0.2).project(center, self.cam_zoom).0;
            
            painter.line_segment([p1, p2], Stroke::new(2.0, COL_SCAN_BEAM));
            // Glow
            painter.line_segment([p1, p2], Stroke::new(10.0, COL_SCAN_BEAM.linear_multiply(0.2)));
        }

        // 5. Final Flash Text Overlay (The "Grounding")
        if t > 4.0 {
            let opacity = ((t - 4.0) * 2.0).clamp(0.0, 1.0);
            let y_off = (1.0 - opacity) * 20.0;
            
            // Main Title
            painter.text(
                center + Vec2::new(0.0, -20.0 + y_off),
                Align2::CENTER_CENTER,
                "Screen Grounded Translator",
                FontId::proportional(32.0),
                COL_FINAL_TEXT.linear_multiply(opacity)
            );
            
            painter.text(
                center + Vec2::new(0.0, 20.0 + y_off),
                Align2::CENTER_CENTER,
                "nganlinh4",
                FontId::proportional(24.0),
                COL_ANALYZED.linear_multiply(opacity)
            );
            
            // Subtext
            if t > 4.2 {
                painter.text(
                    center + Vec2::new(0.0, 60.0),
                    Align2::CENTER_CENTER,
                    "SGT",
                    FontId::monospace(12.0),
                    Color32::GRAY.linear_multiply(opacity)
                );
            }
        }
    }
}
