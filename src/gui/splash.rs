use eframe::egui;
use eframe::egui::{Color32, Pos2, Rect, Vec2, FontId, Align2, Stroke};
use std::f32::consts::PI;

// --- CONFIGURATION ---
const ANIMATION_DURATION: f32 = 7.5;
const PHASE_ASSEMBLE: f32 = 0.5;   // Particles start moving to targets
const PHASE_CONNECT: f32 = 2.5;    // Neural lines appear
const PHASE_STABILIZE: f32 = 5.0;  // Lock in and glow
const FADE_OUT_START: f32 = 6.5;

// --- PALETTE (Holographic Tech) ---
const C_VOID: Color32 = Color32::from_rgb(10, 11, 16);          // Deep Space
const C_CYAN: Color32 = Color32::from_rgb(0, 240, 255);         // Data Stream
const C_MAGENTA: Color32 = Color32::from_rgb(255, 0, 128);      // System Core
const C_WHITE: Color32 = Color32::from_rgb(220, 240, 255);      // High Energy

// --- 3D MATH KERNEL ---
#[derive(Clone, Copy, Debug, PartialEq)]
struct Vec3 { x: f32, y: f32, z: f32 }

impl Vec3 {
    const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    fn new(x: f32, y: f32, z: f32) -> Self { Self { x, y, z } }

    fn add(self, v: Vec3) -> Self { Self::new(self.x + v.x, self.y + v.y, self.z + v.z) }
    fn sub(self, v: Vec3) -> Self { Self::new(self.x - v.x, self.y - v.y, self.z - v.z) }
    fn mul(self, s: f32) -> Self { Self::new(self.x * s, self.y * s, self.z * s) }
    fn len(self) -> f32 { (self.x*self.x + self.y*self.y + self.z*self.z).sqrt() }
    
    fn rotate_y(self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x * c + self.z * s, self.y, -self.x * s + self.z * c)
    }
    fn rotate_x(self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x, self.y * c - self.z * s, self.y * s + self.z * c)
    }

    // Returns (ScreenPos, Scale, Z-Depth)
    fn project(self, center: Pos2, fov_scale: f32, cam_z: f32) -> Option<(Pos2, f32, f32)> {
        let z_depth = cam_z - self.z;
        if z_depth <= 10.0 { return None; } // Near clip plane
        let scale = fov_scale / z_depth;
        let x = center.x + self.x * scale;
        let y = center.y - self.y * scale; 
        Some((Pos2::new(x, y), scale, z_depth))
    }
}

// --- PARTICLE SYSTEM ---
#[derive(PartialEq, Clone, Copy)]
enum PType {
    CoreVoxel,  // The SGT Text
    Node,       // Points on the Neural Sphere
    DataBit,    // Fast orbiting bits
}

struct Particle {
    pos: Vec3,
    start_pos: Vec3,
    target: Vec3,
    vel: Vec3,
    
    ptype: PType,
    color: Color32,
    base_size: f32,
    
    // Connectivity
    connections: Vec<usize>, // Indices of neighbors to draw lines to
    
    // Animation
    phase_offset: f32,
}

pub struct SplashScreen {
    start_time: f64,
    particles: Vec<Particle>,
    init_done: bool,
    mouse_influence: Vec2,
    
    // Dynamic Text State
    loading_text: String,
}

pub enum SplashStatus {
    Ongoing,
    Finished,
}

