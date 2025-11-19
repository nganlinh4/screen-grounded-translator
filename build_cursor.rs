use std::fs::File;
use std::io::{Write, Result};

fn main() -> Result<()> {
    let width = 32u32;
    let height = 32u32;

    // --- CONFIGURATION ---
    
    // Hotspot: The "click" point. 
    // Placed at the bottom-left tip of the bristles.
    let hotspot_x = 3u16;
    let hotspot_y = 28u16;

    // --- PALETTE (ARGB) ---
    let t   = 0x00_00_00_00; // Transparent
    let out = 0xFF_1a_1a_1a; // Dark Outline (Soft Black)
    
    // Shadows
    let shd = 0x60_00_00_00; // Hard Drop Shadow
    let sft = 0x30_00_00_00; // Soft Shadow edge

    // Wood Handle
    let w_d = 0xFF_5C_40_33; // Dark Wood
    let w_m = 0xFF_8B_5A_2B; // Mid Wood
    let w_l = 0xFF_DE_B8_87; // Light Wood (Burlywood)

    // Bristles (Straw)
    let s_d = 0xFF_DA_A5_20; // Dark Straw (GoldenRod)
    let s_m = 0xFF_FF_D7_00; // Mid Straw (Gold)
    let s_l = 0xFF_FF_E4_B5; // Light Straw (Moccasin)

    // Binding (The rope/band)
    let b_d = 0xFF_8B_00_00; // Dark Red
    let b_l = 0xFF_FF_45_00; // Orange Red Highlight

    // 32x32 Grid - The "Chibi" Broom
    // Legend:
    // . = Transparent
    // # = Outline
    // h = Hard Shadow, s = Soft Shadow
    // 1=WoodDark, 2=WoodMid, 3=WoodLight
    // 4=StrawDark, 5=StrawMid, 6=StrawLight
    // 7=BindDark, 8=BindLight
    let art_rows = [
        "................................",
        ".......................####.....",
        "......................#1221#....", // Knob at end of handle
        ".....................#12332#....",
        "....................#1221##.....",
        "...................#121#........",
        "..................#221#.........",
        ".................#132#..........", // Thick handle shaft
        "................#221#...........",
        "...............#132#............",
        "..............#221#.............",
        ".............#132#..............",
        "............#221#...............",
        "...........#####................",
        "..........#7877#................", // Red Binding
        ".........#787787#...............",
        "........#7787777#...............",
        ".......#44556554#...............", // Top of bristles
        "......#4455665544#..............",
        ".....#45556665554#..............",
        "....#455556655554#..............", // Wide body
        "...#4555555555554#..............",
        "..#44555555555544#..............",
        ".#44555555555544#...............",
        "#4554#45555554#4#...............", // Ragged bottom
        "#####.########.##...............",
        ".ss....ssssss...................", // Drop shadow
        "..hh..hhhhhh....................",
        "...hhhhhhhh.....................",
        "....ssssss......................",
        "................................",
        "................................",
    ];

    // --- PARSING & GENERATION ---
    
    let mut pixels: Vec<u32> = Vec::with_capacity((width * height) as usize);

    for row in art_rows {
        for char in row.chars() {
            let color = match char {
                '.' => t,
                '#' => out,
                'h' => shd,
                's' => sft,
                '1' => w_d,
                '2' => w_m,
                '3' => w_l,
                '4' => s_d,
                '5' => s_m,
                '6' => s_l,
                '7' => b_d,
                '8' => b_l,
                _ => t,
            };
            pixels.push(color);
        }
    }
    
    let mut cursor_data = Vec::new();

    // 1. ICONDIR
    cursor_data.extend_from_slice(&[0, 0]); // Reserved
    cursor_data.extend_from_slice(&[2, 0]); // Type = Cursor
    cursor_data.extend_from_slice(&[1, 0]); // Count = 1

    // 2. ICONDIRENTRY
    cursor_data.push(width as u8);
    cursor_data.push(height as u8);
    cursor_data.push(0); // Palette (0=Truecolor)
    cursor_data.push(0); // Reserved
    cursor_data.extend_from_slice(&hotspot_x.to_le_bytes());
    cursor_data.extend_from_slice(&hotspot_y.to_le_bytes());
    
    // Placeholder for size and offset
    let size_offset_idx = cursor_data.len();
    cursor_data.extend_from_slice(&[0, 0, 0, 0]); 
    cursor_data.extend_from_slice(&[22, 0, 0, 0]); // Offset constant

    let bmp_start = cursor_data.len();

    // 3. BITMAPINFOHEADER
    cursor_data.extend_from_slice(&40u32.to_le_bytes()); // Size
    cursor_data.extend_from_slice(&(width as i32).to_le_bytes());
    cursor_data.extend_from_slice(&((height * 2) as i32).to_le_bytes());
    cursor_data.extend_from_slice(&1u16.to_le_bytes()); // Planes
    cursor_data.extend_from_slice(&32u16.to_le_bytes()); // BitCount
    cursor_data.extend_from_slice(&0u32.to_le_bytes()); // Compression
    cursor_data.extend_from_slice(&0u32.to_le_bytes()); // SizeImage
    cursor_data.extend_from_slice(&0i32.to_le_bytes());
    cursor_data.extend_from_slice(&0i32.to_le_bytes());
    cursor_data.extend_from_slice(&0u32.to_le_bytes());
    cursor_data.extend_from_slice(&0u32.to_le_bytes());

    // 4. Pixel Data (XOR Mask) - Write Bottom-Up
    for y in (0..height).rev() {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            let pixel = pixels[idx];
            
            // BGRA
            let a = (pixel >> 24) as u8;
            let r = (pixel >> 16) as u8;
            let g = (pixel >> 8) as u8;
            let b = pixel as u8;
            
            cursor_data.extend_from_slice(&[b, g, r, a]);
        }
    }

    // 5. AND Mask (1-bit transparency)
    let row_padding = ((width + 31) / 32) * 4 - ((width + 7) / 8);
    for y in (0..height).rev() {
        let mut byte = 0u8;
        for x in 0..width {
            let idx = (y * width + x) as usize;
            let alpha = (pixels[idx] >> 24) as u8;
            if alpha == 0 { byte |= 1 << (7 - (x % 8)); }
            
            if (x + 1) % 8 == 0 {
                cursor_data.push(byte);
                byte = 0;
            }
        }
        if width % 8 != 0 { cursor_data.push(byte); }
        for _ in 0..row_padding { cursor_data.push(0); }
    }

    // Patch file size
    let size = (cursor_data.len() - bmp_start) as u32;
    let sb = size.to_le_bytes();
    cursor_data[size_offset_idx] = sb[0];
    cursor_data[size_offset_idx+1] = sb[1];
    cursor_data[size_offset_idx+2] = sb[2];
    cursor_data[size_offset_idx+3] = sb[3];

    let mut file = File::create("broom.cur")?;
    file.write_all(&cursor_data)?;

    println!("✅ Created 'broom.cur'");
    println!("   Style: Stout, RPG-style, rounded handle, bushy bristles.");
    Ok(())
}