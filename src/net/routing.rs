//! # Policy Routing ve FIB (Forwarding Information Base)
//!
//! Linux `ip rule` ve `ip route` komutlarının eşdeğeri.
//! Çoklu routing table + rule-based seçim ile policy routing sağlar.
//!
//! ## Policy Routing Nedir?
//!
//! Normal routing tek bir tablo kullanır. Policy routing:
//! - Kaynak IP'ye göre farklı gateway (multi-ISP)
//! - fwmark'a göre route seçimi
//! - TOS/DSCP bazlı yönlendirme
//!
//! ```text
//!  Paket geldi
//!      │
//!      ▼
//!  ┌─────────────────────────────────────────┐
//!  │  Rule Tablosu (öncelik sıralı)          │
//!  │  rule 0: from all lookup local          │
//!  │  rule 100: from 10.0.0.0/8 lookup 100   │
//!  │  rule 200: fwmark 1 lookup 200          │
//!  │  rule 32766: from all lookup main       │
//!  └─────────────────────────────────────────┘
//!      │
//!      ▼ (ilk eşleşen rule'un tablosuna bak)
//!  ┌─────────────────────────────────────────┐
//!  │  FIB Trie (LPM lookup)                  │
//!  │  10.0.0.0/8 → gw 10.0.0.1 dev eth0     │
//!  │  192.168.1.0/24 → dev eth1              │
//!  │  default → gw 192.168.1.1 dev eth0      │
//!  └─────────────────────────────────────────┘
//! ```
//!
//! ## FIB Trie (Longest Prefix Match)
//!
//! 32-bit IP adresi için binary trie. Her bit bir node.
//! O(32) = O(1) lookup. Linux'un trie implementasyonundan
//! (fib_table_lookup) esinlenilmiştir.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use super::Ipv4Addr;

fn prefix_mask(prefix_len: u8) -> u32 {
    match prefix_len {
        0 => 0,
        32..=u8::MAX => u32::MAX,
        bits => (!0u32) << (32 - bits as u32),
    }
}

// ============================================================================
// ROUTING TABLE SABİTLERİ
// ============================================================================

/// Varsayılan route tablosu ID'leri (Linux uyumlu)
pub const RT_TABLE_UNSPEC: u32 = 0;
pub const RT_TABLE_COMPAT: u32 = 255;
pub const RT_TABLE_DEFAULT: u32 = 253;
pub const RT_TABLE_MAIN: u32 = 254;
pub const RT_TABLE_LOCAL: u32 = 255;

/// Route türleri
pub const RTN_UNSPEC: u8 = 0;
pub const RTN_UNICAST: u8 = 1;
pub const RTN_LOCAL: u8 = 2;
pub const RTN_BROADCAST: u8 = 3;
pub const RTN_ANYCAST: u8 = 4;
pub const RTN_MULTICAST: u8 = 5;
pub const RTN_BLACKHOLE: u8 = 6;
pub const RTN_UNREACHABLE: u8 = 7;
pub const RTN_PROHIBIT: u8 = 8;

/// Scope değerleri
pub const RT_SCOPE_UNIVERSE: u8 = 0;
pub const RT_SCOPE_SITE: u8 = 200;
pub const RT_SCOPE_LINK: u8 = 253;
pub const RT_SCOPE_HOST: u8 = 254;
pub const RT_SCOPE_NOWHERE: u8 = 255;

// ============================================================================
// ROUTE ENTRY (Tek bir routing table girdisi)
// ============================================================================

/// Route tablosu girdisi
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteEntry {
    /// Hedef ağ adresi (IP + prefix length)
    pub dst: u32,
    pub dst_prefix_len: u8,
    
    /// Gateway (next-hop) — 0.0.0.0 ise direkt bağlı
    pub gateway: u32,
    
    /// Çıkış arabirim adı (örn. "eth0")
    pub iface: String,
    
    /// Route türü (unicast, local, blackhole, vb.)
    pub route_type: u8,
    
    /// Scope (universe, link, host)
    pub scope: u8,
    
    /// Metric (düşük = tercih edilen)
    pub metric: u32,
    
    /// MTU override (0 = kullanma)
    pub mtu: u16,
    
    /// Route kaynağı (static, dhcp, kernel)
    pub protocol: u8,
}