impl SplashScreen {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            start_time: ctx.input(|i| i.time),
            particles: Vec::with_capacity(2500),
            init_done: false,
            mouse_influence: Vec2::ZERO,
            loading_text: "INITIALIZING NEURAL LINK...".to_string(),
        }
    }

    fn init_scene(&mut self) {
        let mut rng_state = 1337u64;
        let mut rng = || {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (rng_state >> 32) as f32 / 4294967295.0
        };

        // --- 1. SGT LOGO (Central Voxel Grid) ---
        // Dense grid for solid look
        let s_grid = [" XXX ", "X   X", "X    ", " XXX ", "    X", "X   X", " XXX "];
        let g_grid = [" XXX ", "X   X", "X    ", "X XX ", "X   X", "X   X", " XXX "];
        let t_grid = ["XXXXX", "  X  ", "  X  ", "  X  ", "  X  ", "  X  ", "  X  "];

        let mut spawn_voxels = |grid: &[&str], x_offset: f32| {
            for (row, line) in grid.iter().enumerate() {
                for (col, char) in line.chars().enumerate() {
                    if char == 'X' {
                        // Extrude depth
                        for d in 0..2 {
                            let tx = (x_offset + col as f32) * 14.0;
                            let ty = ((3.0 - row as f32) * 14.0) + 0.0;
                            let tz = (d as f32 * 14.0) - 7.0;

                            let target = Vec3::new(tx, ty, tz);
                            let start = Vec3::new(
                                (rng() - 0.5) * 800.0,
                                (rng() - 0.5) * 800.0,
                                -500.0 + rng() * 1000.0
                            );

                            self.particles.push(Particle {
                                pos: start,
                                start_pos: start,
                                target,
                                vel: Vec3::ZERO,
                                ptype: PType::CoreVoxel,
                                color: C_WHITE,
                                base_size: 4.0,
                                connections: Vec::new(), // Voxels are solid, no lines needed
                                phase_offset: rng(),
                            });
                        }
                    }
                }
            }
        };

        spawn_voxels(&s_grid, -14.0);
        spawn_voxels(&g_grid, -2.5);
        spawn_voxels(&t_grid, 9.0);

        // --- 2. NEURAL SPHERE (The World) ---
        // Fibonacci Sphere algorithm for even distribution
        let num_nodes = 300;
        let sphere_radius = 220.0;
        let phi = PI * (3.0 - 5.0f32.sqrt()); // Golden angle

        let sphere_start_idx = self.particles.len();

        for i in 0..num_nodes {
            let y = 1.0 - (i as f32 / (num_nodes - 1) as f32) * 2.0; // y goes from 1 to -1
            let radius = (1.0 - y * y).sqrt();
            let theta = phi * i as f32;

            let x = theta.cos() * radius;
            let z = theta.sin() * radius;

            let target = Vec3::new(x * sphere_radius, y * sphere_radius, z * sphere_radius);
            
            // Start exploded
            let start = target.mul(3.5);

            self.particles.push(Particle {
                pos: start,
                start_pos: start,
                target,
                vel: Vec3::ZERO,
                ptype: PType::Node,
                color: C_CYAN,
                base_size: 2.5,
                connections: Vec::new(),
                phase_offset: rng(),
            });
        }

        // Pre-calculate connections (Nearest Neighbors on Sphere)
        // This is O(N^2) but N=300 is tiny, so it's instant.
        // Doing this once at init allows 60FPS rendering of lines.
        for i in sphere_start_idx..self.particles.len() {
            let p1_target = self.particles[i].target;
            let mut neighbors = Vec::new();
            
            for j in sphere_start_idx..self.particles.len() {
                if i == j { continue; }
                let dist = p1_target.sub(self.particles[j].target).len();
                if dist < 45.0 { // Connection threshold
                    neighbors.push((dist, j));
                }
            }
            // Keep closest 3
            neighbors.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            for (_, idx) in neighbors.iter().take(3) {
                self.particles[i].connections.push(*idx);
            }
        }

        // --- 3. DATA RINGS (Counter-Rotating) ---
        for r in 0..2 {
            let count = 150;
            let radius = 280.0 + (r as f32 * 40.0);
            let speed_mult = if r == 0 { 1.0 } else { -0.7 };
            
            for i in 0..count {
                let angle = (i as f32 / count as f32) * PI * 2.0;
                let x = angle.cos() * radius;
                let z = angle.sin() * radius;
                // Tilt the rings
                let tilt = 0.3;
                let y = z * tilt; 
                let z = z * (1.0 - tilt); // Flatten z slightly

                let target = Vec3::new(x, y, z);
                let start = Vec3::new(x, 2000.0 * speed_mult, z); // Rain down

                self.particles.push(Particle {
                    pos: start,
                    start_pos: start,
                    target,
                    vel: Vec3::new(0.0, speed_mult, 0.0), // Store rotation direction in Y vel
                    ptype: PType::DataBit,
                    color: if r == 0 { C_MAGENTA } else { C_CYAN },
                    base_size: 1.5,
                    connections: Vec::new(),
                    phase_offset: rng(),
                });
            }
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

        // 1. Mouse Gyroscope
        if let Some(pointer) = ctx.input(|i| i.pointer.hover_pos()) {
            let rect = ctx.input(|i| i.screen_rect());
            let center = rect.center();
            let tx = (pointer.x - center.x) / center.x;
            let ty = (pointer.y - center.y) / center.y;
            // Smooth lerp
            self.mouse_influence.x += (tx - self.mouse_influence.x) * 0.05;
            self.mouse_influence.y += (ty - self.mouse_influence.y) * 0.05;
        }

        // 2. Status Text Logic
        if t < 2.0 { self.loading_text = "Translate".to_string(); }
        else if t < 3.5 { self.loading_text = "OCR".to_string(); }
        else if t < 4.5 { self.loading_text = "Transcribe".to_string(); }
        else { self.loading_text = "nganlinh4".to_string(); }

        // 3. Physics Engine
        let assemble_factor = ((t - PHASE_ASSEMBLE) / 2.0).clamp(0.0, 1.0);
        let ease_assemble = 1.0 - (1.0 - assemble_factor).powi(4); // Quartic ease out
        
        for p in &mut self.particles {
            match p.ptype {
                PType::CoreVoxel | PType::Node => {
                    // Magnetic Assembly
                    // Blend between chaotic start and orderly target
                    let dest = if t < PHASE_ASSEMBLE {
                        p.start_pos
                    } else {
                        // Lerp towards target
                        let v = p.target.sub(p.start_pos);
                        p.start_pos.add(v.mul(ease_assemble))
                    };
                    
                    // Add "breathing" noise
                    let pulse = (t * 2.0 + p.phase_offset * 10.0).sin() * 2.0;
                    let noisy_dest = dest.add(Vec3::new(pulse, pulse, pulse));

                    // Spring physics to reach noisy_dest
                    let diff = noisy_dest.sub(p.pos);
                    p.vel = p.vel.add(diff.mul(0.1)).mul(0.85); // Stiff spring
                    p.pos = p.pos.add(p.vel);
                },
                
                PType::DataBit => {
                    // Orbit Logic
                    let rot_speed = p.vel.y * 1.5; // Stored in Y
                    let rot = t * rot_speed;
                    
                    // Rotate the TARGET around Y
                    let orbital_pos = p.target.rotate_y(rot);
                    
                    // Lerp vertically from start (rain effect) to orbit plane
                    let current_y = if t < PHASE_ASSEMBLE {
                        p.start_pos.y * (1.0 - ease_assemble) + orbital_pos.y * ease_assemble
                    } else {
                        orbital_pos.y
                    };

                    let final_target = Vec3::new(orbital_pos.x, current_y, orbital_pos.z);
                    
                    let diff = final_target.sub(p.pos);
                    p.vel.x = p.vel.x * 0.9 + diff.x * 0.1;
                    p.vel.z = p.vel.z * 0.9 + diff.z * 0.1;
                    p.pos.x += p.vel.x;
                    p.pos.y = current_y; // Lock Y hard
                    p.pos.z += p.vel.z;
                }
            }
        }

        // Render Frame
        egui::CentralPanel::default().show(ctx, |ui| {
            self.paint(ui, t);
        });

        SplashStatus::Ongoing
    }

    fn paint(&self, ui: &mut egui::Ui, t: f32) {
        let painter = ui.painter();
        let rect = ui.max_rect();
        let center = rect.center();
        
        // --- 1. GLOBAL OPACITY ---
        let master_alpha = if t > FADE_OUT_START {
            (1.0 - (t - FADE_OUT_START) * 1.5).clamp(0.0, 1.0)
        } else {
            (t * 2.0).clamp(0.0, 1.0) // Fade in
        };

        // --- 2. BACKGROUND ---
        painter.rect_filled(rect, 0.0, C_VOID);
        
        // Grid Floor (Cyber Plane)
        if master_alpha > 0.1 {
            let grid_t = (t * 0.2) % 1.0;
            let horizon_y = center.y + 200.0;
            for i in 0..10 {
                let z_fac = (i as f32 + grid_t) / 10.0; // 0 to 1
                let y = horizon_y + (z_fac * z_fac * 200.0);
                let w = rect.width() * z_fac * 2.0;
                let alpha = (1.0 - z_fac) * 0.1 * master_alpha;
                
                let line_color = C_CYAN.linear_multiply(alpha);
                painter.line_segment(
                    [Pos2::new(center.x - w, y), Pos2::new(center.x + w, y)],
                    Stroke::new(1.0, line_color)
                );
            }
        }

        // --- 3. CAMERA PROJECTION ---
        let fov = 1000.0;
        let cam_dist = 2000.0 - (ease_out_cubic((t / 5.0).clamp(0.0, 1.0)) * 800.0);
        let cam_rot = Vec3::new(
            self.mouse_influence.y * 0.3, 
            self.mouse_influence.x * 0.3 + (t * 0.2), 
            0.0
        );

        // Transform & Sort Particles
        // (Z-Depth, ScreenPos, Scale, Color, Type, Size, &Connections)
        let mut draw_list = Vec::with_capacity(self.particles.len());

        for (idx, p) in self.particles.iter().enumerate() {
            // Apply Camera Rot
            let view_pos = p.pos.rotate_y(cam_rot.y).rotate_x(cam_rot.x);
            
            if let Some((screen_pos, scale, z)) = view_pos.project(center, fov, cam_dist) {
                // Depth Fog
                let fog = (1.0 - (z / 3000.0)).clamp(0.2, 1.0);
                let col = p.color.linear_multiply(fog * master_alpha);
                
                draw_list.push((z, screen_pos, scale, col, p.ptype, p.base_size, &p.connections, idx));
            }
        }
        
        // Sort: Furthest first
        draw_list.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

        // --- 4. RENDER LAYERS ---
        
        // A. PLEXUS LINES (Draw first so nodes are on top)
        if t > PHASE_CONNECT {
            let line_alpha = ((t - PHASE_CONNECT)).clamp(0.0, 1.0) * master_alpha * 0.4;
            if line_alpha > 0.05 {
                // We need a quick lookup map for screen positions
                let mut screen_map = std::collections::HashMap::new();
                for (_, pos, _, _, _, _, _, idx) in &draw_list {
                    screen_map.insert(*idx, *pos);
                }
                
                let stroke = Stroke::new(1.0, C_CYAN.linear_multiply(line_alpha));
                
                // Only iterate particles that have connections (Sphere nodes)
                for (_, pos_a, _, _, ptype, _, conns, idx) in &draw_list {
                    if *ptype == PType::Node {
                        for &target_idx in *conns {
                            // Only draw if target index is "greater" to avoid double drawing lines
                            // Or simpler: just draw them.
                            if let Some(pos_b) = screen_map.get(&target_idx) {
                                painter.line_segment([*pos_a, *pos_b], stroke);
                            }
                        }
                    }
                }
            }
        }

        // B. PARTICLES
        for (_, pos, scale, col, ptype, base_size, _, _) in &draw_list {
            let size = base_size * scale;
            if size < 1.0 { continue; }

            match ptype {
                PType::CoreVoxel => {
                    // Holographic Voxel
                    painter.rect_filled(
                        Rect::from_center_size(*pos, Vec2::splat(size)),
                        1.0,
                        *col
                    );
                    // Glow
                    painter.circle_filled(*pos, size * 2.0, col.linear_multiply(0.2));
                },
                PType::Node => {
                    // Neural Node
                    painter.circle_filled(*pos, size, *col);
                },
                PType::DataBit => {
                    // Data Stream
                    painter.rect_filled(
                        Rect::from_center_size(*pos, Vec2::new(size * 3.0, size)), // Dash shape
                        0.0,
                        *col
                    );
                }
            }
        }

        // --- 5. UI OVERLAY (HUD) ---
        if master_alpha > 0.1 {
            // A. Progress Bar
            let bar_w = 300.0;
            let bar_h = 4.0;
            let progress = (t / PHASE_STABILIZE).clamp(0.0, 1.0);
            
            let bar_rect = Rect::from_center_size(
                center + Vec2::new(0.0, 200.0),
                Vec2::new(bar_w, bar_h)
            );
            
            // BG
            painter.rect_filled(bar_rect, 2.0, Color32::from_white_alpha(30));
            // Fill
            let mut fill_rect = bar_rect;
            fill_rect.set_width(bar_w * progress);
            painter.rect_filled(fill_rect, 2.0, C_CYAN.linear_multiply(master_alpha));
            
            // B. Main Title (with Chromatic Aberration)
            let title_pos = center + Vec2::new(0.0, 150.0);
            let font = FontId::proportional(22.0);
            
            // Red Shift
            painter.text(
                title_pos + Vec2::new(-2.0, 0.0), Align2::CENTER_TOP, "SCREEN GROUNDED TRANSLATOR", 
                font.clone(), Color32::from_rgba_premultiplied(255, 0, 0, (100.0 * master_alpha) as u8)
            );
            // Blue Shift
            painter.text(
                title_pos + Vec2::new(2.0, 0.0), Align2::CENTER_TOP, "SCREEN GROUNDED TRANSLATOR", 
                font.clone(), Color32::from_rgba_premultiplied(0, 255, 255, (100.0 * master_alpha) as u8)
            );
            // White Core
            painter.text(
                title_pos, Align2::CENTER_TOP, "SCREEN GROUNDED TRANSLATOR", 
                font, C_WHITE.linear_multiply(master_alpha)
            );

            // C. Status Text (Typewriter style)
            painter.text(
                center + Vec2::new(0.0, 220.0),
                Align2::CENTER_TOP,
                &self.loading_text,
                FontId::monospace(12.0),
                C_CYAN.linear_multiply(master_alpha)
            );
        }
        
        // --- 6. POST PROCESS: SCANLINES ---
        // Simple horizontal lines overlay
        let scan_alpha = (20.0 * master_alpha) as u8;
        for i in (0..rect.height() as i32).step_by(4) {
             painter.line_segment(
                [Pos2::new(rect.left(), rect.top() + i as f32), Pos2::new(rect.right(), rect.top() + i as f32)],
                Stroke::new(1.0, Color32::from_black_alpha(scan_alpha))
            );
        }
    }
}

// Helper Easing
fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}
