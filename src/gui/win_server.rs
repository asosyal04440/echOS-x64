//! # echOS Kernel Window Server  (Faz 5)
//!
//! Bare-metal kernel-side implementasyon.
//! User-space ELF süreçleri bu sunucu üzerinden pencere oluşturur,
//! piksel yazar ve giriş olaylarını okur.
//!
//! ## Syscall ABI
//! | Numara | İsim               | Açıklama                                        |
//! |--------|--------------------|-------------------------------------------------|
//! | 451    | SYS_WIN_CREATE     | Yeni pencere oluştur, id döndürür               |
//! | 452    | SYS_WIN_DESTROY    | Pencereyi kapat                                 |
//! | 453    | SYS_WIN_GET_BUFFER | ARGB piksel arabelleğinin çekirdek adresi       |
//! | 454    | SYS_WIN_FLUSH      | Arabelleği dirty olarak işaretle (compositor)   |
//! | 455    | SYS_EVENT_POLL     | Bekleyen giriş olayını çek                      |

use alloc::{string::String, vec, vec::Vec};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Olay tipleri
// ---------------------------------------------------------------------------

/// Pencerelere iletilen giriş + yaşam döngüsü olayları.
/// `repr(C)` ile user-space'e kopyalanabilir.
#[derive(Clone, Copy, Debug)]
#[repr(u32)]
#[allow(dead_code)]
pub enum WinEventKind {
    None       = 0,
    KeyDown    = 1,
    KeyUp      = 2,
    MouseMove  = 3,
    MouseDown  = 4,
    MouseUp    = 5,
    Close      = 6,
    Resize     = 7,
    Focus      = 8,
    FocusLost  = 9,
}

/// 48-bayt sabit boyutlu olay kaydı (C uyumlu).
#[derive(Clone, Copy)]
#[repr(C)]
pub struct WinEvent {
    /// `WinEventKind` değeri
    pub kind  : u32,
    /// Hangi pencere bu olayı aldı
    pub win_id: u32,
    /// Fare koordinatı ya da yeniden boyutlandırma genişliği
    pub x     : i32,
    /// Fare koordinatı ya da yeniden boyutlandırma yüksekliği
    pub y     : i32,
    /// Tuş tarama kodu (klavye olaylarında)
    pub key   : u32,
    /// Modifier bitler (Shift=1, Ctrl=2, Alt=4, Meta=8)
    pub mods  : u32,
    pub _pad  : [u32; 4],
}

impl WinEvent {
    /// Boş / geçersiz olay
    pub const fn none() -> Self {
        WinEvent { kind: 0, win_id: 0, x: 0, y: 0, key: 0, mods: 0, _pad: [0; 4] }
    }

    /// Kısayol kurucu: klavye olayı
    pub fn key(win_id: u32, down: bool, scancode: u32, mods: u32) -> Self {
        WinEvent {
            kind: if down { WinEventKind::KeyDown as u32 } else { WinEventKind::KeyUp as u32 },
            win_id,
            x: 0, y: 0,
            key: scancode,
            mods,
            _pad: [0; 4],
        }
    }

    /// Kısayol kurucu: fare hareketi
    pub fn mouse_move(win_id: u32, x: i32, y: i32) -> Self {
        WinEvent {
            kind: WinEventKind::MouseMove as u32,
            win_id, x, y,
            key: 0, mods: 0, _pad: [0; 4],
        }
    }

    /// Kısayol kurucu: fare tuşu
    pub fn mouse_btn(win_id: u32, down: bool, x: i32, y: i32, btn: u32) -> Self {
        WinEvent {
            kind: if down { WinEventKind::MouseDown as u32 } else { WinEventKind::MouseUp as u32 },
            win_id, x, y,
            key: btn, mods: 0, _pad: [0; 4],
        }
    }

    /// Kısayol kurucu: kapat
    pub fn close(win_id: u32) -> Self {
        WinEvent { kind: WinEventKind::Close as u32, win_id, x: 0, y: 0, key: 0, mods: 0, _pad: [0; 4] }
    }
}

// ---------------------------------------------------------------------------
// Pencere tanıtıcısı
// ---------------------------------------------------------------------------