impl RouteEntry {
    /// Yeni unicast route oluştur
    pub fn unicast(dst: u32, prefix_len: u8, gateway: u32, iface: &str, metric: u32) -> Self {
        RouteEntry {
            dst,
            dst_prefix_len: prefix_len,
            gateway,
            iface: String::from(iface),
            route_type: RTN_UNICAST,
            scope: if gateway == 0 { RT_SCOPE_LINK } else { RT_SCOPE_UNIVERSE },
            metric,
            mtu: 0,
            protocol: 4,  // static
        }
    }
    
    /// Default route (0.0.0.0/0)
    pub fn default_route(gateway: u32, iface: &str, metric: u32) -> Self {
        Self::unicast(0, 0, gateway, iface, metric)
    }
    
    /// Host route (/32)
    pub fn host_route(ip: u32, iface: &str) -> Self {
        Self::unicast(ip, 32, 0, iface, 0)
    }
    
    /// Direkt bağlı ağ (gateway yok)
    pub fn connected(network: u32, prefix_len: u8, iface: &str) -> Self {
        Self::unicast(network, prefix_len, 0, iface, 0)
    }
    
    /// Bu route verilen IP'yi kapsıyor mu?
    pub fn matches(&self, ip: u32) -> bool {
        if self.dst_prefix_len == 0 {
            return true;  // Default route her şeyi kapsar
        }
        let mask = prefix_mask(self.dst_prefix_len);
        (ip & mask) == (self.dst & mask)
    }
    
    /// Prefix uzunluğu (LPM karşılaştırması için)
    pub fn prefix_len(&self) -> u8 {
        self.dst_prefix_len
    }
}

// ============================================================================
// FIB TRIE (Longest Prefix Match)
// ============================================================================

/// FIB Trie node
struct FibNode {
    /// Bu prefix için metric sıralı aday route'lar
    routes: Vec<RouteEntry>,
    /// Sol çocuk (bit = 0)
    left: Option<Box<FibNode>>,
    /// Sağ çocuk (bit = 1)
    right: Option<Box<FibNode>>,
}

impl FibNode {
    fn new() -> Self {
        FibNode {
            routes: Vec::new(),
            left: None,
            right: None,
        }
    }
}

/// FIB Trie — Longest Prefix Match ile route lookup
pub struct FibTrie {
    root: FibNode,
    entry_count: u32,
}

impl FibTrie {
    pub fn new() -> Self {
        FibTrie {
            root: FibNode::new(),
            entry_count: 0,
        }
    }
    
    /// Route ekle
    pub fn insert(&mut self, entry: RouteEntry) {
        let prefix_len = entry.dst_prefix_len as u32;
        let dst = entry.dst & prefix_mask(entry.dst_prefix_len);
        
        let mut node = &mut self.root;
        
        for i in 0..prefix_len {
            let bit = (dst >> (31 - i)) & 1;
            let next = if bit == 0 {
                &mut node.left
            } else {
                &mut node.right
            };
            
            if next.is_none() {
                *next = Some(Box::new(FibNode::new()));
            }
            
            node = next.as_mut().unwrap();
        }
        
        if let Some(existing) = node
            .routes
            .iter_mut()
            .find(|route| route.gateway == entry.gateway && route.iface == entry.iface)
        {
            *existing = entry;
        } else {
            node.routes.push(entry);
            self.entry_count += 1;
        }
        node.routes.sort_by_key(|route| route.metric);
    }
    
    /// Route sil
    pub fn remove(&mut self, dst: u32, prefix_len: u8) -> bool {
        let prefix_len_u32 = prefix_len as u32;
        let dst = dst & prefix_mask(prefix_len);
        
        let mut node = &mut self.root;
        
        for i in 0..prefix_len_u32 {
            let bit = (dst >> (31 - i)) & 1;
            let next = if bit == 0 {
                &mut node.left
            } else {
                &mut node.right
            };
            
            match next {
                Some(n) => node = n,
                None => return false,
            }
        }
        
        if !node.routes.is_empty() {
            self.entry_count -= node.routes.len() as u32;
            node.routes.clear();
            true
        } else {
            false
        }
    }
    
