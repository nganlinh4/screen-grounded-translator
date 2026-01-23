use eframe::egui;
use eframe::egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use std::cmp::Ordering;
use std::f32::consts::PI;

use crate::gui::icons::{paint_icon, Icon};
use crate::{WINDOW_HEIGHT, WINDOW_WIDTH};

// --- CONFIGURATION ---
const ANIMATION_DURATION: f32 = 3.6;
const START_TRANSITION: f32 = 0.8;
const EXIT_DURATION: f32 = 1.6; // Extended for majestic slow-motion reveal

// --- PALETTE ---
const C_VOID: Color32 = Color32::from_rgb(5, 5, 10);
const C_CYAN: Color32 = Color32::from_rgb(0, 255, 240);
const C_MAGENTA: Color32 = Color32::from_rgb(255, 0, 110);
const C_WHITE: Color32 = Color32::from_rgb(240, 245, 255);
const C_SHADOW: Color32 = Color32::from_rgb(20, 20, 30);

// Moon Palette (Textured Pink Moon)
const C_MOON_BASE: Color32 = Color32::from_rgb(230, 60, 120);
const C_MOON_SHADOW: Color32 = Color32::from_rgb(130, 20, 60); // Deep crater shadows
const C_MOON_HIGHLIGHT: Color32 = Color32::from_rgb(255, 180, 220); // Crater rims
const C_MOON_GLOW: Color32 = Color32::from_rgb(255, 0, 100);

// Dark Cloud Palette - REVERTED TO BLACK AESTHETIC
const C_CLOUD_CORE: Color32 = Color32::from_rgb(2, 2, 5); // Almost pure black void

// --- DAY PALETTE ---
const C_SKY_DAY_TOP: Color32 = Color32::from_rgb(100, 180, 255);
const C_DAY_REP: Color32 = Color32::from_rgb(0, 110, 255); // Representative (Vibrant Blue)
const C_DAY_SEC: Color32 = Color32::from_rgb(255, 255, 255); // Secondary (White) - S/T Voxels
const C_DAY_TEXT: Color32 = Color32::from_rgb(255, 120, 0); // Text (Orange) - Title/Loading

const C_SUN_BODY: Color32 = Color32::from_rgb(255, 160, 20);
const C_SUN_FLARE: Color32 = Color32::from_rgb(255, 240, 150);
const C_SUN_GLOW: Color32 = Color32::from_rgb(255, 200, 50);
const C_SUN_HIGHLIGHT: Color32 = Color32::from_rgb(255, 255, 220);

