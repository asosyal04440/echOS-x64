//! # echOS HTTP Client
//!
//! HTTP/1.1 client with GET, POST, and download support

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;
use alloc::borrow::ToOwned;
use spin::Mutex;

use super::{NetError, Ipv4Addr, Port};
use super::socket::{SocketAddr, SocketType, AddressFamily, Protocol};
use super::socket::{socket as socket_create, connect, send, recv, close};

// ============================================================================
// HTTP CONSTANTS
// ============================================================================

/// HTTP port
const HTTP_PORT: u16 = 80;
/// HTTPS port
const HTTPS_PORT: u16 = 443;
/// Maximum response header size
const MAX_HEADER_SIZE: usize = 8192;
/// Maximum redirect count
const MAX_REDIRECTS: u8 = 5;
/// Receive buffer size
const RECV_BUF_SIZE: usize = 4096;
/// Default timeout (ms)
const DEFAULT_TIMEOUT_MS: u64 = 30000;

// ============================================================================
// HTTP ERROR
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpError {
    Network(NetError),
    InvalidUrl,
    InvalidResponse,
    ConnectionFailed,
    Timeout,
    TooManyRedirects,
    NotFound,
    ServerError,
    InvalidHeader,
    ChunkedEncoding,
    ContentLength,
    TlsNotSupported,
}

impl From<NetError> for HttpError {
    fn from(err: NetError) -> Self {
        HttpError::Network(err)
    }
}

// ============================================================================
// HTTP METHOD
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    HEAD,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::GET => "GET",
            HttpMethod::POST => "POST",
            HttpMethod::PUT => "PUT",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::HEAD => "HEAD",
        }
    }
}

// ============================================================================
// HTTP HEADERS
// ============================================================================

#[derive(Clone, Debug)]
pub struct HttpHeaders {
    headers: BTreeMap<String, String>,
}

impl HttpHeaders {
    pub fn new() -> Self {
        let mut headers = HttpHeaders {
            headers: BTreeMap::new(),
        };
        
        // Default headers
        headers.insert("User-Agent", "echOS/1.0");
        headers.insert("Accept", "*/*");
        headers.insert("Connection", "close");
        
        headers
    }
    
    pub fn insert(&mut self, key: &str, value: &str) {
        self.headers.insert(key.to_string().to_lowercase(), value.to_string());
    }
    
    pub fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(&key.to_lowercase()).map(|s| s.as_str())
    }
    
    pub fn remove(&mut self, key: &str) {
        self.headers.remove(&key.to_lowercase());
    }
    
    pub fn to_string(&self) -> String {
        let mut result = String::new();
        for (key, value) in &self.headers {
            result.push_str(key);
            result.push_str(": ");
            result.push_str(value);
            result.push_str("\r\n");
        }
        result
    }
}

impl Default for HttpHeaders {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// HTTP URL
// ============================================================================

#[derive(Clone, Debug)]
pub struct HttpUrl {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

impl HttpUrl {
    /// Parse URL from string
    pub fn parse(url: &str) -> Result<Self, HttpError> {
        // Simple URL parser
        // Format: scheme://host[:port][/path][?query][#fragment]
        
        let mut scheme = String::new();
        let mut host = String::new();
        let mut port = 0u16;
        let mut path = String::from("/");
        let mut query = String::new();
        let mut fragment = String::new();
        
        // Parse scheme
        let rest = if let Some(idx) = url.find("://") {
            scheme = url[..idx].to_string();
            &url[idx + 3..]
        } else {
            // Default to http
            scheme = String::from("http");
            url
        };
        
        // Determine default port
        port = if scheme == "https" { HTTPS_PORT } else { HTTP_PORT };
        
        // Parse host and port
        let path_start = rest.find('/').unwrap_or(rest.len());
        let host_port = &rest[..path_start];
        
        if let Some(idx) = host_port.find(':') {
            host = host_port[..idx].to_string();
            if let Ok(p) = host_port[idx + 1..].parse::<u16>() {
                port = p;
            }
        } else {
            host = host_port.to_string();
        }
        
        // Parse path, query, fragment
        if path_start < rest.len() {
            let path_rest = &rest[path_start..];
            
            // Fragment
            let path_query = if let Some(idx) = path_rest.find('#') {
                fragment = path_rest[idx + 1..].to_string();
                &path_rest[..idx]
            } else {
                path_rest
            };
            
            // Query
            let path_only = if let Some(idx) = path_query.find('?') {
                query = path_query[idx + 1..].to_string();
                &path_query[..idx]
            } else {
                path_query
            };
            
            path = path_only.to_string();
        }
        
        if host.is_empty() {
            return Err(HttpError::InvalidUrl);
        }
        
        Ok(HttpUrl {
            scheme,
            host,
            port,
            path,
            query,
            fragment,
        })
    }
    
