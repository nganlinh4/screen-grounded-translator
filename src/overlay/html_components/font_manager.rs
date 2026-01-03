//! Font Manager - Bundles Google Sans Flex variable font and serves it via local HTTP
//!
//! Spins up a tiny ephemeral HTTP server to serve the bundled font.
//! This bypasses WebView2 file:// restrictions and base64 size limits.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Mutex, Once};
use windows::Win32::Graphics::Gdi::AddFontMemResourceEx;
use wry::WebViewBuilder;

/// Google Sans Flex variable font - bundled at compile time (~5MB)
static GOOGLE_SANS_FLEX_TTF: &[u8] =
    include_bytes!("../../../assets/GoogleSansFlex-VariableFont_GRAD,ROND,opsz,slnt,wdth,wght.ttf");

static INIT_FONTS: Once = Once::new();
lazy_static::lazy_static! {
    static ref FONT_SERVER_URL: Mutex<Option<String>> = Mutex::new(None);
}

pub fn warmup_fonts() {
    start_font_server();
    load_gdi_font();
}

fn load_gdi_font() {
    unsafe {
        let mut num_fonts = 0;
        let len = GOOGLE_SANS_FLEX_TTF.len() as u32;
        // AddFontMemResourceEx installs the fonts from the memory image
        let handle = AddFontMemResourceEx(
            GOOGLE_SANS_FLEX_TTF.as_ptr() as *mut _,
            len,
            None,
            &mut num_fonts,
        );

        if handle.is_invalid() {
            eprintln!("Failed to load Google Sans Flex into GDI");
        }
    }
}

/// Helper to configure WebViewBuilder (legacy pass-through)
pub fn configure_webview(builder: WebViewBuilder) -> WebViewBuilder {
    builder
}

fn start_font_server() {
    INIT_FONTS.call_once(|| {
        std::thread::spawn(|| {
            // Bind to ephemeral port
            let listener = match TcpListener::bind("127.0.0.1:0") {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Failed to bind font server: {}", e);
                    return;
                }
            };

            let addr = listener.local_addr().unwrap();
            let url = format!("http://{}:{}/GoogleSansFlex.ttf", addr.ip(), addr.port());

            {
                let mut url_guard = FONT_SERVER_URL.lock().unwrap();
                *url_guard = Some(url);
            }

            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let _ = std::thread::spawn(move || {
                            let mut buffer = [0; 8192];
                            let mut bytes_read = 0;

                            // Read request headers to avoid RST on close
                            loop {
                                match stream.read(&mut buffer[bytes_read..]) {
                                    Ok(n) if n > 0 => {
                                        bytes_read += n;
                                        // Stop if we see end of headers or buffer full
                                        if bytes_read == buffer.len()
                                            || buffer[..bytes_read]
                                                .windows(4)
                                                .any(|w| w == b"\r\n\r\n")
                                        {
                                            break;
                                        }
                                    }
                                    _ => break,
                                }
                            }

                            let request_str = String::from_utf8_lossy(&buffer[..bytes_read]);
                            let is_head = request_str.starts_with("HEAD");

                            // Serve font
                            let response_header = format!(
                                "HTTP/1.1 200 OK\r\n\
                                Content-Type: font/ttf\r\n\
                                Access-Control-Allow-Origin: *\r\n\
                                Content-Length: {}\r\n\
                                Connection: close\r\n\r\n",
                                GOOGLE_SANS_FLEX_TTF.len()
                            );

                            if stream.write_all(response_header.as_bytes()).is_err() {
                                return;
                            }

                            if !is_head {
                                if let Err(e) = stream.write_all(GOOGLE_SANS_FLEX_TTF) {
                                    // 10053 is "An established connection was aborted by the software in your host machine"
                                    // This often happens if the client closes the connection early (e.g. satisfied cache, page reload)
                                    // We filter it out to avoid log spam, as it's usually benign from the server's POV
                                    if e.raw_os_error() != Some(10053) {
                                        eprintln!("Font server body error: {}", e);
                                    }
                                    return;
                                }
                            }
                            let _ = stream.flush();
                        });
                    }
                    Err(e) => eprintln!("Font server request error: {}", e),
                }
            }
        });

        // Give the thread a moment to bind and set the URL
        std::thread::sleep(std::time::Duration::from_millis(50));
    });
}

pub fn get_font_css() -> String {
    // Ensure server is started
    start_font_server();

    // Get URL with retry logic
    let mut font_url = String::new();
    for _ in 0..10 {
        if let Ok(guard) = FONT_SERVER_URL.lock() {
            if let Some(url) = guard.as_ref() {
                font_url = url.clone();
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    if font_url.is_empty() {
        eprintln!("ERROR: Could not get font server URL");
    }

    format!(
        r#"
        @font-face {{
            font-family: 'Google Sans Flex';
            font-style: normal;
            font-weight: 100 1000;
            font-stretch: 25% 151%;
            font-display: block;
            src: url('{}') format('truetype');
        }}
    "#,
        font_url
    )
}