    /// Longest Prefix Match lookup
    ///
    /// Verilen IP adresi için en spesifik (en uzun prefix) route'u döndürür.
    pub fn lookup(&self, ip: u32) -> Option<&RouteEntry> {
        self.lookup_candidates(ip)
            .and_then(|candidates| candidates.first())
    }

    /// En iyi eşleşen prefix için tüm aday route'ları metric sırasıyla döndürür.
    pub fn lookup_candidates(&self, ip: u32) -> Option<&[RouteEntry]> {
        let mut node = &self.root;
        let mut best = if node.routes.is_empty() {
            None
        } else {
            Some(node.routes.as_slice())
        };
        
        for i in 0..32u32 {
            let bit = (ip >> (31 - i)) & 1;
            let next = if bit == 0 {
                node.left.as_ref()
            } else {
                node.right.as_ref()
            };
            
            match next {
                Some(n) => {
                    node = n;
                    if !node.routes.is_empty() {
                        best = Some(node.routes.as_slice());
                    }
                }
                None => break,
            }
        }
        
        best
    }
    
    /// Route sayısı
    pub fn len(&self) -> u32 {
        self.entry_count
    }
    
    /// Boş mu?
    pub fn is_empty(&self) -> bool {
        self.entry_count == 0
    }
}

// ============================================================================
// ROUTING TABLE (Bir FIB trie + metadata)
// ============================================================================

/// Bir routing tablosu
pub struct RoutingTable {
    /// Tablo ID'si
    pub id: u32,
    /// Tablo adı (opsiyonel)
    pub name: String,
    /// FIB trie
    fib: FibTrie,
}

impl RoutingTable {
    pub fn new(id: u32, name: &str) -> Self {
        RoutingTable {
            id,
            name: String::from(name),
            fib: FibTrie::new(),
        }
    }
    
    pub fn add_route(&mut self, entry: RouteEntry) {
        self.fib.insert(entry);
    }
    
    pub fn remove_route(&mut self, dst: u32, prefix_len: u8) -> bool {
        self.fib.remove(dst, prefix_len)
    }
    
    pub fn lookup(&self, ip: u32) -> Option<&RouteEntry> {
        self.fib.lookup(ip)
    }

    pub fn lookup_candidates(&self, ip: u32) -> Option<&[RouteEntry]> {
        self.fib.lookup_candidates(ip)
    }
    
    pub fn route_count(&self) -> u32 {
        self.fib.len()
    }
}

// ============================================================================
// POLICY RULE
// ============================================================================

/// Routing policy kuralı (ip rule eşdeğeri)
#[derive(Clone, Debug)]
pub struct PolicyRule {
    /// Öncelik (düşük = önce değerlendirilir)
    pub priority: u32,
    
    /// Kaynak IP filtresi (0 = herhangi)
    pub src_match: u32,
    pub src_prefix_len: u8,
    
    /// Hedef IP filtresi (0 = herhangi)
    pub dst_match: u32,
    pub dst_prefix_len: u8,
    
    /// fwmark filtresi (0 = herhangi)
    pub fwmark: u32,
    pub fwmark_mask: u32,
    
    /// TOS filtresi (0xFF = herhangi)
    pub tos: u8,
    
    /// Eşleşirse hangi tabloya bakılacak
    pub table_id: u32,
    
    /// Input arabirim filtresi (boş = herhangi)
    pub iif: String,
    
    /// Output arabirim filtresi (boş = herhangi)
    pub oif: String,
}

impl PolicyRule {
    /// Varsayılan kural: tüm paketler → main tablosu
    pub fn default_main(priority: u32) -> Self {
        PolicyRule {
            priority,
            src_match: 0,
            src_prefix_len: 0,
            dst_match: 0,
            dst_prefix_len: 0,
            fwmark: 0,
            fwmark_mask: 0,
            tos: 0xFF,
            table_id: RT_TABLE_MAIN,
            iif: String::new(),
            oif: String::new(),
        }
    }
    
    /// Source-based rule
    pub fn from_source(priority: u32, src: u32, prefix_len: u8, table_id: u32) -> Self {
        PolicyRule {
            priority,
            src_match: src,
            src_prefix_len: prefix_len,
            dst_match: 0,
            dst_prefix_len: 0,
            fwmark: 0,
            fwmark_mask: 0,
            tos: 0xFF,
            table_id,
            iif: String::new(),
            oif: String::new(),
        }
    }
    