    /// Get full URL as string
    pub fn to_url_string(&self) -> String {
        let mut result = String::new();
        result.push_str(&self.scheme);
        result.push_str("://");
        result.push_str(&self.host);
        
        if (self.scheme == "http" && self.port != HTTP_PORT) ||
           (self.scheme == "https" && self.port != HTTPS_PORT) {
            result.push(':');
            result.push_str(&self.port.to_string());
        }
        
        result.push_str(&self.path);
        
        if !self.query.is_empty() {
            result.push('?');
            result.push_str(&self.query);
        }
        
        if !self.fragment.is_empty() {
            result.push('#');
            result.push_str(&self.fragment);
        }
        
        result
    }
    
    /// Check if HTTPS
    pub fn is_https(&self) -> bool {
        self.scheme == "https"
    }
}

// ============================================================================
// HTTP RESPONSE
// ============================================================================

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status_code: u16,
    pub status_text: String,
    pub headers: HttpHeaders,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn new() -> Self {
        HttpResponse {
            status_code: 0,
            status_text: String::new(),
            headers: HttpHeaders::new(),
            body: Vec::new(),
        }
    }
    
    /// Check if response is successful (2xx)
    pub fn is_success(&self) -> bool {
        self.status_code >= 200 && self.status_code < 300
    }
    
    /// Check if redirect (3xx)
    pub fn is_redirect(&self) -> bool {
        self.status_code >= 300 && self.status_code < 400
    }
    
    /// Check if client error (4xx)
    pub fn is_client_error(&self) -> bool {
        self.status_code >= 400 && self.status_code < 500
    }
    
    /// Check if server error (5xx)
    pub fn is_server_error(&self) -> bool {
        self.status_code >= 500 && self.status_code < 600
    }
    
    /// Get body as string
    pub fn body_as_string(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
    
    /// Get content length
    pub fn content_length(&self) -> Option<usize> {
        self.headers.get("content-length")
            .and_then(|s| s.parse::<usize>().ok())
    }
    
    /// Check if chunked transfer
    pub fn is_chunked(&self) -> bool {
        self.headers.get("transfer-encoding")
            .map(|s| s.to_lowercase() == "chunked")
            .unwrap_or(false)
    }
    
    /// Get redirect location
    pub fn location(&self) -> Option<&str> {
        self.headers.get("location")
    }
}

impl Default for HttpResponse {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// HTTP CLIENT
// ============================================================================

pub struct HttpClient {
    timeout_ms: u64,
    max_redirects: u8,
    follow_redirects: bool,
}

impl HttpClient {
    pub fn new() -> Self {
        HttpClient {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_redirects: MAX_REDIRECTS,
            follow_redirects: true,
        }
    }
    
    /// Set timeout in milliseconds
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }
    
    /// Set whether to follow redirects
    pub fn set_follow_redirects(&mut self, follow: bool) {
        self.follow_redirects = follow;
    }
    
    /// Perform HTTP GET request
    pub fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
        self.request(HttpMethod::GET, url, None, None)
    }
    
    /// Perform HTTP POST request
    pub fn post(&self, url: &str, body: &[u8], content_type: Option<&str>) -> Result<HttpResponse, HttpError> {
        self.request(HttpMethod::POST, url, Some(body), content_type)
    }
    