/// Olay kuyruğu kapasitesi (halka tamponu).
const EVENT_QUEUE_CAP: usize = 64;

pub struct WindowHandle {
    pub id    : u32,
    /// Sahibi olan görevin kimliği (`current_task_id()`)
    pub tid   : usize,
    pub x     : i32,
    pub y     : i32,
    pub width : u32,
    pub height: u32,
    pub title : String,
    /// Kernel heap'te saklanan ARGB piksel arabelleği
    pub surface: Vec<u32>,
    /// Compositor'ın birleştirmesi gerekiyor mu?
    pub dirty  : bool,
    // Olay kuyruğu (halka tamponu)
    events : [WinEvent; EVENT_QUEUE_CAP],
    ev_head: usize,
    ev_tail: usize,
}

impl WindowHandle {
    fn new(id: u32, tid: usize, x: i32, y: i32, w: u32, h: u32, title: String) -> Self {
        let pixels = (w * h) as usize;
        WindowHandle {
            id, tid, x, y, width: w, height: h, title,
            surface: vec![0u32; pixels],
            dirty: false,
            events: [WinEvent::none(); EVENT_QUEUE_CAP],
            ev_head: 0,
            ev_tail: 0,
        }
    }

    /// Kuyruğa olay ekle; doluysa en eski olay üzerine yazılır.
    pub fn push_event(&mut self, ev: WinEvent) {
        let next = (self.ev_tail + 1) % EVENT_QUEUE_CAP;
        if next == self.ev_head {
            // Doluysa en eskiyi at
            self.ev_head = (self.ev_head + 1) % EVENT_QUEUE_CAP;
        }
        self.events[self.ev_tail] = ev;
        self.ev_tail = next;
    }

    /// Kuyruktan olay al (FIFO).
    pub fn pop_event(&mut self) -> Option<WinEvent> {
        if self.ev_head == self.ev_tail { return None; }
        let ev = self.events[self.ev_head];
        self.ev_head = (self.ev_head + 1) % EVENT_QUEUE_CAP;
        Some(ev)
    }

    /// Ham piksel arabelleği pointer'ı (salt okunur).
    pub fn surface_ptr(&self) -> *const u32 { self.surface.as_ptr() }

    /// Ham piksel arabelleği pointer'ı (yazma erişimi).
    pub fn surface_ptr_mut(&mut self) -> *mut u32 { self.surface.as_mut_ptr() }
}

// ---------------------------------------------------------------------------
// Küresel Window Server
// ---------------------------------------------------------------------------

pub struct WinServer {
    handles  : Vec<WindowHandle>,
    next_id  : u32,
    /// Odaklanmış pencere id'si (0 = yok)
    pub focused: u32,
}

impl WinServer {
    /// `const fn` ile derleme zamanında başlatılır; `static` değişkenle kullanıma uygundur.
    /// `handles` vektörü boş, `next_id` = 1, `focused` = 0 (odak yok) ile gelir.
    const fn new() -> Self {
        WinServer { handles: Vec::new(), next_id: 1, focused: 0 }
    }

