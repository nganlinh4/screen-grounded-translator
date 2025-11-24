// Procedural Pixel Art Generator for Broom Cursor
// Generates 9 frames of animation

pub const BROOM_W: i32 = 32;
pub const BROOM_H: i32 = 32;

#[derive(Clone, Copy)]
pub enum BroomState {
    Idle1,
    Idle2,
    Left,
    Right,
    Sweep1Windup,
    Sweep2Smash,
    Sweep3DragR,
    Sweep4DragL,
    Sweep5Lift,
}

pub fn get_broom_pixels(state: BroomState) -> Vec<u32> {
    let mut pixels = vec![0u32; (BROOM_W * BROOM_H) as usize];

    // Palette
    const _T: u32 = 0x00000000; // Transparent
    const H_DK: u32 = 0xFF5D4037; // Handle Dark
    const H_LT: u32 = 0xFF8D6E63; // Handle Light
    const BAND: u32 = 0xFFB71C1C; // Band Red
    const S_DK: u32 = 0xFFFBC02D; // Straw Dark
    const S_LT: u32 = 0xFFFFF176; // Straw Light
    const S_SH: u32 = 0xFFF57F17; // Straw Shadow

    let set_px = |p: &mut Vec<u32>, x: i32, y: i32, c: u32| {
        if x >= 0 && x < BROOM_W && y >= 0 && y < BROOM_H {
            p[(y * BROOM_W + x) as usize] = c;
        }
    };

    // Center bottom reference
    let cx = 16.0;
    let mut cy = 28.0;

    // Animation Params
    let mut tilt: f32 = 0.0; // degrees
    let mut squish = 1.0;
    let mut spread = 0.0;
    // head_offset_x moves the connection point (Band + Handle Base)
    let mut head_offset_x = 0.0; 

    match state {
        BroomState::Idle1 => { cy -= 0.0; }
        BroomState::Idle2 => { cy -= 1.0; } // Bob up
        BroomState::Left => { 
            // Mouse moves Left -> Head drags Right -> Handle tilts Right (Top-Left to Bottom-Right)
            // Positive Rotation
            tilt = 20.0; 
            head_offset_x = 4.0; 
            spread = -1.0; 
        }
        BroomState::Right => { 
            // Mouse moves Right -> Head drags Left -> Handle tilts Left (Top-Right to Bottom-Left)
            // Negative Rotation
            tilt = -20.0; 
            head_offset_x = -4.0; 
            spread = 1.0; 
        }
        BroomState::Sweep1Windup => {
            tilt = -25.0; // Wind up back
            cy -= 5.0;
            head_offset_x = -2.0;
        }
        BroomState::Sweep2Smash => {
            tilt = 0.0;
            squish = 0.5; // Big squish
            cy += 2.0;
            spread = 5.0;
        }
        BroomState::Sweep3DragR => {
            tilt = 15.0;
            squish = 0.8;
            head_offset_x = 4.0;
        }
        BroomState::Sweep4DragL => {
            tilt = -15.0;
            squish = 0.8;
            head_offset_x = -4.0;
        }
        BroomState::Sweep5Lift => {
            tilt = 0.0;
            cy -= 3.0;
        }
    }

    // The calculated center of the broom head (Band position)
    let head_cx = cx + head_offset_x;

    // 1. Draw Bristles
    let bristle_h = (10.0 * squish) as i32;
    let top_w = 8.0;
    let bot_w = 14.0 + spread;

    for y in 0..bristle_h {
        let prog = y as f32 / bristle_h as f32;
        let cur_w = top_w + (bot_w - top_w) * prog;
        
        // Center bristles on the shifted head center
        let start_x = head_cx - (cur_w / 2.0);
        
        let mut skew = 0.0;
        let rad = tilt.to_radians();
        
        // Calculate Skew based on tilt to align with handle angle
        let tilt_skew = rad.tan() * (bristle_h - y) as f32;

        match state {
            // Apply tilt skew so bristles look continuous with handle
            BroomState::Left | BroomState::Right | BroomState::Sweep1Windup => skew = tilt_skew,
            // During sweep drag, apply exaggerated drag skew
            BroomState::Sweep3DragR => skew = -3.0 * prog,
            BroomState::Sweep4DragL => skew = 3.0 * prog,
            BroomState::Sweep2Smash => skew = 0.0, // Flat smash
            _ => {}
        }

        let py = (cy as i32) - bristle_h + y;
        let start_x_int = (start_x + skew).round() as i32;
        let end_x_int = (start_x + skew + cur_w).round() as i32;

        for px in start_x_int..end_x_int {
            // Texture
            let is_shadow = (px + y) % 3 == 0;
            let is_light = (px - y) % 4 == 0;
            let col = if is_shadow { S_SH } else if is_light { S_LT } else { S_DK };
            set_px(&mut pixels, px, py, col);
        }
    }

    // 2. Draw Band (The Red Connector)
    let band_y = (cy as i32) - bristle_h;
    let band_h = 3;
    for y in 0..band_h {
        let py = band_y - y;
        let start_x = (head_cx - (top_w / 2.0)).round() as i32;
        
        // Tilt band slightly
        let rad = tilt.to_radians();
        let _tilt_off = (rad.tan() * (bristle_h + y) as f32).round() as i32;
        
        // Note: We intentionally don't add tilt_off here because head_cx assumes 
        // the BASE of the handle. The band sits right at that base.
        // However, to make it look 3D, we can shift it slightly if tilted strongly.
        let fine_tune = if tilt.abs() > 10.0 { (tilt / 10.0) as i32 } else { 0 };

        for px in (start_x + fine_tune)..(start_x + fine_tune + top_w as i32) {
            set_px(&mut pixels, px, py, BAND);
        }
    }

    // 3. Draw Handle
    let handle_len = 16;
    let handle_start_y = band_y - band_h;
    
    for i in 0..handle_len {
        let rad = tilt.to_radians();
        
        // Pivot point is exactly `head_cx`
        let pivot_x = head_cx;
        let pivot_y = handle_start_y as f32;
        
        // Point relative to pivot (going UP)
        let rel_y = -(i as f32);
        
        // Rotate
        let rot_x = -rel_y * rad.sin();
        let rot_y = rel_y * rad.cos();
        
        let px = (pivot_x + rot_x).round() as i32;
        let py = (pivot_y + rot_y).round() as i32;
        
        set_px(&mut pixels, px, py, H_DK);
        set_px(&mut pixels, px + 1, py, H_LT);
    }

    pixels
}