const C_CLOUD_WHITE: Color32 = Color32::from_rgb(255, 255, 255);

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
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    fn add(self, v: Vec3) -> Self {
        Self::new(self.x + v.x, self.y + v.y, self.z + v.z)
    }
    fn sub(self, v: Vec3) -> Self {
        Self::new(self.x - v.x, self.y - v.y, self.z - v.z)
    }
    fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
    fn len(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    fn normalize(self) -> Self {
        let l = self.len();
        if l == 0.0 {
            Self::ZERO
        } else {
            self.mul(1.0 / l)
        }
    }
    fn lerp(self, target: Vec3, t: f32) -> Self {
        Self::new(
            lerp(self.x, target.x, t),
            lerp(self.y, target.y, t),
            lerp(self.z, target.z, t),
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

// --- ATMOSPHERE ENTITIES ---

struct Cloud {
    pos: Vec2,
    velocity: f32,
    scale: f32,
    opacity: f32,
    puffs: Vec<(Vec2, f32)>, // (Offset from center, Radius multiplier)
}

struct Star {
    pos: Vec2, // 0.0-1.0 normalized screen coords
    phase: f32,
    brightness: f32,
    size: f32,
}

// --- MOON ENTITIES ---
struct MoonFeature {
    pos: Vec2, // Normalized on moon disk (-1.0 to 1.0)
    radius: f32,
    is_crater: bool, // if true, draws a depth ring; if false, draws a filled patch (Mare)
}

// --- VOXEL ENTITIES ---
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
    clouds: Vec<Cloud>,
    stars: Vec<Star>,
    moon_features: Vec<MoonFeature>,
    init_done: bool,
    mouse_influence: Vec2,
    mouse_world_pos: Vec3,
    loading_text: String,
    exit_start_time: Option<f64>,
    is_dark: bool,
}

pub enum SplashStatus {
    Ongoing,
    Finished,
}

impl SplashScreen {
    pub fn new(ctx: &egui::Context) -> Self {
        let is_dark = ctx.style().visuals.dark_mode;
        Self {
            start_time: ctx.input(|i| i.time),
            voxels: Vec::with_capacity(500),
            clouds: Vec::new(),
            stars: Vec::new(),
            moon_features: Vec::new(),
            init_done: false,
            mouse_influence: Vec2::ZERO,
            mouse_world_pos: Vec3::ZERO,
            loading_text: "TRANSLATING...".to_string(),
            exit_start_time: None,
            is_dark,
        }
    }

    pub fn reset_timer(&mut self, ctx: &egui::Context) {
        self.start_time = ctx.input(|i| i.time);
    }

    fn init_scene(&mut self) {
        let mut rng_state = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(987654321u64);

        let mut rng = || {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (rng_state >> 32) as f32 / 4294967295.0
        };

        // --- 1. Init Text Voxels ---
        let s_map = [" ####", "##   ", " ### ", "   ##", "#### "];
        let g_map = [" ####", "##   ", "## ##", "##  #", " ####"];
        let t_map = ["#####", "  #  ", "  #  ", "  #  ", "  #  "];

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

        let c_primary = if self.is_dark { C_MAGENTA } else { C_DAY_REP };
        let c_secondary = if self.is_dark { C_CYAN } else { C_DAY_SEC };

        spawn_letter(&s_map, -120.0, c_secondary);
        spawn_letter(&g_map, -35.0, c_primary);
        spawn_letter(&t_map, 50.0, c_secondary);

        // Debris
        for _ in 0..100 {
            let h_y = (rng() * 300.0) - 150.0;
            let h_radius = 80.0 + rng() * 60.0;
            let h_angle = rng() * PI * 2.0;

            let n = rng();
            // Spread targets in a thick 3D torus/nebula
            let t_y = rng() * 700.0 - 50.0;
            // Diverse depth: Some very close, some very far
            let t_dist = 400.0 + n.powi(2) * 1400.0;
            let target = Vec3::new(h_angle.cos() * t_dist, t_y, h_angle.sin() * t_dist);

            self.voxels.push(Voxel {
                helix_radius: h_radius,
                helix_angle_offset: h_angle,
                helix_y: h_y,
                target_pos: target,
                pos: Vec3::ZERO,
                rot: Vec3::new(rng(), rng(), rng()),
                // Diverse sizes: small dust to larger puffs
                scale: 0.1 + n * 0.5,
                velocity: Vec3::ZERO,
                color: C_SHADOW,
                noise_factor: n,
                is_debris: true,
            });
        }

        // --- 2. Init Stars ---
        for _ in 0..150 {
            self.stars.push(Star {
                pos: Vec2::new(rng(), rng() * 0.85), // Keep mostly top/middle
                phase: rng() * PI * 2.0,
                brightness: 0.3 + rng() * 0.7,
                size: if rng() > 0.95 {
                    1.5 + rng()
                } else {
                    0.8 + rng() * 0.5
                },
            });
        }

        // --- 3. Init Dark Clouds (Volumetric Puffs) ---
        for _ in 0..15 {
            // Fewer total clouds, but more complex
            let mut puffs = Vec::new();
            // Core main puff
            puffs.push((Vec2::ZERO, 1.0));
            // Satellites
            let num_puffs = 5 + (rng() * 4.0) as usize;
            for _ in 0..num_puffs {
                let angle = rng() * PI * 2.0;
                let dist = 15.0 + rng() * 25.0;
                let r_mult = 0.4 + rng() * 0.5;
                puffs.push((
                    Vec2::new(angle.cos() * dist, angle.sin() * dist * 0.6), // Squashed vertically
                    r_mult,
                ));
            }

            self.clouds.push(Cloud {
                pos: Vec2::new(rng() * 1200.0 - 600.0, rng() * 400.0 - 200.0),
                velocity: 5.0 + rng() * 15.0, // Drifting right
                scale: 1.2 + rng() * 1.5,
                opacity: 0.4 + rng() * 0.4,
                puffs,
            });
        }

        // --- 4. Init Moon Features ---
        // Maria (Dark Patches - large, irregular)
        for _ in 0..20 {
            let angle = rng() * PI * 2.0;
            let dist = rng().sqrt() * 0.7; // Bias towards center/middle
            let pos = Vec2::new(angle.cos() * dist, angle.sin() * dist);

            self.moon_features.push(MoonFeature {
                pos,
                radius: 0.15 + rng() * 0.25,
                is_crater: false,
            });
        }

        // Craters (Small, sharp)
        for _ in 0..50 {
            let angle = rng() * PI * 2.0;
            let dist = rng().powf(0.8);
            let pos = Vec2::new(angle.cos() * dist, angle.sin() * dist);

            self.moon_features.push(MoonFeature {
                pos,
                radius: 0.02 + rng() * 0.06,
                is_crater: true,
            });
        }

        self.init_done = true;
    }

    pub fn update(&mut self, ctx: &egui::Context) -> SplashStatus {
        self.is_dark = ctx.style().visuals.dark_mode;
        if !self.init_done {
            self.init_scene();
        }

        let now = ctx.input(|i| i.time);
        let dt = ctx.input(|i| i.stable_dt);

        if self.exit_start_time.is_none() {
            let t = (now - self.start_time) as f32;
            if t > ANIMATION_DURATION - 0.5 {
                if ctx.input(|i| i.pointer.any_click()) {
                    // Prevent click on theme switcher from triggering splash exit
                    let is_in_switcher = if let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) {
                        pos.x < 100.0 && pos.y < 60.0
                    } else {
                        false
                    };

                    if !is_in_switcher {
                        self.exit_start_time = Some(now);
                    }
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
            warp_progress = (dt / EXIT_DURATION).clamp(0.0, 1.0); // Linear global progress, curves applied per-voxel
        }

        ctx.request_repaint();

        // --- UPDATE CLOUDS ---
        let viewport_rect = ctx.input(|i| {
            i.viewport()
                .inner_rect
                .unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::ZERO))
        });
        // Fallback to expected size if viewport not ready
        let size = if viewport_rect.width() < 100.0 || viewport_rect.height() < 100.0 {
            Vec2::new(WINDOW_WIDTH, WINDOW_HEIGHT)
        } else {
            viewport_rect.size()
        };
        // Use window-local coords (0,0 origin) for consistency with paint()
        let rect = Rect::from_min_size(Pos2::ZERO, size);
        for cloud in &mut self.clouds {
            cloud.pos.x += cloud.velocity * dt;
            // Wrap around
            if cloud.pos.x > rect.width() / 2.0 + 300.0 {
                cloud.pos.x = -rect.width() / 2.0 - 300.0;
            }
        }

        if let Some(pointer) = ctx.input(|i| i.pointer.hover_pos()) {
            let center = rect.center();
            let tx = (pointer.x - center.x) / center.x;
            let ty = (pointer.y - center.y) / center.y;
            self.mouse_influence.x += (tx - self.mouse_influence.x) * 0.05;
            self.mouse_influence.y += (ty - self.mouse_influence.y) * 0.05;

            let cam_z_offset = warp_progress * 2000.0;
            let cam_dist =
                600.0 + smoothstep(0.0, ANIMATION_DURATION, physics_t) * 100.0 - cam_z_offset;

            let fov = 800.0;
            let mouse_wx = (pointer.x - center.x) * cam_dist / fov;
            let mouse_wy = -(pointer.y - center.y) * cam_dist / fov;
            self.mouse_world_pos = Vec3::new(mouse_wx, mouse_wy, 0.0);
        }

        if self.exit_start_time.is_none() {
            if t_abs < 0.8 {
                self.loading_text = "TRANSLATING...".to_string();
            } else if t_abs < 1.6 {
                self.loading_text = "OCR...".to_string();
            } else if t_abs < 2.4 {
                self.loading_text = "TRANSCRIBING...".to_string();
            } else {
                self.loading_text = "nganlinh4".to_string();
            }
        } else {
            self.loading_text = "READY TO ROCK!".to_string();
        }

        // --- PHYSICS UPDATE (Voxels) ---
        let helix_spin = physics_t * 2.0 + (physics_t * physics_t * 0.2);

        for v in &mut self.voxels {
            let my_start = START_TRANSITION + (v.noise_factor * 0.6);
            let my_end = my_start + 1.0;
            let progress = smoothstep(my_start, my_end, physics_t);

            if progress <= 0.0 {
                let current_h_y = v.helix_y + (physics_t * 2.0 + v.noise_factor * 10.0).sin() * 5.0;
                let current_angle = v.helix_angle_offset + helix_spin;
                let mut current_radius = v.helix_radius * (1.0 + physics_t * 0.1);

                if v.is_debris && physics_t > ANIMATION_DURATION * 0.7 {
                    let flare_start = ANIMATION_DURATION * 0.7;
                    let flare = (physics_t - flare_start).powi(2) * 20.0;
                    current_radius += flare;
                }

                v.pos = Vec3::new(
                    current_angle.cos() * current_radius,
                    current_h_y,
                    current_angle.sin() * current_radius,
                );
                v.rot.y += 0.05;
                v.scale = 0.8;
                v.velocity = Vec3::ZERO;
            } else {
                let current_h_y = v.helix_y + (physics_t * 2.0 + v.noise_factor * 10.0).sin() * 5.0;
                let current_angle = v.helix_angle_offset + helix_spin;

                let mut current_radius = v.helix_radius * (1.0 + physics_t * 0.1);
                if v.is_debris && physics_t > ANIMATION_DURATION * 0.7 {
                    let flare_start = ANIMATION_DURATION * 0.7;
                    let flare = (physics_t - flare_start).powi(2) * 20.0;
                    current_radius += flare;
                }

                let helix_pos = Vec3::new(
                    current_angle.cos() * current_radius,
                    current_h_y,
                    current_angle.sin() * current_radius,
                );
                let mut target_base = v.target_pos;

                // Add slow cosmic drift/orbit to debris targets so they don't look static
                if v.is_debris {
                    let orbit_speed = 0.02 + v.noise_factor * 0.08;
                    target_base = target_base.rotate_y(t_abs * orbit_speed);
                    target_base.y += (t_abs * 0.5 + v.noise_factor * 10.0).sin() * 20.0;
                }

                if warp_progress > 0.0 {
                    // "One by One" Departure:
                    // Stagger start times across the first 75% of the animation.
                    // Each particle moves for only the remaining 25% (0.4s) effectively.
                    let start_threshold = v.noise_factor * 0.75;
                    let move_duration = 0.25;

                    // Normalize progress to this particle's specific window
                    let local_linear =
                        ((warp_progress - start_threshold) / move_duration).clamp(0.0, 1.0);

                    // Cubic ease-in for explosive departure
                    let local_eased = local_linear * local_linear * local_linear;

                    if local_eased > 0.0 {
                        let radial = Vec3::new(v.pos.x, v.pos.y, 0.0).normalize();

                        // Swirl/Curl
                        let curl_angle = local_eased * (v.noise_factor - 0.5) * 6.0;
                        let swirl_vec = radial.rotate_z(curl_angle);

                        // Distance scaling - fast exit
                        let dist_mult = 1200.0;

                        target_base = target_base.add(swirl_vec.mul(local_eased * dist_mult));
                        target_base.z += local_eased * (v.noise_factor - 0.5) * 800.0;
                    }
                }

                let pos = helix_pos.lerp(target_base, progress);

                if progress > 0.9 && !v.is_debris && warp_progress == 0.0 {
                    let to_mouse = pos.sub(self.mouse_world_pos);
                    let dist_sq = to_mouse.x * to_mouse.x + to_mouse.y * to_mouse.y;
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

        SplashStatus::Ongoing
    }

    pub fn paint(&self, ctx: &egui::Context, _theme_mode: &crate::config::ThemeMode) -> bool {
        let mut theme_clicked = false;
        let now = ctx.input(|i| i.time);
        let t = (now - self.start_time) as f32;

        let mut warp_prog = 0.0;
        if let Some(exit_start) = self.exit_start_time {
            let dt = (now - exit_start) as f32;
            warp_prog = (dt / EXIT_DURATION).powi(5);
        }

        // FIX: The viewport's inner_rect returns SCREEN coordinates (window position on screen).
        // However, the layer_painter paints in WINDOW-LOCAL coordinates where (0,0) is top-left of window.
        // We must ALWAYS anchor our rect at (0,0) to ensure consistent positioning regardless of window location.
        let viewport_rect = ctx.input(|i| {
            i.viewport()
                .inner_rect
                .unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::ZERO))
        });

        // Use viewport size but ALWAYS anchor at (0,0) for window-local painting
        let size = if viewport_rect.width() < 100.0 || viewport_rect.height() < 100.0 {
            // Fallback during startup before window is fully realized
            Vec2::new(WINDOW_WIDTH, WINDOW_HEIGHT)
        } else {
            viewport_rect.size()
        };
        // CRITICAL: Always use Pos2::ZERO as origin - painter works in window-local coords
        let rect = Rect::from_min_size(Pos2::ZERO, size);

        // --- INTERACTION BLOCKER ---
        egui::Area::new(egui::Id::new("splash_blocker"))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                ui.allocate_response(
                    size,
                    egui::Sense::click_and_drag().union(egui::Sense::hover()),
                );
            });

        // Use a Foreground layer to paint ON TOP of the main UI
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("splash_overlay"),
        ));

        // Center is now correctly at (width/2, height/2) in window-local coords
        let center = rect.center();
        let _center_vec = Vec2::new(center.x, center.y);

        let alpha_fade_in = 0.4;
        let alpha = if t < alpha_fade_in {
            t / alpha_fade_in
        } else {
            1.0
        };
        let master_alpha = alpha.clamp(0.0, 1.0);

        // --- THEME SWITCHER OVERLAY (User can switch theme ABOVE splash) ---
        // Fade out over 0.3s when exit starts
        let switcher_alpha = if let Some(exit_start) = self.exit_start_time {
            let exit_dt = (now - exit_start) as f32;
            (1.0 - exit_dt / 0.3).max(0.0)
        } else {
            1.0
        };

        if master_alpha > 0.1 && switcher_alpha > 0.01 {
            egui::Area::new(egui::Id::new("splash_theme_switcher"))
                .order(egui::Order::Tooltip) // High priority above blocker
                .fixed_pos(Pos2::new(14.0, 11.0))
                .show(ctx, |ui| {
                    let icon = if self.is_dark { Icon::Sun } else { Icon::Moon };

                    // Pulse effect for the button
                    let pulse = (now * 2.0).sin().abs() * 0.2 + 0.8;
                    let btn_bg = if self.is_dark {
                        Color32::from_white_alpha((30.0 * switcher_alpha) as u8)
                    } else {
                        Color32::from_black_alpha((20.0 * switcher_alpha) as u8)
                    };

                    let icon_color = if self.is_dark {
                        Color32::WHITE.linear_multiply(switcher_alpha)
                    } else {
                        Color32::BLACK.linear_multiply(switcher_alpha)
                    };

                    let (rect, resp) =
                        ui.allocate_at_least(Vec2::splat(32.0), egui::Sense::click());

                    // Glass background
                    let fill = if resp.hovered() {
                        btn_bg.linear_multiply(1.5)
                    } else {
                        btn_bg.linear_multiply(pulse as f32)
                    };
                    ui.painter().rect_filled(rect, 8.0, fill);

                    // Seamless cutout for the Moon icon in Day mode
                    // We must override the GLOBAL context style because the icon painter uses painter.ctx().style()
                    let old_panel_fill = ctx.style().visuals.panel_fill;
                    if !self.is_dark {
                        // Also fade the moon cutout color
                        let cutout_color =
                            Color32::from_rgb(109, 174, 235).linear_multiply(switcher_alpha);
                        ctx.style_mut(|s| s.visuals.panel_fill = cutout_color);
                    }

                    // High-quality manual vector icon
                    paint_icon(ui.painter(), rect.shrink(6.0), icon, icon_color);

                    // Restore global style
                    ctx.style_mut(|s| s.visuals.panel_fill = old_panel_fill);

                    // Only allow clicks when fully visible
                    if resp.clicked() && switcher_alpha > 0.9 {
                        theme_clicked = true;
                    }
                });
        }

        // 1. Background
        // Startup: Fade from Solid Black (Night) or White (Day) to Target Color
        // This ensures the Main UI underneath is hidden start-up.
        let mut bg_color = if self.is_dark { C_VOID } else { C_SKY_DAY_TOP };
        if t < 0.5 {
            let t_fade = (t / 0.5).clamp(0.0, 1.0);
            let start_col = if self.is_dark {
                Color32::BLACK
            } else {
                Color32::WHITE
            };
            // Lerp r,g,b
            bg_color = Color32::from_rgb(
                lerp(start_col.r() as f32, bg_color.r() as f32, t_fade) as u8,
                lerp(start_col.g() as f32, bg_color.g() as f32, t_fade) as u8,
                lerp(start_col.b() as f32, bg_color.b() as f32, t_fade) as u8,
            );
        }

        // Exit: Fast fade out of background SKY only (reveals App UI)
        let sky_exit_fade = (1.0 - warp_prog * 4.0).clamp(0.0, 1.0);

        if self.is_dark {
            painter.rect_filled(rect, 12.0, bg_color.linear_multiply(sky_exit_fade));
        } else {
            // Gradient Sky with Rounded Corners
            let c_top = C_SKY_DAY_TOP.linear_multiply(sky_exit_fade);
            // We use a solid rounded rect for Day Mode to ensure the corners are perfectly
            // rounded as requested.
            painter.rect_filled(rect, 12.0, c_top);
        }

        if master_alpha <= 0.05 {
            return theme_clicked;
        }

        // --- LAYER 0: STARS ---
        // Parallax stars
        let star_offset = self.mouse_influence * -10.0;
        let star_time = t * 2.0;

        for (i, star) in self.stars.iter().enumerate() {
            let sx = rect.left() + (star.pos.x * rect.width()) + star_offset.x;
            let sy = rect.top() + (star.pos.y * rect.height()) + star_offset.y;

            // Random Fade Calculation (Decoupled from Sky)
            let rnd = ((i as f32 * 1.618).fract() + (star.pos.x * 10.0).fract()).fract();
            let start = rnd * 0.7; // Spread starts over 0.0 - 0.7
            let dur = 0.2;
            let local_fade = if warp_prog > 0.0 {
                let p = ((warp_prog - start) / dur).clamp(0.0, 1.0);
                1.0 - p
            } else {
                1.0
            };

            // Twinkle
            let twinkle = (star.phase + star_time).sin() * 0.3 + 0.7;
            let star_alpha =
                (star.brightness * twinkle * master_alpha * local_fade).clamp(0.0, 1.0);

            if star_alpha > 0.1 {
                let size = star.size * (1.0 - warp_prog);
                if self.is_dark {
                    painter.circle_filled(
                        Pos2::new(sx, sy),
                        size,
                        C_WHITE.linear_multiply(star_alpha),
                    );
                } else {
                    let day_star_alpha = star_alpha * 0.3;
                    painter.circle_filled(
                        Pos2::new(sx, sy),
                        size,
                        C_WHITE.linear_multiply(day_star_alpha),
                    );
                }
            }
        }

        // --- LAYER 1.5: GOD RAYS (DAY MODE) ---
        if !self.is_dark && master_alpha > 0.1 && warp_prog < 0.9 {
            let sun_pos = center + Vec2::new(0.0, -40.0 * (1.0 - warp_prog));
            let ray_count = 12;
            let ray_rot = t * 0.1;

            for i in 0..ray_count {
                let angle = (i as f32 / ray_count as f32) * PI * 2.0 + ray_rot;
                let next_angle = ((i as f32 + 0.5) / ray_count as f32) * PI * 2.0 + ray_rot;

                // Draw a large fan wedge (Original Style: Clear Center -> Visible Edge)
                let mut mesh = egui::Mesh::default();
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: sun_pos,
                    uv: Pos2::ZERO,
                    color: Color32::TRANSPARENT,
                });

                let ray_len = 1200.0;
                // Increased alpha from 30 to 55 to make it "pop"
                let c1 = Color32::from_white_alpha(55);

                let p1 = sun_pos + Vec2::new(angle.cos() * ray_len, angle.sin() * ray_len);
                let p2 =
                    sun_pos + Vec2::new(next_angle.cos() * ray_len, next_angle.sin() * ray_len);

                mesh.vertices.push(egui::epaint::Vertex {
                    pos: p1,
                    uv: Pos2::ZERO,
                    color: c1,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: p2,
                    uv: Pos2::ZERO,
                    color: c1,
                });

                mesh.add_triangle(0, 1, 2);
                painter.add(mesh);
            }
        }

        // --- LAYER 2: THE REALISTIC PINK MOON ---
        let moon_parallax = self.mouse_influence * -30.0;
        let moon_base_pos = center + Vec2::new(0.0, -40.0) + moon_parallax;
        let moon_rad = 140.0;
        let moon_alpha = master_alpha * (1.0 - warp_prog * 3.0).clamp(0.0, 1.0); // Simple fast fade for main body

        if moon_alpha > 0.01 {
            if self.is_dark {
                let moon_bob = (t * 0.5).sin() * 5.0;
                let final_moon_pos = moon_base_pos + Vec2::new(0.0, moon_bob);

                // 2a. Atmospheric Glow (Softer, layered)
                painter.circle_filled(
                    final_moon_pos,
                    moon_rad * 1.6,
                    C_MOON_GLOW.linear_multiply(0.03 * moon_alpha),
                );
                painter.circle_filled(
                    final_moon_pos,
                    moon_rad * 1.2,
                    C_MOON_GLOW.linear_multiply(0.08 * moon_alpha),
                );

                // 2b. Spherical Shading (Gradient Approximation)
                // Main body
                painter.circle_filled(
                    final_moon_pos,
                    moon_rad,
                    C_MOON_BASE.linear_multiply(moon_alpha),
                );
                // Shadow side (Bottom Right)
                painter.circle_filled(
                    final_moon_pos + Vec2::new(10.0, 10.0),
                    moon_rad * 0.9,
                    Color32::from_black_alpha((50.0 * moon_alpha) as u8),
                );
                // Highlight side (Top Left)
                painter.circle_filled(
                    final_moon_pos - Vec2::new(10.0, 10.0),
                    moon_rad * 0.85,
                    Color32::from_white_alpha((20.0 * moon_alpha) as u8),
                );

                // 2c. Surface Features
                let feature_rot = t * 0.05;

                for feat in &self.moon_features {
                    let fx = feat.pos.x;
                    let fy = feat.pos.y;

                    // Rotation
                    let rot_cos = feature_rot.cos();
                    let rot_sin = feature_rot.sin();
                    let r_x = fx * rot_cos - fy * rot_sin;
                    let r_y = fx * rot_sin + fy * rot_cos;

                    // Sphere Projection (Fake Z)
                    let dist_sq = r_x * r_x + r_y * r_y;
                    if dist_sq > 0.95 {
                        continue;
                    } // Clip edge features

                    let f_pos = final_moon_pos + Vec2::new(r_x * moon_rad, r_y * moon_rad);

                    // Perspective distortion
                    let z_depth = (1.0 - dist_sq).sqrt(); // 1.0 at center, 0.0 at edge
                    let f_radius = feat.radius * moon_rad * (0.5 + 0.5 * z_depth);
                    let f_alpha = moon_alpha * z_depth; // Fade near edges

                    if feat.is_crater {
                        // Crater: Recessed shadowing
                        // Shadow (Top Left inner)
                        painter.circle_filled(
                            f_pos + Vec2::new(-1.0, -1.0),
                            f_radius,
                            C_MOON_SHADOW.linear_multiply(f_alpha * 0.8),
                        );
                        // Highlight (Bottom Right inner)
                        painter.circle_filled(
                            f_pos + Vec2::new(1.0, 1.0),
                            f_radius * 0.9,
                            C_MOON_HIGHLIGHT.linear_multiply(f_alpha * 0.4),
                        );
                    } else {
                        // Maria: Flat dark patches
                        painter.circle_filled(
                            f_pos,
                            f_radius,
                            C_MOON_SHADOW.linear_multiply(f_alpha * 0.3),
                        );
                    }
                }

                // 2d. Rim Light (Top Left)
                // Simulate light hitting the edge of the sphere
                painter.circle_stroke(
                    final_moon_pos - Vec2::new(2.0, 2.0),
                    moon_rad - 1.0,
                    Stroke::new(2.0, C_MOON_HIGHLIGHT.linear_multiply(0.4 * moon_alpha)),
                );
            } else {
                // --- SUN VARIANT ---
                let sun_bob = (t * 0.5).sin() * 5.0;
                let final_sun_pos = moon_base_pos + Vec2::new(0.0, sun_bob);

                // Glow
                painter.circle_filled(
                    final_sun_pos,
                    moon_rad * 2.0,
                    C_SUN_GLOW.linear_multiply(0.1 * moon_alpha),
                );
                painter.circle_filled(
                    final_sun_pos,
                    moon_rad * 1.4,
                    C_SUN_GLOW.linear_multiply(0.2 * moon_alpha),
                );

                // Sun Body
                painter.circle_filled(
                    final_sun_pos,
                    moon_rad,
                    C_SUN_BODY.linear_multiply(moon_alpha),
                );

                // Sun Spots (Reusing moon features)
                let feature_rot = t * 0.08;
                for feat in &self.moon_features {
                    let fx = feat.pos.x;
                    let fy = feat.pos.y;

                    let rot_cos = feature_rot.cos();
                    let rot_sin = feature_rot.sin();
                    let r_x = fx * rot_cos - fy * rot_sin;
                    let r_y = fx * rot_sin + fy * rot_cos;

                    let dist_sq = r_x * r_x + r_y * r_y;
                    if dist_sq > 0.95 {
                        continue;
                    }

                    let f_pos = final_sun_pos + Vec2::new(r_x * moon_rad, r_y * moon_rad);
                    let z_depth = (1.0 - dist_sq).sqrt();
                    let f_radius = feat.radius * moon_rad * (0.5 + 0.5 * z_depth);
                    let f_alpha = moon_alpha * z_depth;

                    if feat.is_crater {
                        // Sunspots (Darker, sharper, smaller)
                        painter.circle_filled(
                            f_pos,
                            f_radius * 0.6, // Smaller than craters
                            Color32::from_rgb(160, 60, 0).linear_multiply(f_alpha * 0.8),
                        );
                    } else {
                        // Hot Flares/Patches (Bright, glowy) - remove dark rims to avoid crater look
                        painter.circle_filled(
                            f_pos,
                            f_radius * 1.5, // Larger soft glow
                            C_SUN_FLARE.linear_multiply(f_alpha * 0.3),
                        );
                        painter.circle_filled(
                            f_pos,
                            f_radius * 0.8,
                            C_WHITE.linear_multiply(f_alpha * 0.5), // Hot center
                        );
                    }
                }

                // Rim Light
                painter.circle_stroke(
                    final_sun_pos,
                    moon_rad - 1.0,
                    Stroke::new(3.0, C_SUN_HIGHLIGHT.linear_multiply(0.5 * moon_alpha)),
                );
            }
        }

        // --- LAYER 3: VOLUMETRIC DARK CLOUDS (BLACK SILHOUETTE) ---
        let cloud_parallax = self.mouse_influence * -15.0;

        // Clip clouds in Day Mode so they don't enter the "Sea" (Stairs)
        // The Sea is composed of lines that start roughly 18px below the horizon (perspective).
        // We extend the clip rect down by 30px to ensure the clouds are drawn behind the top-most dense stairs,
        // eliminating any gap between the sky and the sea.
        let horizon = center.y + 120.0;
        let cloud_painter = if !self.is_dark {
            painter.with_clip_rect(Rect::from_min_max(
                rect.min,
                Pos2::new(rect.max.x, horizon + 30.0),
            ))
        } else {
            painter.clone()
        };

        for (i, cloud) in self.clouds.iter().enumerate() {
            let c_x = center.x + cloud.pos.x + cloud_parallax.x;
            let c_y = center.y + cloud.pos.y + cloud_parallax.y;

            // Random Fade (Decoupled)
            let rnd = (i as f32 * 0.73).fract();
            let start = rnd * 0.6; // Spread over 0.0 - 0.6
            let dur = 0.3;
            let local_fade = if warp_prog > 0.0 {
                let p = ((warp_prog - start) / dur).clamp(0.0, 1.0);
                1.0 - p
            } else {
                1.0
            };

            let cloud_alpha = cloud.opacity * master_alpha * local_fade;

            if cloud_alpha > 0.01 {
                // Pass 1: Dark Core (Deep black shadow)
                for (offset, puff_r_mult) in &cloud.puffs {
                    let p_pos = Pos2::new(c_x, c_y) + (*offset * cloud.scale);
                    let radius = 30.0 * cloud.scale * puff_r_mult;

                    let core_col = if self.is_dark {
                        C_CLOUD_CORE.linear_multiply(cloud_alpha * 0.95)
                    } else {
                        C_CLOUD_WHITE.linear_multiply(cloud_alpha * 0.95)
                    };

                    cloud_painter.circle_filled(
                        p_pos + Vec2::new(2.0, 5.0), // Shadow offset down-right
                        radius,
                        core_col,
                    );
                }

                // Note: Second pass (Main Body with highlight) was intentionally removed
            }
        }

        // --- LAYER 4: RETRO GRID ---
        let render_t = t.min(ANIMATION_DURATION + 5.0);
        let cam_y = 150.0 + (render_t * 30.0) + (warp_prog * 10000.0);
        // horizon is already defined above
        let grid_fade = if warp_prog > 0.0 { 1.0 } else { 1.0 }; // Handled by local_fade now

        if grid_fade > 0.0 {
            // Horizontal lines
            for i in 0..16 {
                // Random Grid Line Fade
                let rnd = (i as f32 * 0.9).sin() * 0.5 + 0.5;
                let start = rnd * 0.5;
                let dur = 0.25;
                let local_fade = if warp_prog > 0.0 {
                    let p = ((warp_prog - start) / dur).clamp(0.0, 1.0);
                    1.0 - p
                } else {
                    1.0
                };

                if local_fade <= 0.0 {
                    continue;
                }

                let z_dist = 1.0 + (i as f32 * 0.5) - ((cam_y * 0.05) % 0.5);
                let perspective = 250.0 / (z_dist - warp_prog * 0.8).max(0.1);
                let y = horizon + perspective * 0.6;

                if y > rect.bottom() || y < horizon {
                    continue;
                }

                let w = rect.width() * (2.5 / z_dist);
                let x1 = center.x - w;
                let x2 = center.x + w;

                // Distance fade + Random Line Fade
                let alpha_grid = (1.0 - (y - horizon) / (rect.bottom() - horizon)).powf(0.5)
                    * master_alpha
                    * 0.5
                    * local_fade;

                let (grid_col, thickness) = if self.is_dark {
                    (C_MAGENTA, 1.5)
                } else {
                    // Day Mode: Thicker Blue "Stairs"
                    (C_DAY_REP, 4.0 * (1.0 - (y - horizon) / rect.height()))
                };

                painter.line_segment(
                    [Pos2::new(x1, y), Pos2::new(x2, y)],
                    Stroke::new(thickness, grid_col.linear_multiply(alpha_grid)),
                );
            }
        }

        // --- LAYER 5: 3D VOXELS (SPHERES) ---
        let physics_t = t.min(ANIMATION_DURATION);
        let fov = 800.0;
        let cam_fly_dist = warp_prog * 2000.0;
        let cam_dist = (600.0 + smoothstep(0.0, 8.0, physics_t) * 100.0) - cam_fly_dist;

        let global_rot = Vec3::new(
            self.mouse_influence.y * 0.2,
            self.mouse_influence.x * 0.2,
            0.0,
        );

        // Light direction highlight offset (Top-Left)
        let light_dir_2d = Vec2::new(-0.4, -0.4);

        // Store: (Z-depth, ScreenPos, Radius, BaseColor, IsGlowing/White, IsDebris)
        let mut draw_list: Vec<(f32, Pos2, f32, Color32, bool, bool)> =
            Vec::with_capacity(self.voxels.len());

        let sphere_radius_base = 8.5; // Overlap for pipe look

        for v in &self.voxels {
            let mut local_debris_alpha = 1.0;
            if v.is_debris {
                let fade_start = 4.0 + (v.noise_factor * 3.0);
                let fade_end = fade_start + 2.5;
                local_debris_alpha = 1.0 - smoothstep(fade_start, fade_end, physics_t);
                if local_debris_alpha <= 0.01 {
                    continue;
                }
            }

            let mut v_center = v.pos;
            v_center = v_center
                .rotate_x(global_rot.x)
                .rotate_y(global_rot.y)
                .rotate_z(global_rot.z);

            let z_depth = cam_dist - v_center.z;
            if z_depth < 0.1 {
                continue;
            }

            let scale = fov / z_depth;
            let screen_pos =
                Pos2::new(center.x + v_center.x * scale, center.y - v_center.y * scale);

            // Radius calculation
            let r = sphere_radius_base * v.scale * scale;

            // Color Logic
            let mut alpha_local = master_alpha;
            if v.is_debris {
                alpha_local *= local_debris_alpha;

                // PREMIUM: Diverse opacity based on distance/noise
                let base_opacity = 0.4 + (v.noise_factor * 0.6);
                alpha_local *= base_opacity;

                // PREMIUM: Add subtle cosmic twinkle/glimmer
                let twinkle =
                    (t * (3.0 + v.noise_factor * 2.0) + v.noise_factor * 50.0).sin() * 0.25 + 0.75;
                alpha_local *= twinkle;
            }

            let mut base_col = v.color;

            // DYNAMIC COLOR SWAP: Fix sphere colors when theme changes on splash
            if !v.is_debris && v.color != C_WHITE {
                if self.is_dark {
                    // Switch to Night Colors
                    if v.color == C_DAY_REP {
                        base_col = C_MAGENTA;
                    } else if v.color == C_DAY_SEC {
                        base_col = C_CYAN;
                    }
                } else {
                    // Switch to Day Colors
                    if v.color == C_MAGENTA {
                        base_col = C_DAY_REP;
                    } else if v.color == C_CYAN {
                        base_col = C_DAY_SEC;
                    }
                }
            }

            // Day mode debris fix
            if !self.is_dark && v.is_debris {
                base_col = C_CLOUD_WHITE;
            }

            if warp_prog > 0.0 {
                // Exact match of physics timing
                let start_threshold = v.noise_factor * 0.75;
                let move_duration = 0.25;
                let local_linear = ((warp_prog - start_threshold) / move_duration).clamp(0.0, 1.0);

                // Fade out halfway through its flight
                let fade = (local_linear * 1.5).clamp(0.0, 1.0);
                alpha_local *= 1.0 - fade;
            }

            let final_col = base_col.linear_multiply(alpha_local);

            draw_list.push((
                z_depth,
                screen_pos,
                r,
                final_col,
                v.color == C_WHITE || v.color == C_DAY_SEC,
                v.is_debris,
            ));
        }

        // Sort back-to-front (Z-Painter's Algorithm)
        draw_list.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));

        for (_, pos, r, col, is_white_voxel, is_debris) in draw_list {
            // 1. Shadow/Base (The "Rim" on the shadow side)
            // Use the clipped cloud painter for debris in Day mode so they don't enter the sea
            let p = if !self.is_dark && is_debris {
                &cloud_painter
            } else {
                &painter
            };

            // 1. Shadow/Base (The "Rim" on the shadow side)
            if !(!self.is_dark && is_debris) {
                let shadow_col = if self.is_dark {
                    Color32::from_black_alpha(200).linear_multiply(col.a() as f32 / 255.0)
                } else {
                    // In Day mode, white voxels get a blueish/grey shadow to define shape
                    if is_white_voxel {
                        Color32::from_rgb(100, 120, 150).linear_multiply(col.a() as f32 / 255.0)
                    } else {
                        // Blue voxels get dark blue shadow
                        Color32::from_rgb(0, 40, 100).linear_multiply(col.a() as f32 / 255.0)
                    }
                };
                p.circle_filled(pos, r, shadow_col);

                // 2. Main Body (Shifted towards light to create crescent shadow)
                let body_offset = light_dir_2d * (r * 0.15);
                p.circle_filled(pos + body_offset, r * 0.85, col);

                // 3. Inner Gradient / Glow (Soft light in center)
                let glow_col = if is_white_voxel {
                    Color32::WHITE.linear_multiply(0.5)
                } else {
                    col.linear_multiply(0.5)
                };
                let gradient_offset = light_dir_2d * (r * 0.3);
                p.circle_filled(pos + gradient_offset, r * 0.5, glow_col);
            } else {
                // Day Mode Debris: Pure white soft-edged "puffs" (no shading, no border)
                p.circle_filled(pos, r, col);
            }

            // 4. Specular Highlight (Sharp Reflection) - ONLY FOR MAIN LOGO VOXELS
            if !is_debris {
                let highlight_pos = pos + (light_dir_2d * (r * 0.5));
                let highlight_alpha = if self.is_dark { 0.8 } else { 0.9 };
                let highlight_col = Color32::from_white_alpha((255.0 * highlight_alpha) as u8)
                    .linear_multiply(col.a() as f32 / 255.0);

                painter.circle_filled(highlight_pos, r * 0.25, highlight_col);
                painter.circle_filled(
                    highlight_pos,
                    r * 0.15,
                    Color32::WHITE.linear_multiply(col.a() as f32 / 255.0),
                ); // Hotspot
            }
        }

        // --- LAYER 6: UI TEXT ---
        if master_alpha > 0.1 && warp_prog < 0.1 {
            let ui_alpha = 1.0 - (warp_prog * 10.0).clamp(0.0, 1.0);

            // UI Colors based on theme
            let ui_text_col = if self.is_dark { C_WHITE } else { C_DAY_TEXT };
            let ui_color = ui_text_col.linear_multiply(master_alpha * ui_alpha);

            // Loading text color (Orange in Day)
            let loading_col = if self.is_dark {
                C_CYAN.linear_multiply(master_alpha * ui_alpha)
            } else {
                C_DAY_TEXT.linear_multiply(master_alpha * ui_alpha)
            };

            // Click Text Color (Cyan in Night, White in Day)
            let click_col = if self.is_dark {
                C_CYAN.linear_multiply(master_alpha * ui_alpha)
            } else {
                C_WHITE.linear_multiply(master_alpha * ui_alpha)
            };

            let magenta_color = if self.is_dark {
                C_MAGENTA.linear_multiply(master_alpha * ui_alpha)
            } else {
                C_DAY_REP.linear_multiply(master_alpha * ui_alpha)
            };

            let title_text = format!("SCREEN GOATED TOOLBOX {}", env!("CARGO_PKG_VERSION"));
            let title_font = FontId::proportional(30.0); // Increased size
            let title_pos = center + Vec2::new(0.0, 150.0);

            // Stylized Shadow Colors
            let shadow_col = if self.is_dark {
                C_MAGENTA.linear_multiply(master_alpha * ui_alpha) // Retro Pink Shadow
            } else {
                C_WHITE.linear_multiply(master_alpha * ui_alpha) // Crisp White Shadow
            };

            // Stylized Bold/Shadow: Draw distinct color offset
            painter.text(
                title_pos + Vec2::new(2.0, 2.0), // Increased offset for better visibility
                Align2::CENTER_TOP,
                &title_text,
                title_font.clone(),
                shadow_col,
            );
            painter.text(
                title_pos,
                Align2::CENTER_TOP,
                &title_text,
                title_font,
                ui_color,
            );
            painter.text(
                center + Vec2::new(0.0, 210.0),
                Align2::CENTER_TOP,
                &self.loading_text,
                FontId::monospace(12.0),
                loading_col,
            );

            let bar_rect =
                Rect::from_center_size(center + Vec2::new(0.0, 230.0), Vec2::new(200.0, 4.0));
            // Bar Background
            let bar_bg_col = if self.is_dark {
                Color32::from_white_alpha((30.0 * ui_alpha) as u8)
            } else {
                Color32::from_black_alpha((30.0 * ui_alpha) as u8)
            };
            painter.rect_filled(bar_rect, 2.0, bar_bg_col);
            let prog = (physics_t / (ANIMATION_DURATION - 0.5)).clamp(0.0, 1.0);
            let mut fill = bar_rect;
            fill.set_width(bar_rect.width() * prog);
            painter.rect_filled(fill, 2.0, magenta_color);

            if t > ANIMATION_DURATION - 0.5 {
                let pulse = (t * 5.0).sin().abs() * 0.7 + 0.3;
                painter.text(
                    center - Vec2::new(0.0, 220.0),
                    Align2::CENTER_TOP,
                    "Click anywhere to continue",
                    FontId::proportional(14.0),
                    click_col.linear_multiply(pulse),
                );
            }
        }
        theme_clicked
    }
}
