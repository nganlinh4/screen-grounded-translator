use eframe::egui;
use eframe::egui::{Color32, Pos2, Rect, Vec2, FontId, Align2, Stroke, Shape};
use std::f32::consts::PI;
use std::cmp::Ordering;

// --- CONFIGURATION ---
const ANIMATION_DURATION: f32 = 8.5;
const START_TRANSITION: f32 = 3.0; 
const EXIT_DURATION: f32 = 0.6; // Faster, punchier warp

// --- PALETTE ---
const C_VOID: Color32 = Color32::from_rgb(10, 12, 18);
const C_CYAN: Color32 = Color32::from_rgb(0, 255, 240);
const C_MAGENTA: Color32 = Color32::from_rgb(255, 0, 110);
const C_WHITE: Color32 = Color32::from_rgb(240, 245, 255);
const C_SHADOW: Color32 = Color32::from_rgb(20, 20, 30);

// --- MATH UTILS ---
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// --- 3D MATH KERNEL ---
#[derive(Clone, Copy, Debug)]
struct Vec3 { x: f32, y: f32, z: f32 }

impl Vec3 {
    const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    fn new(x: f32, y: f32, z: f32) -> Self { Self { x, y, z } }

    fn add(self, v: Vec3) -> Self { Self::new(self.x + v.x, self.y + v.y, self.z + v.z) }
    fn sub(self, v: Vec3) -> Self { Self::new(self.x - v.x, self.y - v.y, self.z - v.z) }
    fn mul(self, s: f32) -> Self { Self::new(self.x * s, self.y * s, self.z * s) }
    fn dot(self, v: Vec3) -> f32 { self.x * v.x + self.y * v.y + self.z * v.z }
    fn len(self) -> f32 { (self.x*self.x + self.y*self.y + self.z*self.z).sqrt() }
    fn normalize(self) -> Self {
        let l = self.len();
        if l == 0.0 { Self::ZERO } else { self.mul(1.0/l) }
    }
    fn lerp(self, target: Vec3, t: f32) -> Self {
        Self::new(
            lerp(self.x, target.x, t),
            lerp(self.y, target.y, t),
            lerp(self.z, target.z, t)
        )
    }
    
    fn rotate_x(self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x, self.y * c - self.z * s, self.y * s + self.z * c)
    }
    fn rotate_y(self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x * c + self.z * s, self.y, -self.x * s + self.z * c)
    }
    fn rotate_z(self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(self.x * c - self.y * s, self.x * s + self.y * c, self.z)
    }
}

// --- GEOMETRY ENGINE ---
struct Voxel {
    helix_radius: f32,
    helix_angle_offset: f32,
    helix_y: f32,
    target_pos: Vec3,
    pos: Vec3,
    rot: Vec3,
    scale: f32,
    velocity: Vec3,
    color: Color32,
    noise_factor: f32,
    is_debris: bool,
}

pub struct SplashScreen {
    start_time: f64,
    voxels: Vec<Voxel>,
    init_done: bool,
    mouse_influence: Vec2,
    mouse_world_pos: Vec3,
    loading_text: String,
    exit_start_time: Option<f64>,
}

pub enum SplashStatus {
    Ongoing,
    Finished,
}

