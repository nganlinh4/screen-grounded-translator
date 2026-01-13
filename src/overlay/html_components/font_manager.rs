//! Font Manager - Bundles Google Sans Flex variable font
//!
//! Serves both HTML pages and fonts from a local HTTP server.
//! This ensures same-origin access, bypassing CORS/PNA restrictions.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};
use windows::Win32::Graphics::Gdi::AddFontMemResourceEx;
use wry::WebViewBuilder;

/// Google Sans Flex variable font - bundled at compile time (~5MB)
static GOOGLE_SANS_FLEX_TTF: &[u8] = crate::assets::GOOGLE_SANS_FLEX;

static START_SERVER_ONCE: Once = Once::new();
static PAGE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

lazy_static::lazy_static! {
    /// Server URL once started
    static ref SERVER_URL: Mutex<Option<String>> = Mutex::new(None);

    /// Pending HTML pages waiting to be served (page_id -> html)
    static ref PENDING_PAGES: Mutex<HashMap<u64, String>> = Mutex::new(HashMap::new());
}

/// Warmup: Start server and load font into GDI.
pub fn warmup_fonts() {
    load_gdi_font();
    start_server();
}

fn load_gdi_font() {
    unsafe {
        let mut num_fonts = 0;
        let len = GOOGLE_SANS_FLEX_TTF.len() as u32;
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

fn start_server() {
    START_SERVER_ONCE.call_once(|| {
        std::thread::spawn(|| {
            let listener = match TcpListener::bind("127.0.0.1:0") {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Failed to bind font server: {}", e);
                    return;
                }
            };

            let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            let url = format!("http://127.0.0.1:{}", port);

            if let Ok(mut guard) = SERVER_URL.lock() {
                *guard = Some(url);
            }

            for stream in listener.incoming() {
                if let Ok(mut stream) = stream {
                    let _ = handle_request(&mut stream);
                }
            }
        });
    });
}

fn handle_request(stream: &mut std::net::TcpStream) -> std::io::Result<()> {
    let mut buffer = [0u8; 4096];
    let n = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..n]);

    // Parse the request line
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    let method = parts.get(0).copied().unwrap_or("GET");
    let path = parts.get(1).copied().unwrap_or("/");

    // CORS headers for all responses
    let cors_headers = "Access-Control-Allow-Origin: *\r\n\
                        Access-Control-Allow-Methods: GET, HEAD, OPTIONS\r\n\
                        Access-Control-Allow-Headers: *\r\n\
                        Access-Control-Allow-Private-Network: true\r\n";

    // Handle OPTIONS preflight
    if method == "OPTIONS" {
        let response =
            format!("HTTP/1.1 204 No Content\r\n{cors_headers}Connection: close\r\n\r\n");
        stream.write_all(response.as_bytes())?;
        return Ok(());
    }

    // Route requests
    if path == "/font/GoogleSansFlex.ttf" {
        // Serve font
        let headers = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: font/ttf\r\n\
             Content-Length: {}\r\n\
             {cors_headers}\
             Cache-Control: max-age=31536000\r\n\
             Connection: close\r\n\r\n",
            GOOGLE_SANS_FLEX_TTF.len()
        );
        stream.write_all(headers.as_bytes())?;
        if method != "HEAD" {
            stream.write_all(GOOGLE_SANS_FLEX_TTF)?;
        }
    } else if path.starts_with("/page/") {
        // Serve stored HTML page
        let id_str = path.strip_prefix("/page/").unwrap_or("0");
        let page_id: u64 = id_str.parse().unwrap_or(0);

        let html = PENDING_PAGES
            .lock()
            .ok()
            .and_then(|mut map| map.remove(&page_id))
            .unwrap_or_else(|| "<html><body>Page not found</body></html>".to_string());

        let html_bytes = html.as_bytes();
        let headers = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\n\
             {cors_headers}\
             Connection: close\r\n\r\n",
            html_bytes.len()
        );
        stream.write_all(headers.as_bytes())?;
        if method != "HEAD" {
            stream.write_all(html_bytes)?;
        }
    } else {
        // 404
        let body = b"Not Found";
        let headers = format!(
            "HTTP/1.1 404 Not Found\r\n\
             Content-Type: text/plain\r\n\
             Content-Length: {}\r\n\
             {cors_headers}\
             Connection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes())?;
        stream.write_all(body)?;
    }

    Ok(())
}

/// Get the server base URL, waiting if necessary
fn get_server_url() -> Option<String> {
    // Ensure server is started
    start_server();

    // Wait for URL to be available (up to 2 seconds)
    for _ in 0..40 {
        if let Ok(guard) = SERVER_URL.lock() {
            if let Some(url) = guard.as_ref() {
                return Some(url.clone());
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    None
}

/// Store HTML content and get a page URL to load it
pub fn store_html_page(html: String) -> Option<String> {
    let base_url = get_server_url()?;
    let page_id = PAGE_ID_COUNTER.fetch_add(1, Ordering::SeqCst);

    if let Ok(mut map) = PENDING_PAGES.lock() {
        map.insert(page_id, html);
    }

    Some(format!("{}/page/{}", base_url, page_id))
}

/// Configure WebViewBuilder (no-op, URL loading handles everything)
pub fn configure_webview(builder: WebViewBuilder) -> WebViewBuilder {
    builder
}

/// Returns the CSS @font-face rule using the local server
pub fn get_font_css() -> String {
    let base_url = get_server_url().unwrap_or_else(|| "http://127.0.0.1:0".to_string());

    format!(
        r#"
        @font-face {{
            font-family: 'Google Sans Flex';
            font-style: normal;
            font-weight: 100 1000;
            font-stretch: 25% 1000%;
            font-display: swap;
            src: url('{}/font/GoogleSansFlex.ttf') format('truetype');
        }}
    "#,
        base_url
    )
}