    /// Monoton artan pencere kimliği tahsis eder.
    /// Taşma (overflow) durumunda 0 atlanarak 1'e sarar; 0 "geçersiz id" anlamına gelir.
    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        if self.next_id == 0 { self.next_id = 1; }
        id
    }

    /// Yeni pencere oluştur; id > 0 başarı, 0 hata demektir.
    pub fn create(&mut self, tid: usize, x: i32, y: i32, w: u32, h: u32, title: &str) -> u32 {
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return 0;
        }
        let id = self.alloc_id();
        self.handles.push(WindowHandle::new(id, tid, x, y, w, h, String::from(title)));
        // İlk pencere otomatik odak alır
        if self.focused == 0 { self.focused = id; }
        id
    }

    /// Pencereyi kapat ve kaydından sil.
    pub fn destroy(&mut self, id: u32) -> bool {
        if let Some(pos) = self.handles.iter().position(|h| h.id == id) {
            self.handles.remove(pos);
            if self.focused == id {
                self.focused = self.handles.last().map(|h| h.id).unwrap_or(0);
            }
            true
        } else { false }
    }

    /// Salt-okunur erişim.
    pub fn get(&self, id: u32) -> Option<&WindowHandle> {
        self.handles.iter().find(|h| h.id == id)
    }

    /// Yazma erişimi.
    pub fn get_mut(&mut self, id: u32) -> Option<&mut WindowHandle> {
        self.handles.iter_mut().find(|h| h.id == id)
    }

    /// Compositor'a sunulacak dirty pencereler.
    pub fn dirty_handles(&self) -> impl Iterator<Item = &WindowHandle> {
        self.handles.iter().filter(|h| h.dirty)
    }

    /// Tüm dirty bayraklarını sıfırla.
    pub fn clear_dirty(&mut self) {
        for h in &mut self.handles { h.dirty = false; }
    }

    /// Belirli bir göreve ait tüm pencereler için olay yayınla.
    pub fn broadcast_to_tid(&mut self, tid: usize, ev: WinEvent) {
        for h in &mut self.handles {
            if h.tid == tid { h.push_event(ev); }
        }
    }

    /// Belirli bir pencereye olay gönder.
    pub fn send_event(&mut self, id: u32, ev: WinEvent) {
        if let Some(h) = self.get_mut(id) { h.push_event(ev); }
    }

    /// Tüm tanıtıcılara erişim (compositor render döngüsü için).
    pub fn all_handles(&self) -> &[WindowHandle] { &self.handles }
}

// ---------------------------------------------------------------------------
// Statik global singleton
// ---------------------------------------------------------------------------

static WIN_SERVER: Mutex<WinServer> = Mutex::new(WinServer::new());

/// Güvenli kilitli erişim yardımcısı.
#[inline]
pub fn with_server<F: FnOnce(&mut WinServer) -> R, R>(f: F) -> R {
    f(&mut WIN_SERVER.lock())
}

// ---------------------------------------------------------------------------
// Compositor yardımcı fonksiyonlar (çekirdek içi kullanım)
// ---------------------------------------------------------------------------