    /// fwmark-based rule
    pub fn from_fwmark(priority: u32, fwmark: u32, mask: u32, table_id: u32) -> Self {
        PolicyRule {
            priority,
            src_match: 0,
            src_prefix_len: 0,
            dst_match: 0,
            dst_prefix_len: 0,
            fwmark,
            fwmark_mask: mask,
            tos: 0xFF,
            table_id,
            iif: String::new(),
            oif: String::new(),
        }
    }
    
    /// Bu kural verilen paket parametrelerine uyuyor mu?
    pub fn matches(&self, src_ip: u32, dst_ip: u32, mark: u32, tos: u8) -> bool {
        // Source IP kontrolü
        if self.src_prefix_len > 0 {
            let mask = prefix_mask(self.src_prefix_len);
            if (src_ip & mask) != (self.src_match & mask) {
                return false;
            }
        }
        
        // Destination IP kontrolü
        if self.dst_prefix_len > 0 {
            let mask = prefix_mask(self.dst_prefix_len);
            if (dst_ip & mask) != (self.dst_match & mask) {
                return false;
            }
        }
        
        // fwmark kontrolü
        if self.fwmark_mask != 0 {
            if (mark & self.fwmark_mask) != (self.fwmark & self.fwmark_mask) {
                return false;
            }
        }
        
        // TOS kontrolü
        if self.tos != 0xFF && self.tos != tos {
            return false;
        }
        
        true
    }
}

// ============================================================================
// ROUTING MANAGER (Global)
// ============================================================================

/// Route lookup sonucu
#[derive(Clone, Debug)]
pub struct RouteResult {
    pub gateway: Ipv4Addr,
    pub iface: String,
    pub metric: u32,
    pub mtu: u16,
    pub table_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RouteCacheKey {
    src_ip: u32,
    dst_ip: u32,
    fwmark: u32,
    tos: u8,
}

/// Global routing manager
pub struct RoutingManager {
    /// Routing tabloları (table_id → RoutingTable)
    tables: BTreeMap<u32, RoutingTable>,
    
    /// Policy kuralları (öncelik sıralı)
    rules: Vec<PolicyRule>,
    
    /// Lookup istatistikleri
    lookups: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
    cache: Mutex<BTreeMap<RouteCacheKey, RouteResult>>,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    gateway_health: Mutex<BTreeMap<u32, bool>>,
    failovers: AtomicU64,
}

impl RoutingManager {
    pub fn new() -> Self {
        let mut tables = BTreeMap::new();
        
        // Varsayılan tablolar
        tables.insert(RT_TABLE_LOCAL, RoutingTable::new(RT_TABLE_LOCAL, "local"));
        tables.insert(RT_TABLE_MAIN, RoutingTable::new(RT_TABLE_MAIN, "main"));
        
        // Varsayılan kurallar
        let rules = vec![
            PolicyRule::default_main(32766),  // Tüm paketler → main tablosu
        ];
        
        RoutingManager {
            tables,
            rules,
            lookups: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            cache: Mutex::new(BTreeMap::new()),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            gateway_health: Mutex::new(BTreeMap::new()),
            failovers: AtomicU64::new(0),
        }
    }
    
    /// Tablo ekle
    pub fn add_table(&mut self, id: u32, name: &str) {
        self.tables.insert(id, RoutingTable::new(id, name));
        self.invalidate_cache();
    }
    
    /// Route ekle (belirtilen tabloya)
    pub fn add_route(&mut self, table_id: u32, entry: RouteEntry) {
        if let Some(table) = self.tables.get_mut(&table_id) {
            table.add_route(entry);
            self.invalidate_cache();
        }
    }
    
    /// Route sil
    pub fn remove_route(&mut self, table_id: u32, dst: u32, prefix_len: u8) -> bool {
        if let Some(table) = self.tables.get_mut(&table_id) {
            let removed = table.remove_route(dst, prefix_len);
            if removed {
                self.invalidate_cache();
            }
            removed
        } else {
            false
        }
    }
    
