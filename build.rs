use std::path::Path;
use std::fs;
use std::io::{Write, Cursor};

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    
    // Ensure assets directory exists
    let assets_dir = Path::new(&manifest_dir).join("assets");
    let _ = fs::create_dir_all(&assets_dir);
    
    // Copy tray icon PNG
    let icon_path = Path::new(&manifest_dir).join("icon.png");
    if icon_path.exists() {
        let tray_icon_path = assets_dir.join("tray_icon.png");
        let _ = fs::copy(&icon_path, &tray_icon_path);
    }
    
    // Generate multi-size ICO
    let app_icon_path = assets_dir.join("app-icon.png");
    if app_icon_path.exists() {
        let ico_path = assets_dir.join("app.ico");
        create_multi_size_ico(&app_icon_path, &ico_path);
    }
    
    // Embed icon in Windows executable using manual windres compilation
    #[cfg(target_os = "windows")]
    {
        let ico_path = Path::new(&manifest_dir).join("assets").join("app.ico");
        let rc_path = Path::new(&manifest_dir).join("app.rc");
        
        println!("cargo:warning=Checking icon path: {:?}", ico_path);
        
        if ico_path.exists() && rc_path.exists() {
            println!("cargo:warning=Found icon and rc file, compiling resources manually...");
            
            // Define output path for the object file in the OUT_DIR
            let out_dir = std::env::var("OUT_DIR").unwrap();
            let res_path = Path::new(&out_dir).join("resources.o");
            
            // Run windres manually
            // windres app.rc -o resources.o
            let status = std::process::Command::new("windres")
                .arg(&rc_path)
                .arg("-o")
                .arg(&res_path)
                .status();
                
            match status {
                Ok(s) if s.success() => {
                    println!("cargo:warning=Manual windres compilation successful!");
                    // Tell Cargo to pass the object file to the linker
                    println!("cargo:rustc-link-arg={}", res_path.display());
                },
                Ok(s) => {
                    println!("cargo:warning=Manual windres compilation failed with exit code: {}", s);
                    panic!("windres failed");
                },
                Err(e) => {
                    println!("cargo:warning=Failed to execute windres: {}", e);
                    panic!("Failed to execute windres: {}", e);
                }
            }
        } else {
            println!("cargo:warning=Missing app.ico or app.rc!");
        }
    }
    
    println!("cargo:rerun-if-changed=assets/app-icon.png");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=build.rs");
}

fn create_multi_size_ico(png_path: &Path, ico_path: &Path) {
    let img = image::open(png_path).expect("Failed to open PNG");
    let mut file = fs::File::create(ico_path).expect("Failed to create ICO");
    
    let sizes = [16, 32, 48, 64, 256];
    let num_images = sizes.len() as u16;
    
    // ICO Header
    file.write_all(&[0, 0]).unwrap(); // Reserved
    file.write_all(&[1, 0]).unwrap(); // Type 1 (Icon)
    file.write_all(&num_images.to_le_bytes()).unwrap();
    
    let mut offset = 6 + (16 * num_images as u32);
    
    // Prepare image data
    let mut images_data: Vec<Vec<u8>> = Vec::new();
    
    for &size in &sizes {
        let mut data = Vec::new();
        
        if size == 256 {
            // Use PNG format for 256x256 (Vista+)
            let resized = img.resize(size, size, image::imageops::FilterType::Lanczos3);
            let mut buffer = Cursor::new(Vec::new());
            resized.write_to(&mut buffer, image::ImageOutputFormat::Png).unwrap();
            data = buffer.into_inner();
        } else {
            // BMP format for smaller sizes
            let resized = img.resize(size, size, image::imageops::FilterType::Lanczos3);
            let rgba = resized.to_rgba8();
            
            // BMP Header (40 bytes)
            data.extend_from_slice(&40u32.to_le_bytes());
            data.extend_from_slice(&(size as i32).to_le_bytes());
            data.extend_from_slice(&(size as i32 * 2).to_le_bytes()); // Height * 2
            data.extend_from_slice(&[1, 0]); // Planes
            data.extend_from_slice(&[32, 0]); // BPP
            data.extend_from_slice(&[0, 0, 0, 0]); // Compression
            data.extend_from_slice(&[0, 0, 0, 0]); // ImageSize
            data.extend_from_slice(&[0, 0, 0, 0]); // Xppm
            data.extend_from_slice(&[0, 0, 0, 0]); // Yppm
            data.extend_from_slice(&[0, 0, 0, 0]); // ColorsUsed
            data.extend_from_slice(&[0, 0, 0, 0]); // ColorsImportant
            
            // Pixel Data (BGRA, bottom-up)
            for row in (0..size).rev() {
                for col in 0..size {
                    let pixel = rgba.get_pixel(col, row);
                    data.push(pixel[2]); // B
                    data.push(pixel[1]); // G
                    data.push(pixel[0]); // R
                    data.push(pixel[3]); // A
                }
            }
            
            // AND Mask (1 bit per pixel, padded to 32 bits)
            // All zeros (transparent) since we use alpha channel
            let row_bytes = ((size + 31) / 32) * 4;
            for _ in 0..size {
                for _ in 0..row_bytes {
                    data.push(0);
                }
            }
        }
        images_data.push(data);
    }
    
    // Write Directory Entries
    for (i, size) in sizes.iter().enumerate() {
        let width = if *size == 256 { 0 } else { *size as u8 };
        let height = if *size == 256 { 0 } else { *size as u8 };
        let data_size = images_data[i].len() as u32;
        
        file.write_all(&[width]).unwrap();
        file.write_all(&[height]).unwrap();
        file.write_all(&[0]).unwrap(); // Colors
        file.write_all(&[0]).unwrap(); // Reserved
        file.write_all(&[1, 0]).unwrap(); // Planes
        file.write_all(&[32, 0]).unwrap(); // BPP
        file.write_all(&data_size.to_le_bytes()).unwrap();
        file.write_all(&offset.to_le_bytes()).unwrap();
        
        offset += data_size;
    }
    
    // Write Image Data
    for data in images_data {
        file.write_all(&data).unwrap();
    }
}
