//! HTTP Server API style request queues and URL groups.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use super::{Ipv4Addr, NetError, Port};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpSysRequest {
    pub request_id: u64,
    pub method: String,
    pub raw_url: String,
    pub path: String,
    pub remote_addr: Ipv4Addr,
    pub remote_port: Port,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpSysResponse {
    pub status_code: u16,
    pub reason: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpSysResponse {
    pub fn ok(body: &[u8], content_type: &str) -> Self {
        let mut headers = BTreeMap::new();
        headers.insert(String::from("content-type"), String::from(content_type));
        headers.insert(String::from("content-length"), body.len().to_string());
        Self {
            status_code: 200,
            reason: String::from("OK"),
            headers,
            body: body.to_vec(),
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(
            alloc::format!("HTTP/1.1 {} {}\r\n", self.status_code, self.reason).as_bytes(),
        );
        for (k, v) in &self.headers {
            out.extend_from_slice(alloc::format!("{k}: {v}\r\n").as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&self.body);
        out
    }
}

#[derive(Clone, Debug)]
struct UrlGroup {
    session_id: u64,
    queue_id: Option<u64>,
    urls: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestQueueInfo {
    pub queue_id: u64,
    pub pending_requests: usize,
    pub bound_groups: usize,
}

static NEXT_SERVER_SESSION_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_URL_GROUP_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_REQUEST_QUEUE_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

static SERVER_SESSIONS: Mutex<BTreeMap<u64, String>> = Mutex::new(BTreeMap::new());
static URL_GROUPS: Mutex<BTreeMap<u64, UrlGroup>> = Mutex::new(BTreeMap::new());
static REQUEST_QUEUES: Mutex<BTreeMap<u64, VecDeque<HttpSysRequest>>> = Mutex::new(BTreeMap::new());
static RESPONSES: Mutex<BTreeMap<u64, HttpSysResponse>> = Mutex::new(BTreeMap::new());
static REQUEST_TO_QUEUE: Mutex<BTreeMap<u64, u64>> = Mutex::new(BTreeMap::new());

pub fn create_server_session(name: &str) -> u64 {
    let id = NEXT_SERVER_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    SERVER_SESSIONS.lock().insert(id, String::from(name));
    id
}

pub fn create_url_group(session_id: u64) -> Result<u64, NetError> {
    if !SERVER_SESSIONS.lock().contains_key(&session_id) {
        return Err(NetError::InvalidFd);
    }
    let id = NEXT_URL_GROUP_ID.fetch_add(1, Ordering::Relaxed);
    URL_GROUPS.lock().insert(
        id,
        UrlGroup {
            session_id,
            queue_id: None,
            urls: Vec::new(),
        },
    );
    Ok(id)
}

pub fn create_request_queue() -> u64 {
    let id = NEXT_REQUEST_QUEUE_ID.fetch_add(1, Ordering::Relaxed);
    REQUEST_QUEUES.lock().insert(id, VecDeque::new());
    id
}

pub fn bind_url_group_to_queue(group_id: u64, queue_id: u64) -> Result<(), NetError> {
    if !REQUEST_QUEUES.lock().contains_key(&queue_id) {
        return Err(NetError::InvalidFd);
    }
    let mut groups = URL_GROUPS.lock();
    let group = groups.get_mut(&group_id).ok_or(NetError::InvalidFd)?;
    group.queue_id = Some(queue_id);
    Ok(())
}

pub fn add_url_to_group(group_id: u64, url_prefix: &str) -> Result<(), NetError> {
    let mut groups = URL_GROUPS.lock();
    let group = groups.get_mut(&group_id).ok_or(NetError::InvalidFd)?;
    if !group.urls.iter().any(|u| u == url_prefix) {
        group.urls.push(String::from(url_prefix));
    }
    Ok(())
}

pub fn remove_url_from_group(group_id: u64, url_prefix: &str) -> Result<(), NetError> {
    let mut groups = URL_GROUPS.lock();
    let group = groups.get_mut(&group_id).ok_or(NetError::InvalidFd)?;
    let before = group.urls.len();
    group.urls.retain(|prefix| prefix != url_prefix);
    if before == group.urls.len() {
        return Err(NetError::AddrNotAvailable);
    }
    Ok(())
}

pub fn inject_request(
    method: &str,
    raw_url: &str,
    remote_addr: Ipv4Addr,
    remote_port: Port,
    headers: BTreeMap<String, String>,
    body: &[u8],
) -> Result<u64, NetError> {
    let groups = URL_GROUPS.lock();
    let mut target_queue = None;
    let mut best_match = 0usize;
    for group in groups.values() {
        for prefix in &group.urls {
            if raw_url.starts_with(prefix) && prefix.len() >= best_match {
                best_match = prefix.len();
                target_queue = group.queue_id;
            }
        }
    }
    let queue_id = target_queue.ok_or(NetError::AddrNotAvailable)?;
    drop(groups);

    let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let path = raw_url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.find('/').map(|idx| String::from(&rest[idx..])))
        .unwrap_or_else(|| String::from("/"));
    let request = HttpSysRequest {
        request_id,
        method: String::from(method),
        raw_url: String::from(raw_url),
        path,
        remote_addr,
        remote_port,
        headers,
        body: body.to_vec(),
    };
    REQUEST_QUEUES
        .lock()
        .get_mut(&queue_id)
        .ok_or(NetError::InvalidFd)?
        .push_back(request);
    REQUEST_TO_QUEUE.lock().insert(request_id, queue_id);
    Ok(request_id)
}

pub fn receive_request(queue_id: u64) -> Result<HttpSysRequest, NetError> {
    REQUEST_QUEUES
        .lock()
        .get_mut(&queue_id)
        .ok_or(NetError::InvalidFd)?
        .pop_front()
        .ok_or(NetError::WouldBlock)
}

pub fn send_response(request_id: u64, response: HttpSysResponse) -> Result<(), NetError> {
    if !REQUEST_TO_QUEUE.lock().contains_key(&request_id) {
        return Err(NetError::InvalidFd);
    }
    RESPONSES.lock().insert(request_id, response);
    Ok(())
}

pub fn get_response(request_id: u64) -> Option<HttpSysResponse> {
    RESPONSES.lock().get(&request_id).cloned()
}

pub fn take_response(request_id: u64) -> Option<HttpSysResponse> {
    REQUEST_TO_QUEUE.lock().remove(&request_id);
    RESPONSES.lock().remove(&request_id)
}

pub fn query_request_queue(queue_id: u64) -> Result<RequestQueueInfo, NetError> {
    let queues = REQUEST_QUEUES.lock();
    let pending_requests = queues.get(&queue_id).ok_or(NetError::InvalidFd)?.len();
    let bound_groups = URL_GROUPS
        .lock()
        .values()
        .filter(|group| group.queue_id == Some(queue_id))
        .count();
    Ok(RequestQueueInfo {
        queue_id,
        pending_requests,
        bound_groups,
    })
}

pub fn close_request_queue(queue_id: u64) {
    REQUEST_QUEUES.lock().remove(&queue_id);
    URL_GROUPS.lock().values_mut().for_each(|group| {
        if group.queue_id == Some(queue_id) {
            group.queue_id = None;
        }
    });
    let request_ids: Vec<u64> = REQUEST_TO_QUEUE
        .lock()
        .iter()
        .filter(|(_, mapped_queue)| **mapped_queue == queue_id)
        .map(|(request_id, _)| *request_id)
        .collect();
    for request_id in request_ids {
        REQUEST_TO_QUEUE.lock().remove(&request_id);
        RESPONSES.lock().remove(&request_id);
    }
}

pub fn close_server_session(session_id: u64) {
    SERVER_SESSIONS.lock().remove(&session_id);
    let queue_ids: Vec<u64> = URL_GROUPS
        .lock()
        .values()
        .filter(|group| group.session_id == session_id)
        .filter_map(|group| group.queue_id)
        .collect();
    URL_GROUPS.lock().retain(|_, group| group.session_id != session_id);
    for queue_id in queue_ids {
        close_request_queue(queue_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_queue_roundtrip() {
        let session = create_server_session("svc");
        let group = create_url_group(session).unwrap();
        let queue = create_request_queue();
        bind_url_group_to_queue(group, queue).unwrap();
        add_url_to_group(group, "http://example.com/api").unwrap();

        let request_id = inject_request(
            "GET",
            "http://example.com/api/health",
            Ipv4Addr([127, 0, 0, 1]),
            Port(8080),
            BTreeMap::new(),
            b"",
        )
        .unwrap();
        let req = receive_request(queue).unwrap();
        assert_eq!(req.request_id, request_id);
        assert_eq!(req.path, "/api/health");
        assert_eq!(query_request_queue(queue).unwrap().pending_requests, 0);

        send_response(request_id, HttpSysResponse::ok(b"ok", "text/plain")).unwrap();
        assert_eq!(get_response(request_id).unwrap().status_code, 200);
        assert_eq!(take_response(request_id).unwrap().body, b"ok");
        remove_url_from_group(group, "http://example.com/api").unwrap();
        close_request_queue(queue);
    }
}