impl SplashScreen {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            start_time: ctx.input(|i| i.time),
            voxels: Vec::with_capacity(500),
            init_done: false,
            mouse_influence: Vec2::ZERO,
            mouse_world_pos: Vec3::ZERO,
            loading_text: "TRANSLATING...".to_string(),
            exit_start_time: None,
        }
    }

    // NEW: Method to reset timer when window becomes visible
    pub fn reset_timer(&mut self, ctx: &egui::Context) {
        self.start_time = ctx.input(|i| i.time);
    }

    fn init_scene(&mut self) {
        let mut rng_state = 12345u64;
        let mut rng = || {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (rng_state >> 32) as f32 / 4294967295.0
        };

        let s_map = [ " ####", "##   ", " ### ", "   ##", "#### " ];
        let g_map = [ " ####", "##   ", "## ##", "##  #", " ####" ];
        let t_map = [ "#####", "  #  ", "  #  ", "  #  ", "  #  " ];

        let spacing = 14.0;
        let mut total_voxels = 0;

        let mut spawn_letter = |map: &[&str], offset_x: f32, color_theme: Color32| {
            for (y, row) in map.iter().enumerate() {
                for (x, ch) in row.chars().enumerate() {
                    if ch == '#' {
                        total_voxels += 1;
                        let tx = offset_x + (x as f32 * spacing);
                        let ty = (2.0 - y as f32) * spacing;
                        let tz = 0.0;
                        let target = Vec3::new(tx, ty, tz);

                        let strand_idx = total_voxels % 2;
                        let h_y = ((total_voxels as f32 * 3.0) % 240.0) - 120.0; 
                        let h_radius = 60.0;
                        let h_angle = (if strand_idx == 0 { 0.0 } else { PI }) + (h_y * 0.05);

                        self.voxels.push(Voxel {
                            helix_radius: h_radius,
                            helix_angle_offset: h_angle,
                            helix_y: h_y,
                            target_pos: target,
                            pos: Vec3::ZERO,
                            rot: Vec3::new(rng() * 6.0, rng() * 6.0, rng() * 6.0),
                            scale: 0.1,
                            velocity: Vec3::ZERO,
                            color: if rng() > 0.85 { C_WHITE } else { color_theme },
                            noise_factor: rng(),
                            is_debris: false,
                        });
                    }
                }
            }
        };

        spawn_letter(&s_map, -110.0, C_CYAN);
        spawn_letter(&g_map, -25.0, C_MAGENTA);
        spawn_letter(&t_map, 60.0, C_CYAN);

        for _ in 0..60 {
            let h_y = (rng() * 300.0) - 150.0;
            let h_radius = 80.0 + rng() * 60.0;
            let h_angle = rng() * PI * 2.0;
            let target = Vec3::new(h_angle.cos(), 0.0, h_angle.sin()).mul(500.0);

            self.voxels.push(Voxel {
                helix_radius: h_radius,
                helix_angle_offset: h_angle,
                helix_y: h_y,
                target_pos: target,
                pos: Vec3::ZERO,
                rot: Vec3::new(rng(), rng(), rng()),
                scale: 0.3,
                velocity: Vec3::ZERO,
                color: C_SHADOW,
                noise_factor: rng(),
                is_debris: true,
            });
        }
        self.init_done = true;
    }

    pub fn update(&mut self, ctx: &egui::Context) -> SplashStatus {
        if !self.init_done { self.init_scene(); }

        let now = ctx.input(|i| i.time);
        
        if self.exit_start_time.is_none() {
            let t = (now - self.start_time) as f32;
            if t > ANIMATION_DURATION {
                if ctx.input(|i| i.pointer.any_click()) {
                    self.exit_start_time = Some(now);
                }
            }
        }

        let t_abs = (now - self.start_time) as f32;
        let physics_t = t_abs.min(ANIMATION_DURATION);

        // --- EXIT LOGIC ---
        let mut warp_progress = 0.0;
        if let Some(exit_start) = self.exit_start_time {
            let dt = (now - exit_start) as f32;
            if dt > EXIT_DURATION {
                return SplashStatus::Finished;
            }
            // Quintic easing for massive acceleration "punch"
            warp_progress = (dt / EXIT_DURATION).powi(5); 
        }

        ctx.request_repaint();

        if let Some(pointer) = ctx.input(|i| i.pointer.hover_pos()) {
            let rect = ctx.input(|i| i.screen_rect());
            let center = rect.center();
            let tx = (pointer.x - center.x) / center.x;
            let ty = (pointer.y - center.y) / center.y;
            self.mouse_influence.x += (tx - self.mouse_influence.x) * 0.05;
            self.mouse_influence.y += (ty - self.mouse_influence.y) * 0.05;

            let cam_z_offset = warp_progress * 2000.0; 
            let cam_dist = 600.0 + smoothstep(0.0, 8.0, physics_t) * 100.0 - cam_z_offset;
            
            let fov = 800.0;
            let mouse_wx = (pointer.x - center.x) * cam_dist / fov;
            let mouse_wy = -(pointer.y - center.y) * cam_dist / fov;
            self.mouse_world_pos = Vec3::new(mouse_wx, mouse_wy, 0.0);
        }

        if self.exit_start_time.is_none() {
            if t_abs < 2.0 { self.loading_text = "TRANSLATING...".to_string(); }
            else if t_abs < 4.0 { self.loading_text = "OCR...".to_string(); }
            else if t_abs < 6.0 { self.loading_text = "TRANSCRIBING...".to_string(); }
            else { self.loading_text = "nganlinh4".to_string(); }
        } else {
            self.loading_text = "HYPERLINK_ENGAGED".to_string();
        }

        // --- PHYSICS UPDATE ---
        let helix_spin = physics_t * 2.0 + (physics_t * physics_t * 0.2); 
        
        for v in &mut self.voxels {
            let my_start = START_TRANSITION + (v.noise_factor * 1.5); 
            let my_end = my_start + 2.0;
            let progress = smoothstep(my_start, my_end, physics_t);

            if progress <= 0.0 {
                let current_h_y = v.helix_y + (physics_t * 2.0 + v.noise_factor * 10.0).sin() * 5.0;
                let current_angle = v.helix_angle_offset + helix_spin;
                let current_radius = v.helix_radius * (1.0 + physics_t * 0.1);
                v.pos = Vec3::new(current_angle.cos() * current_radius, current_h_y, current_angle.sin() * current_radius);
                v.rot.y += 0.05;
                v.scale = 0.8;
                v.velocity = Vec3::ZERO;
            } else {
                let current_h_y = v.helix_y + (physics_t * 2.0 + v.noise_factor * 10.0).sin() * 5.0;
                let current_angle = v.helix_angle_offset + helix_spin;
                let current_radius = v.helix_radius * (1.0 + physics_t * 0.1);
                let helix_pos = Vec3::new(current_angle.cos() * current_radius, current_h_y, current_angle.sin() * current_radius);

                let mut target_base = v.target_pos;
                
                // WARP EFFECT: Explode voxels
                if warp_progress > 0.0 {
                    let radial = Vec3::new(v.pos.x, v.pos.y, 0.0).normalize();
                    // Expand outwards massively
                    target_base = target_base.add(radial.mul(warp_progress * 500.0));
                    // Slight twist
                    target_base = target_base.rotate_z(warp_progress * 0.5);
                }

                let pos = helix_pos.lerp(target_base, progress);

                if progress > 0.9 && !v.is_debris && warp_progress == 0.0 {
                    let to_mouse = pos.sub(self.mouse_world_pos);
                    let dist_sq = to_mouse.x*to_mouse.x + to_mouse.y*to_mouse.y; 
                    if dist_sq < 6400.0 {
                        let dist = dist_sq.sqrt();
                        let force = (80.0 - dist) / 80.0; 
                        v.velocity = v.velocity.add(to_mouse.normalize().mul(force * 2.0));
                        v.rot.x += to_mouse.y * force * 0.01;
                        v.rot.y -= to_mouse.x * force * 0.01;
                    }
                }

                let displacement = pos.sub(target_base);
                let spring_force = displacement.mul(-0.1); 
                v.velocity = v.velocity.add(spring_force);
                v.velocity = v.velocity.mul(0.90); 
                
                v.pos = pos.add(v.velocity);
                v.rot = v.rot.lerp(Vec3::ZERO, 0.1); 
                
                if progress > 0.95 {
                    let impact = (physics_t - my_end).max(0.0);
                    let pulse = (impact * 10.0).sin() * (-3.0 * impact).exp() * 0.5;
                    v.scale = 1.0 + pulse;
                } else {
                    v.scale = lerp(0.8, 1.0, progress);
                }
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                self.render(ui, t_abs, warp_progress);
            });

        SplashStatus::Ongoing
    }

    fn render(&self, ui: &mut egui::Ui, t: f32, warp_prog: f32) {
        let rect = ui.max_rect();
        let painter = ui.painter().with_clip_rect(rect);
        let center = rect.center();
        
        let alpha = if t < 1.0 { t } else { 1.0 };
        let master_alpha = alpha.clamp(0.0, 1.0);

        // 1. Background - Keep it DARK but respect entrance Fade-In
        let mut bg_color = C_VOID;
        if t < 1.0 {
            // Smooth easing for background appearance
            let bg_alpha = (t * t).clamp(0.0, 1.0); 
            bg_color = bg_color.linear_multiply(bg_alpha);
        }
        painter.rect_filled(rect, 32.0, bg_color);

        // 2. Grid
        if master_alpha > 0.05 {
            let render_t = t.min(ANIMATION_DURATION + 5.0);
            // Move grid super fast during warp
            let cam_y = 150.0 + (render_t * 30.0) + (warp_prog * 10000.0);
            let horizon = center.y + 100.0;
            let corner_radius = 32.0;
            
            // Fade grid out during warp so we are left with only streaks
            let grid_fade = 1.0 - warp_prog;

            if grid_fade > 0.0 {
                for i in 0..12 {
                    let z_dist = 1.0 + (i as f32 * 0.5) - ((cam_y * 0.05) % 0.5);
                    let perspective = 200.0 / (z_dist - warp_prog * 0.8).max(0.1);
                    let y = horizon + perspective * 0.8;
                    
                    if y > rect.bottom() || y < horizon { continue; }

                    let w = rect.width() * (2.0 / z_dist);
                    
                    let mut x_min = rect.left();
                    let mut x_max = rect.right();
                    
                    if y > rect.bottom() - corner_radius {
                        let dy = y - (rect.bottom() - corner_radius);
                        let dx = (corner_radius.powi(2) - dy.powi(2)).max(0.0).sqrt();
                        x_min = rect.left() + corner_radius - dx;
                        x_max = rect.right() - corner_radius + dx;
                    }

                    let x1 = (center.x - w).clamp(x_min, x_max);
                    let x2 = (center.x + w).clamp(x_min, x_max);

                    let alpha_grid = (1.0 - (y - horizon) / (rect.bottom() - horizon)) * master_alpha * 0.4 * grid_fade;
                    if alpha_grid > 0.0 && x1 < x2 {
                        painter.line_segment(
                            [Pos2::new(x1, y), Pos2::new(x2, y)], 
                            Stroke::new(2.0, C_MAGENTA.linear_multiply(alpha_grid))
                        );
                    }
                }
            }
        }

        // 3. 3D RENDERER
        let physics_t = t.min(ANIMATION_DURATION);
        let fov = 800.0;
        let cam_fly_dist = warp_prog * 2000.0; 
        let cam_dist = (600.0 + smoothstep(0.0, 8.0, physics_t) * 100.0) - cam_fly_dist;
        
        let global_rot = Vec3::new(
             self.mouse_influence.y * 0.2, 
             self.mouse_influence.x * 0.2, 
             0.0
        );

        let light_dir = Vec3::new(-0.5, -1.0, -0.5).normalize();
        
        let mut draw_list: Vec<(f32, Vec<Pos2>, Color32, bool)> = Vec::with_capacity(self.voxels.len() * 6);

        let cube_size = 6.0;
        // STRETCH: Massive Z stretch during warp for "Light Speed" effect
        let z_stretch = 1.0 + (warp_prog * 150.0); 
        let verts = [
            Vec3::new(-1.0, -1.0, -1.0 * z_stretch), Vec3::new( 1.0, -1.0, -1.0 * z_stretch), Vec3::new( 1.0,  1.0, -1.0 * z_stretch), Vec3::new(-1.0,  1.0, -1.0 * z_stretch),
            Vec3::new(-1.0, -1.0,  1.0 * z_stretch), Vec3::new( 1.0, -1.0,  1.0 * z_stretch), Vec3::new( 1.0,  1.0,  1.0 * z_stretch), Vec3::new(-1.0,  1.0,  1.0 * z_stretch),
        ];
        let faces = [
            ([0, 1, 2, 3], Vec3::new(0.0, 0.0, -1.0)),
            ([1, 5, 6, 2], Vec3::new(1.0, 0.0, 0.0)),
            ([5, 4, 7, 6], Vec3::new(0.0, 0.0, 1.0)),
            ([4, 0, 3, 7], Vec3::new(-1.0, 0.0, 0.0)),
            ([3, 2, 6, 7], Vec3::new(0.0, 1.0, 0.0)),
            ([4, 5, 1, 0], Vec3::new(0.0, -1.0, 0.0)),
        ];

        // FIX: Per-particle organic fade instead of global uniform fade
        // Removed: let debris_fade = 1.0 - smoothstep(4.5, 6.5, physics_t);

        for v in &self.voxels {
            // FIX: Calculate local fade for debris based on noise
            let mut local_debris_alpha = 1.0;
            if v.is_debris {
                // Stagger fade start between 4.0s and 7.0s
                let fade_start = 4.0 + (v.noise_factor * 3.0); 
                // Fade duration of 2.5s
                let fade_end = fade_start + 2.5;
                local_debris_alpha = 1.0 - smoothstep(fade_start, fade_end, physics_t);
                
                // Cull if invisible
                if local_debris_alpha <= 0.01 { continue; }
            }

            let mut v_center = v.pos;
            v_center = v_center.rotate_x(global_rot.x).rotate_y(global_rot.y).rotate_z(global_rot.z);
            
            // Don't clip during warp, let them fly behind
            if warp_prog == 0.0 && v_center.z > cam_dist - 10.0 { continue; }

            for (indices, normal) in &faces {
                let rot_normal = normal
                    .rotate_x(v.rot.x).rotate_y(v.rot.y).rotate_z(v.rot.z)
                    .rotate_x(global_rot.x).rotate_y(global_rot.y).rotate_z(global_rot.z);

                // Only cull backfaces if not warping (during warp, geometry is weird)
                if warp_prog == 0.0 && rot_normal.z > 0.0 { continue; }

                let diffuse = rot_normal.dot(light_dir).max(0.0);
                let intensity = 0.3 + 0.7 * diffuse;
                
                let mut alpha_local = master_alpha;
                if v.is_debris { alpha_local *= local_debris_alpha; }
                
                // COLOR SHIFT: During warp, turn everything into Cyan/Magenta streaks
                let mut base_col = v.color;
                if warp_prog > 0.0 {
                    // Fade alpha out at the very end of warp to reveal UI
                    alpha_local *= 1.0 - warp_prog; 
                    // Shift color to Cyan
                    base_col = C_CYAN; 
                }

                let r = (base_col.r() as f32 * intensity) as u8;
                let g = (base_col.g() as f32 * intensity) as u8;
                let b = (base_col.b() as f32 * intensity) as u8;
                let face_color = Color32::from_rgba_premultiplied(r, g, b, (255.0 * alpha_local) as u8);

                let mut poly_verts = Vec::with_capacity(4);
                let mut avg_z = 0.0;
                
                for &idx in indices {
                    let local_v = verts[idx].mul(cube_size * v.scale);
                    let rot_v = local_v.rotate_x(v.rot.x).rotate_y(v.rot.y).rotate_z(v.rot.z);
                    let world_v = rot_v.add(v.pos);
                    let final_v = world_v.rotate_x(global_rot.x).rotate_y(global_rot.y).rotate_z(global_rot.z);
                    
                    let z_depth = cam_dist - final_v.z;
                    avg_z += z_depth;
                    
                    // Warp projection safe-guard
                    if z_depth > 0.1 {
                        let scale = fov / z_depth;
                        let x = center.x + final_v.x * scale;
                        let y = center.y - final_v.y * scale;
                        poly_verts.push(Pos2::new(x, y));
                    }
                }

                if poly_verts.len() == 4 {
                    avg_z /= 4.0;
                    draw_list.push((avg_z, poly_verts, face_color, v.color == C_WHITE));
                }
            }
        }

        draw_list.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));

        for (_, verts, col, is_glowing) in draw_list {
            painter.add(Shape::convex_polygon(verts.clone(), col, Stroke::NONE));
            
            if master_alpha > 0.8 && warp_prog < 0.5 {
                let stroke_col = if is_glowing { C_CYAN.linear_multiply(0.3) } else { Color32::from_black_alpha(40) };
                painter.add(Shape::closed_line(verts, Stroke::new(1.0, stroke_col)));
            }
        }

        if master_alpha > 0.1 && warp_prog < 0.1 {
            let ui_alpha = 1.0 - (warp_prog * 10.0).clamp(0.0, 1.0); // Fade UI instantly on warp
            let ui_color = C_WHITE.linear_multiply(master_alpha * ui_alpha);
            let cyan_color = C_CYAN.linear_multiply(master_alpha * ui_alpha);
            let magenta_color = C_MAGENTA.linear_multiply(master_alpha * ui_alpha);

            painter.text(
                center + Vec2::new(0.0, 180.0),
                Align2::CENTER_TOP,
                "SCREEN GROUNDED TRANSLATOR",
                FontId::proportional(24.0),
                ui_color
            );
            painter.text(
                center + Vec2::new(0.0, 210.0),
                Align2::CENTER_TOP,
                &self.loading_text,
                FontId::monospace(12.0),
                cyan_color
            );
            
            let bar_rect = Rect::from_center_size(center + Vec2::new(0.0, 230.0), Vec2::new(200.0, 4.0));
            painter.rect_filled(bar_rect, 2.0, Color32::from_white_alpha((30.0 * ui_alpha) as u8));
            let prog = (physics_t / (ANIMATION_DURATION - 1.0)).clamp(0.0, 1.0);
            let mut fill = bar_rect;
            fill.set_width(bar_rect.width() * prog);
            painter.rect_filled(fill, 2.0, magenta_color);

            if t > ANIMATION_DURATION {
                let pulse = (t * 5.0).sin().abs() * 0.7 + 0.3; 
                painter.text(
                    center + Vec2::new(0.0, 250.0),
                    Align2::CENTER_TOP,
                    "Click anywhere to continue",
                    FontId::proportional(14.0),
                    cyan_color.linear_multiply(pulse)
                );
            }
        }
    }
}