/// User-space pencerelerini çerçeveye kopyalayan compositor yardımcısı.
/// `fb_ptr`: ekran tamponuna doğrudan yazar:
///   src: win.surface (ARGB), dst: linear framebuffer (BGRX ya da ARGB).
/// Koordinatlar zaten pencerenin (x, y) konumundan gelir.
pub fn composite_user_windows(
    fb_ptr: *mut u32,
    fb_width: u32,
    fb_height: u32,
    fb_stride: u32,
) {
    let srv = WIN_SERVER.lock();
    for h in srv.all_handles() {
        if !h.dirty { continue; }
        let x0 = h.x.max(0) as u32;
        let y0 = h.y.max(0) as u32;
        let x1 = (h.x + h.width  as i32).min(fb_width  as i32) as u32;
        let y1 = (h.y + h.height as i32).min(fb_height as i32) as u32;
        if x1 <= x0 || y1 <= y0 { continue; }

        for row in y0..y1 {
            let src_row = (row - y0) as usize;
            let src_base = src_row * h.width as usize;
            let dst_base = (row * fb_stride + x0) as usize;
            let cols = (x1 - x0) as usize;

            let src_slice = &h.surface[src_base..src_base + cols];
            let dst_slice = unsafe {
                core::slice::from_raw_parts_mut(fb_ptr.add(dst_base), cols)
            };
            // Porter-Duff src-over: alfa kanalı ARGB yüksek bayttadır.
            for (d, &s) in dst_slice.iter_mut().zip(src_slice.iter()) {
                let a = (s >> 24) as u32;
                if a == 255 {
                    *d = s & 0x00FF_FFFF; // BGRX: alfa biti sıfırla
                } else if a == 0 {
                    // Saydam: geç
                } else {
                    let ia = 255 - a;
                    let sr = (s >> 16) & 0xFF;
                    let sg = (s >>  8) & 0xFF;
                    let sb =  s        & 0xFF;
                    let dr = (*d >> 16) & 0xFF;
                    let dg = (*d >>  8) & 0xFF;
                    let db =  *d        & 0xFF;
                    let r = (sr * a + dr * ia) / 255;
                    let g = (sg * a + dg * ia) / 255;
                    let b = (sb * a + db * ia) / 255;
                    *d = (r << 16) | (g << 8) | b;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Syscall köprüleri  (src/posix.rs dispatch() tarafından çağrılır)
// ---------------------------------------------------------------------------

/// SYS_WIN_CREATE = 451
///   args: x, y, width, height, title_ptr, title_len
///   Döndürür: window id (> 0) ya da 0 (başarısız)
pub fn sys_win_create(
    arg0: usize, arg1: usize, arg2: usize,
    arg3: usize, arg4: usize, arg5: usize,
) -> usize {
    let x = arg0 as i32;
    let y = arg1 as i32;
    let w = arg2 as u32;
    let h = arg3 as u32;
    let title_ptr = arg4 as *const u8;
    let title_len = arg5.min(127);

    let title_ok = !title_ptr.is_null()
        && title_len > 0
        && crate::memory::is_user_range(title_ptr as u64, title_len as u64);

    let title: String = if !title_ok {
        String::from("Pencere")
    } else {
        let bytes = unsafe { core::slice::from_raw_parts(title_ptr, title_len) };
        String::from_utf8_lossy(bytes).into_owned()
    };

    let tid = crate::task::scheduler::current_task_id();
    let id  = with_server(|srv| srv.create(tid, x, y, w, h, &title));
    id as usize
}

/// SYS_WIN_DESTROY = 452
///   args: window_id
///   Döndürür: 0 başarı, usize::MAX bulunamadı
pub fn sys_win_destroy(
    arg0: usize, _: usize, _: usize,
    _: usize,    _: usize, _: usize,
) -> usize {
    let id = arg0 as u32;
    let tid = crate::task::scheduler::current_task_id();
    let ok = with_server(|srv| {
        if let Some(h) = srv.get(id) {
            if h.tid != tid {
                return false;
            }
        } else {
            return false;
        }
        srv.destroy(id)
    });
    if ok { 0 } else { usize::MAX }
}

/// SYS_WIN_GET_BUFFER = 453
///   args: window_id
///   Döndürür: piksel arabelleğinin çekirdek adresi (user-space bu adrese yazar)
///   NOT: tam MMU izolasyonu için ileride kullanıcı sayfa eşlemesi gerekir.
pub fn sys_win_get_buffer(
    arg0: usize, _: usize, _: usize,
    _: usize,    _: usize, _: usize,
) -> usize {
    let id = arg0 as u32;
    let tid = crate::task::scheduler::current_task_id();
    with_server(|srv| {
        if let Some(h) = srv.get(id) {
            if h.tid == tid { h.surface_ptr() as usize } else { 0 }
        } else { 0 }
    })
}

/// SYS_WIN_FLUSH = 454
///   args: window_id
///   Arabelleği dirty olarak işaretle; compositor bir sonraki karede birleştirir.
pub fn sys_win_flush(
    arg0: usize, _: usize, _: usize,
    _: usize,    _: usize, _: usize,
) -> usize {
    let id = arg0 as u32;
    let tid = crate::task::scheduler::current_task_id();
    with_server(|srv| {
        if let Some(h) = srv.get_mut(id) {
            if h.tid != tid {
                return usize::MAX;
            }
            h.dirty = true;
            0
        } else { usize::MAX }
    })
}

/// SYS_EVENT_POLL = 455
///   args: window_id, out_event_ptr
///   Döndürür: 1 olay çekildi, 0 kuyruk boş
///   `out_event_ptr` kullanıcı alanında `WinEvent` yapısını gösterir.
pub fn sys_event_poll(
    arg0: usize, arg1: usize, _: usize,
    _: usize,    _: usize,   _: usize,
) -> usize {
    let id  = arg0 as u32;
    let ptr = arg1 as *mut WinEvent;
    let ptr_ok = !ptr.is_null()
        && crate::memory::is_user_range(ptr as u64, core::mem::size_of::<WinEvent>() as u64);
    if !ptr_ok { return 0; }

    let tid = crate::task::scheduler::current_task_id();

    with_server(|srv| {
        if let Some(h) = srv.get_mut(id) {
            if h.tid != tid {
                return 0;
            }
            if let Some(ev) = h.pop_event() {
                unsafe { ptr.write(ev); }
                1
            } else { 0 }
        } else { 0 }
    })
}