    /// Perform HTTP request
    pub fn request(
        &self,
        method: HttpMethod,
        url: &str,
        body: Option<&[u8]>,
        content_type: Option<&str>,
    ) -> Result<HttpResponse, HttpError> {
        let mut current_url = HttpUrl::parse(url)?;
        let mut redirect_count = 0;
        
        loop {
            // Check for HTTPS (not supported yet)
            if current_url.is_https() {
                return Err(HttpError::TlsNotSupported);
            }
            
            // Resolve hostname using DNS
            let dns_server = super::get_config().dns_servers.first()
                .copied()
                .unwrap_or([8, 8, 8, 8]);
            let dns_ip = Ipv4Addr::from_bytes(dns_server);
            let ip = super::dns::resolve(&current_url.host, dns_ip)
                .map_err(|_| HttpError::ConnectionFailed)?;
            
            // Create TCP socket
            let sock_id = socket_create(
                AddressFamily::IPV4,
                SocketType::STREAM,
                Protocol::TCP,
            )?;
            
            // Connect to server
            let addr = SocketAddr::new(ip, Port(current_url.port));
            connect(sock_id, addr)?;
            
            // Build request
            let request = self.build_request(method, &current_url, body, content_type);
            
            // Send request
            send(sock_id, request.as_bytes(), 0)?;
            
            // Receive response
            let response = self.receive_response(sock_id)?;
            
            // Close socket
            let _ = close(sock_id);
            
            // Handle redirect
            if response.is_redirect() && self.follow_redirects {
                redirect_count += 1;
                if redirect_count > self.max_redirects {
                    return Err(HttpError::TooManyRedirects);
                }
                
                if let Some(location) = response.location() {
                    // Handle relative URLs
                    if location.starts_with('/') {
                        current_url.path = location.to_string();
                    } else if location.starts_with("http://") || location.starts_with("https://") {
                        current_url = HttpUrl::parse(location)?;
                    } else {
                        // Relative URL
                        current_url.path = location.to_string();
                    }
                    continue;
                }
            }
            
            return Ok(response);
        }
    }
    
    /// Download file to path
    pub fn download(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        let response = self.get(url)?;
        
        if !response.is_success() {
            if response.status_code == 404 {
                return Err(HttpError::NotFound);
            }
            return Err(HttpError::ServerError);
        }
        
        Ok(response.body)
    }
    
    /// Build HTTP request string
    fn build_request(
        &self,
        method: HttpMethod,
        url: &HttpUrl,
        body: Option<&[u8]>,
        content_type: Option<&str>,
    ) -> String {
        let mut request = String::new();
        
        // Request line
        let mut path_query = url.path.clone();
        if !url.query.is_empty() {
            path_query.push('?');
            path_query.push_str(&url.query);
        }
        
        request.push_str(method.as_str());
        request.push(' ');
        request.push_str(&path_query);
        request.push_str(" HTTP/1.1\r\n");
        
        // Host header
        request.push_str("Host: ");
        request.push_str(&url.host);
        if url.port != HTTP_PORT && url.port != HTTPS_PORT {
            request.push(':');
            request.push_str(&url.port.to_string());
        }
        request.push_str("\r\n");
        
        // Content headers for POST
        if let Some(data) = body {
            request.push_str("Content-Length: ");
            request.push_str(&data.len().to_string());
            request.push_str("\r\n");
            
            if let Some(ct) = content_type {
                request.push_str("Content-Type: ");
                request.push_str(ct);
                request.push_str("\r\n");
            }
        }
        
        // Headers
        request.push_str("User-Agent: echOS/1.0\r\n");
        request.push_str("Accept: */*\r\n");
        request.push_str("Connection: close\r\n");
        
        request.push_str("\r\n");
        
        // Body
        if let Some(data) = body {
            // Convert body to string (for text content)
            // In real implementation, we'd write bytes directly
            let body_str = core::str::from_utf8(data).unwrap_or("");
            request.push_str(body_str);
        }
        
        request
    }
    
    /// Receive and parse HTTP response
    fn receive_response(&self, sock_id: u32) -> Result<HttpResponse, HttpError> {
        let mut response = HttpResponse::new();
        let mut header_buf = vec![0u8; MAX_HEADER_SIZE];
        let mut header_len = 0;
        
        // Receive headers
        loop {
            let mut chunk = vec![0u8; RECV_BUF_SIZE];
            let n = recv(sock_id, &mut chunk, 0)?;
            
            if n == 0 {
                break;
            }
            
            // Copy to header buffer
            let copy_len = core::cmp::min(n, MAX_HEADER_SIZE - header_len);
            header_buf[header_len..header_len + copy_len].copy_from_slice(&chunk[..copy_len]);
            header_len += copy_len;
            
            // Check for header end
            let header_end = find_header_end(&header_buf[..header_len]);
            if header_end.is_some() {
                break;
            }
        }
        
        // Parse headers
        let header_end = find_header_end(&header_buf[..header_len])
            .ok_or(HttpError::InvalidResponse)?;
        
        let header_str = core::str::from_utf8(&header_buf[..header_end])
            .map_err(|_| HttpError::InvalidHeader)?;
        
        self.parse_response_headers(header_str, &mut response)?;
        
        // Receive body
        let body_start = header_end + 4; // Skip "\r\n\r\n"
        
        if response.is_chunked() {
            // Chunked transfer encoding
            self.receive_chunked_body(sock_id, &mut response)?;
        } else if let Some(content_len) = response.content_length() {
            // Content-Length known
            // Copy any body data already in header buffer
            let initial_body_len = header_len - body_start;
            if initial_body_len > 0 {
                response.body.extend_from_slice(&header_buf[body_start..header_len]);
            }
            
            // Receive remaining body
            while response.body.len() < content_len {
                let mut chunk = vec![0u8; RECV_BUF_SIZE];
                let n = recv(sock_id, &mut chunk, 0)?;
                if n == 0 {
                    break;
                }
                response.body.extend_from_slice(&chunk[..n]);
            }
        } else {
            // No content-length, read until connection close
            response.body.extend_from_slice(&header_buf[body_start..header_len]);
            
            loop {
                let mut chunk = vec![0u8; RECV_BUF_SIZE];
                let n = recv(sock_id, &mut chunk, 0)?;
                if n == 0 {
                    break;
                }
                response.body.extend_from_slice(&chunk[..n]);
            }
        }
        
        Ok(response)
    }
    