    /// Policy kuralı ekle (öncelik sıralı insert)
    pub fn add_rule(&mut self, rule: PolicyRule) {
        let pos = self.rules.partition_point(|r| r.priority <= rule.priority);
        self.rules.insert(pos, rule);
        self.invalidate_cache();
    }

    pub fn mark_gateway_failed(&self, gateway: u32) {
        if gateway == 0 {
            return;
        }
        self.gateway_health.lock().insert(gateway, false);
        self.invalidate_cache();
    }

    pub fn mark_gateway_healthy(&self, gateway: u32) {
        if gateway == 0 {
            return;
        }
        self.gateway_health.lock().insert(gateway, true);
        self.invalidate_cache();
    }

    pub fn failover_count(&self) -> u64 {
        self.failovers.load(Ordering::Relaxed)
    }
    
    /// Policy kuralı sil (priority ile)
    pub fn remove_rule(&mut self, priority: u32) -> bool {
        let len_before = self.rules.len();
        self.rules.retain(|r| r.priority != priority);
        let removed = self.rules.len() < len_before;
        if removed {
            self.invalidate_cache();
        }
        removed
    }
    
    /// Route lookup (policy routing ile)
    ///
    /// 1. Rule tablosunda ilk eşleşen kuralı bul
    /// 2. O kuralın gösterdiği tabloya bak
    /// 3. LPM ile en spesifik route'u bul
    pub fn lookup(
        &self,
        src_ip: u32,
        dst_ip: u32,
        fwmark: u32,
        tos: u8,
    ) -> Option<RouteResult> {
        self.lookups.fetch_add(1, Ordering::Relaxed);
        let cache_key = RouteCacheKey {
            src_ip,
            dst_ip,
            fwmark,
            tos,
        };
        if let Some(cached) = self.cache.lock().get(&cache_key).cloned() {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Some(cached);
        }
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
        
        // Policy rule evaluation (öncelik sıralı)
        for rule in &self.rules {
            if !rule.matches(src_ip, dst_ip, fwmark, tos) {
                continue;
            }
            
            // Eşleşen kuralın tablosuna bak
            if let Some(table) = self.tables.get(&rule.table_id) {
                if let Some(candidates) = table.lookup_candidates(dst_ip) {
                    let (entry, failover_used) = self.select_route_candidate(candidates)?;
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    if failover_used {
                        self.failovers.fetch_add(1, Ordering::Relaxed);
                    }
                    
                    let result = RouteResult {
                        gateway: Ipv4Addr::from_bytes(entry.gateway.to_be_bytes()),
                        iface: entry.iface.clone(),
                        metric: entry.metric,
                        mtu: entry.mtu,
                        table_id: rule.table_id,
                    };
                    self.cache.lock().insert(cache_key, result.clone());
                    return Some(result);
                }
            }
        }
        
        self.misses.fetch_add(1, Ordering::Relaxed);
        None
    }
    
    /// Yalnız hedef adrese göre lookup (geriye uyumluluk yüzeyi)
    pub fn route(&self, dst_ip: u32) -> Option<RouteResult> {
        self.lookup(0, dst_ip, 0, 0)
    }
    
    /// İstatistikler
    pub fn stats(&self) -> (u64, u64, u64) {
        (
            self.lookups.load(Ordering::Relaxed),
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }

    pub fn cache_stats(&self) -> (u64, u64, usize) {
        (
            self.cache_hits.load(Ordering::Relaxed),
            self.cache_misses.load(Ordering::Relaxed),
            self.cache.lock().len(),
        )
    }
    
    /// Tablo sayısını döndür
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }
    
    /// Kural sayısını döndür
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
    
    /// Tüm route'ları listele (table_id → Vec<RouteEntry>)
    pub fn dump_rules(&self) -> Vec<PolicyRule> {
        self.rules.clone()
    }

    /// Dump all table IDs currently registered.
    pub fn dump_tables(&self) -> Vec<u32> {
        self.tables.keys().copied().collect()
    }

    pub fn dump_routes(&self, table_id: u32) -> Vec<(u32, u8, u32, String)> {
        let mut result = Vec::new();
        if let Some(table) = self.tables.get(&table_id) {
            // Trie traverse (DFS)
            fn traverse(node: &FibNode, depth: u8, path: u32, result: &mut Vec<(u32, u8, u32, String)>) {
                for route in &node.routes {
                    result.push((
                        route.dst,
                        route.dst_prefix_len,
                        route.gateway,
                        route.iface.clone(),
                    ));
                }
                if let Some(ref left) = node.left {
                    traverse(left, depth + 1, path, result);
                }
                if let Some(ref right) = node.right {
                    traverse(right, depth + 1, path | (1 << (31 - depth as u32)), result);
                }
            }
            traverse(&table.fib.root, 0, 0, &mut result);
        }
        result
    }

    fn invalidate_cache(&self) {
        self.cache.lock().clear();
    }

    fn route_is_healthy(&self, route: &RouteEntry) -> bool {
        if route.gateway == 0 {
            return true;
        }
        self.gateway_health
            .lock()
            .get(&route.gateway)
            .copied()
            .unwrap_or(true)
    }

    fn select_route_candidate<'a>(
        &self,
        candidates: &'a [RouteEntry],
    ) -> Option<(&'a RouteEntry, bool)> {
        let mut fallback = None;

        for (idx, route) in candidates.iter().enumerate() {
            if idx == 0 {
                fallback = Some(route);
            }
            if self.route_is_healthy(route) {
                return Some((route, idx > 0));
            }
        }

        fallback.map(|route| (route, false))
    }
}

// ============================================================================
// GLOBAL ROUTING MANAGER
// ============================================================================

static ROUTING_MANAGER: Mutex<Option<RoutingManager>> = Mutex::new(None);

/// Routing manager'ı başlat
pub fn init() {
    let mut mgr = ROUTING_MANAGER.lock();
    *mgr = Some(RoutingManager::new());
    crate::serial_println!("[ROUTING] Policy routing + FIB trie initialized");
}

/// Route ekle (main tablosuna)
pub fn add_route(entry: RouteEntry) {
    let mut mgr = ROUTING_MANAGER.lock();
    if let Some(ref mut m) = *mgr {
        m.add_route(RT_TABLE_MAIN, entry);
    }
}

/// Route lookup (yalnız hedef adres)
pub fn route_lookup(dst_ip: u32) -> Option<RouteResult> {
    let mgr = ROUTING_MANAGER.lock();
    if let Some(ref m) = *mgr {
        m.route(dst_ip)
    } else {
        None
    }
}

/// Route lookup (policy routing ile)
pub fn policy_route_lookup(
    src_ip: u32,
    dst_ip: u32,
    fwmark: u32,
    tos: u8,
) -> Option<RouteResult> {
    let mgr = ROUTING_MANAGER.lock();
    if let Some(ref m) = *mgr {
        m.lookup(src_ip, dst_ip, fwmark, tos)
    } else {
        None
    }
}

/// Policy kuralı ekle
pub fn add_rule(rule: PolicyRule) {
    let mut mgr = ROUTING_MANAGER.lock();
    if let Some(ref mut m) = *mgr {
        m.add_rule(rule);
    }
}

/// Tablo ekle
pub fn add_table(id: u32, name: &str) {
    let mut mgr = ROUTING_MANAGER.lock();
    if let Some(ref mut m) = *mgr {
        m.add_table(id, name);
    }
}

/// Global route dump for netlink
pub fn dump_routes(table_id: u32) -> Vec<(u32, u8, u32, String)> {
    let mgr = ROUTING_MANAGER.lock();
    if let Some(ref m) = *mgr {
        m.dump_routes(table_id)
    } else {
        Vec::new()
    }
}

/// Global rule dump for netlink
pub fn dump_rules() -> Vec<PolicyRule> {
    let mgr = ROUTING_MANAGER.lock();
    if let Some(ref m) = *mgr {
        m.dump_rules()
    } else {
        Vec::new()
    }
}