    /// Parse response headers
    fn parse_response_headers(&self, header_str: &str, response: &mut HttpResponse) -> Result<(), HttpError> {
        let mut lines = header_str.lines();
        
        // Status line
        let status_line = lines.next().ok_or(HttpError::InvalidResponse)?;
        
        // Parse "HTTP/1.1 200 OK"
        let parts: Vec<&str> = status_line.splitn(3, ' ').collect();
        if parts.len() < 2 {
            return Err(HttpError::InvalidResponse);
        }
        
        response.status_code = parts[1].parse()
            .map_err(|_| HttpError::InvalidResponse)?;
        response.status_text = parts.get(2).unwrap_or(&"").to_string();
        
        // Headers
        for line in lines {
            if line.is_empty() {
                continue;
            }
            
            if let Some(idx) = line.find(':') {
                let key = line[..idx].trim();
                let value = line[idx + 1..].trim();
                response.headers.insert(key, value);
            }
        }
        
        Ok(())
    }
    
    /// Receive chunked body
    fn receive_chunked_body(&self, sock_id: u32, response: &mut HttpResponse) -> Result<(), HttpError> {
        loop {
            // Read chunk size
            let mut size_buf = String::new();
            loop {
                let mut byte = [0u8; 1];
                let n = recv(sock_id, &mut byte, 0)?;
                if n == 0 {
                    return Err(HttpError::ChunkedEncoding);
                }
                
                if byte[0] == b'\n' {
                    break;
                }
                
                if byte[0] != b'\r' {
                    size_buf.push(byte[0] as char);
                }
            }
            
            // Parse chunk size (hex)
            let chunk_size = usize::from_str_radix(size_buf.trim(), 16)
                .map_err(|_| HttpError::ChunkedEncoding)?;
            
            if chunk_size == 0 {
                // Final chunk
                break;
            }
            
            // Read chunk data
            let mut remaining = chunk_size;
            while remaining > 0 {
                let mut chunk = vec![0u8; remaining];
                let n = recv(sock_id, &mut chunk, 0)?;
                if n == 0 {
                    return Err(HttpError::ChunkedEncoding);
                }
                response.body.extend_from_slice(&chunk[..n]);
                remaining -= n;
            }
            
            // Read trailing \r\n
            let mut trailer = [0u8; 2];
            recv(sock_id, &mut trailer, 0)?;
        }
        
        Ok(())
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Find end of HTTP headers (double CRLF)
fn find_header_end(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if data[i] == b'\r' && data[i + 1] == b'\n' && data[i + 2] == b'\r' && data[i + 3] == b'\n' {
            return Some(i);
        }
    }
    None
}

// ============================================================================
// CONVENIENCE FUNCTIONS
// ============================================================================

/// Perform HTTP GET request
pub fn http_get(url: &str) -> Result<HttpResponse, HttpError> {
    HttpClient::new().get(url)
}

/// Perform HTTP POST request
pub fn http_post(url: &str, body: &[u8]) -> Result<HttpResponse, HttpError> {
    HttpClient::new().post(url, body, Some("application/octet-stream"))
}

/// Download file from URL
pub fn http_download(url: &str) -> Result<Vec<u8>, HttpError> {
    HttpClient::new().download(url)
}