/// Global table ID dump for netlink
pub fn dump_tables() -> Vec<u32> {
    let mgr = ROUTING_MANAGER.lock();
    if let Some(ref m) = *mgr {
        m.dump_tables()
    } else {
        Vec::new()
    }
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn fib_trie_basic_lookup() {
        let mut trie = FibTrie::new();
        
        // 10.0.0.0/8 → gateway 10.0.0.1
        trie.insert(RouteEntry::unicast(0x0A000000, 8, 0x0A000001, "eth0", 100));
        
        // 192.168.1.0/24 → direkt bağlı
        trie.insert(RouteEntry::connected(0xC0A80100, 24, "eth1"));
        
        // Default route
        trie.insert(RouteEntry::default_route(0xC0A80101, "eth0", 200));
        
        // LPM: 10.1.2.3 → /8 match
        let result = trie.lookup(0x0A010203);
        assert!(result.is_some());
        assert_eq!(result.unwrap().gateway, 0x0A000001);
        
        // LPM: 192.168.1.50 → /24 match (daha spesifik)
        let result = trie.lookup(0xC0A80132);
        assert!(result.is_some());
        assert_eq!(result.unwrap().gateway, 0);
        
        // LPM: 8.8.8.8 → default route
        let result = trie.lookup(0x08080808);
        assert!(result.is_some());
        assert_eq!(result.unwrap().gateway, 0xC0A80101);
    }
    
    #[test]
    fn fib_trie_longest_prefix_match() {
        let mut trie = FibTrie::new();
        
        // /8, /16, /24 ekle
        trie.insert(RouteEntry::unicast(0x0A000000, 8, 1, "eth0", 100));
        trie.insert(RouteEntry::unicast(0x0A010000, 16, 2, "eth0", 100));
        trie.insert(RouteEntry::unicast(0x0A010200, 24, 3, "eth0", 100));
        
        // 10.1.2.5 → /24 match (en uzun prefix)
        let result = trie.lookup(0x0A010205).unwrap();
        assert_eq!(result.gateway, 3);
        
        // 10.1.3.5 → /16 match
        let result = trie.lookup(0x0A010305).unwrap();
        assert_eq!(result.gateway, 2);
        
        // 10.2.0.1 → /8 match
        let result = trie.lookup(0x0A020001).unwrap();
        assert_eq!(result.gateway, 1);
    }
    
    #[test]
    fn fib_trie_remove() {
        let mut trie = FibTrie::new();
        
        trie.insert(RouteEntry::default_route(0xC0A80101, "eth0", 100));
        assert_eq!(trie.len(), 1);
        
        assert!(trie.remove(0, 0));
        assert_eq!(trie.len(), 0);
        assert!(trie.is_empty());
        
        // Silinmiş route lookup'ta yok
        assert!(trie.lookup(0x08080808).is_none());
    }

    #[test]
    fn fib_trie_keeps_lowest_metric_for_same_prefix_lookup() {
        let mut trie = FibTrie::new();

        trie.insert(RouteEntry::default_route(0xC0A80101, "eth0", 100));
        trie.insert(RouteEntry::default_route(0x0A000001, "eth1", 50));

        let result = trie.lookup(0x08080808).unwrap();
        assert_eq!(result.gateway, 0x0A000001);
        assert_eq!(result.metric, 50);

        let candidates = trie.lookup_candidates(0x08080808).unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].gateway, 0x0A000001);
        assert_eq!(candidates[0].metric, 50);
        assert_eq!(candidates[1].gateway, 0xC0A80101);
        assert_eq!(candidates[1].metric, 100);
    }
    
    #[test]
    fn policy_rule_matching() {
        let rule = PolicyRule::from_source(100, 0x0A000000, 8, 100);
        
        assert!(rule.matches(0x0A010203, 0x08080808, 0, 0));
        assert!(!rule.matches(0xC0A80101, 0x08080808, 0, 0));
    }
    
    #[test]
    fn policy_rule_fwmark() {
        let rule = PolicyRule::from_fwmark(200, 0x01, 0xFF, 200);
        
        assert!(rule.matches(0, 0, 0x01, 0));
        assert!(!rule.matches(0, 0, 0x02, 0));
    }
    
    #[test]
    fn routing_manager_policy() {
        let mut mgr = RoutingManager::new();
        
        // Tablo 100 ekle
        mgr.add_table(100, "custom");
        
        // Tablo 100'e route ekle
        mgr.add_route(100, RouteEntry::default_route(0x0A000001, "eth1", 50));
        
        // Main tabloya farklı default route
        mgr.add_route(RT_TABLE_MAIN, RouteEntry::default_route(0xC0A80101, "eth0", 100));
        
        // 10.x.x.x kaynağından gelenler → tablo 100
        mgr.add_rule(PolicyRule::from_source(100, 0x0A000000, 8, 100));
        
        // 10.0.0.5'ten gelen paket → tablo 100 (gateway 10.0.0.1)
        let result = mgr.lookup(0x0A000005, 0x08080808, 0, 0).unwrap();
        assert_eq!(result.gateway.to_u32(), 0x0A000001);
        assert_eq!(result.iface, "eth1");
        
        // 192.168.1.5'ten gelen paket → main tablo (gateway 192.168.1.1)
        let result = mgr.lookup(0xC0A80105, 0x08080808, 0, 0).unwrap();
        assert_eq!(result.gateway.to_u32(), 0xC0A80101);
        assert_eq!(result.iface, "eth0");
    }
    
    #[test]
    fn route_entry_matches() {
        let route = RouteEntry::unicast(0xC0A80100, 24, 0, "eth0", 100);
        
        assert!(route.matches(0xC0A80101));
        assert!(route.matches(0xC0A801FF));
        assert!(!route.matches(0xC0A80201));
    }
    
    #[test]
    fn routing_stats() {
        let mgr = RoutingManager::new();
        
        // Başlangıçta 0
        let (lookups, hits, misses) = mgr.stats();
        assert_eq!(lookups, 0);
        
        // Route ekle ve lookup yap
        let mut mgr = mgr;
        mgr.add_route(RT_TABLE_MAIN, RouteEntry::default_route(0xC0A80101, "eth0", 100));
        
        let _ = mgr.route(0x08080808);
        let (lookups, hits, _) = mgr.stats();
        assert_eq!(lookups, 1);
        assert_eq!(hits, 1);
    }

    #[test]
    fn routing_cache_populates_and_invalidates() {
        let mut mgr = RoutingManager::new();
        mgr.add_route(RT_TABLE_MAIN, RouteEntry::default_route(0xC0A80101, "eth0", 100));

        let first = mgr.route(0x08080808).unwrap();
        assert_eq!(first.gateway.to_u32(), 0xC0A80101);
        let (cache_hits, cache_misses, cache_len) = mgr.cache_stats();
        assert_eq!(cache_hits, 0);
        assert_eq!(cache_misses, 1);
        assert_eq!(cache_len, 1);

        let second = mgr.route(0x08080808).unwrap();
        assert_eq!(second.gateway.to_u32(), 0xC0A80101);
        let (cache_hits, _, cache_len) = mgr.cache_stats();
        assert_eq!(cache_hits, 1);
        assert_eq!(cache_len, 1);

        mgr.add_route(RT_TABLE_MAIN, RouteEntry::default_route(0x0A000001, "eth1", 50));
        let (_, _, cache_len) = mgr.cache_stats();
        assert_eq!(cache_len, 0);
    }

    #[test]
    fn routing_manager_fails_over_between_default_gateways() {
        let mut mgr = RoutingManager::new();
        let primary = 0xC0A80101;
        let backup = 0x0A000001;

        mgr.add_route(RT_TABLE_MAIN, RouteEntry::default_route(primary, "eth0", 10));
        mgr.add_route(RT_TABLE_MAIN, RouteEntry::default_route(backup, "eth1", 20));

        let first = mgr.route(0x08080808).unwrap();
        assert_eq!(first.gateway.to_u32(), primary);
        assert_eq!(first.iface, "eth0");

        mgr.mark_gateway_failed(primary);
        let failover = mgr.route(0x08080808).unwrap();
        assert_eq!(failover.gateway.to_u32(), backup);
        assert_eq!(failover.iface, "eth1");
        assert_eq!(mgr.failover_count(), 1);

        mgr.mark_gateway_healthy(primary);
        let recovered = mgr.route(0x08080808).unwrap();
        assert_eq!(recovered.gateway.to_u32(), primary);
        assert_eq!(recovered.iface, "eth0");
    }
}

pub fn mark_gateway_failed(gateway: u32) {
    let mgr = ROUTING_MANAGER.lock();
    if let Some(ref m) = *mgr {
        m.mark_gateway_failed(gateway);
    }
}

pub fn mark_gateway_healthy(gateway: u32) {
    let mgr = ROUTING_MANAGER.lock();
    if let Some(ref m) = *mgr {
        m.mark_gateway_healthy(gateway);
    }
}
