use alloc::string::String;
use alloc::vec::Vec;

use super::{get_snmp_counters, SnmpSnapshot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnmpValue {
    Counter64(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnmpVarBind {
    pub oid: &'static str,
    pub name: &'static str,
    pub value: SnmpValue,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnmpAgentSnapshot {
    pub vars: Vec<SnmpVarBind>,
}

impl SnmpAgentSnapshot {
    pub fn render(&self) -> String {
        let mut out = String::new();
        for var in &self.vars {
            let value = match var.value {
                SnmpValue::Counter64(v) => v,
            };
            out.push_str(var.oid);
            out.push(' ');
            out.push_str(var.name);
            out.push(' ');
            out.push_str(&alloc::format!("{}", value));
            out.push('\n');
        }
        out
    }
}

fn push_counter(
    vars: &mut Vec<SnmpVarBind>,
    oid: &'static str,
    name: &'static str,
    value: u64,
) {
    vars.push(SnmpVarBind {
        oid,
        name,
        value: SnmpValue::Counter64(value),
    });
}

pub fn snapshot_from_counters(counters: &SnmpSnapshot) -> SnmpAgentSnapshot {
    let mut vars = Vec::new();

    push_counter(&mut vars, "1.3.6.1.2.1.4.3", "ipInReceives", counters.ip_in_receives);
    push_counter(&mut vars, "1.3.6.1.2.1.4.4", "ipInHdrErrors", counters.ip_in_hdr_errors);
    push_counter(&mut vars, "1.3.6.1.2.1.4.5", "ipInAddrErrors", counters.ip_in_addr_errors);
    push_counter(&mut vars, "1.3.6.1.2.1.4.9", "ipInDelivers", counters.ip_in_delivers);
    push_counter(&mut vars, "1.3.6.1.2.1.4.10", "ipOutRequests", counters.ip_out_requests);
    push_counter(&mut vars, "1.3.6.1.2.1.6.5", "tcpActiveOpens", counters.tcp_active_opens);
    push_counter(&mut vars, "1.3.6.1.2.1.6.6", "tcpPassiveOpens", counters.tcp_passive_opens);
    push_counter(&mut vars, "1.3.6.1.2.1.6.10", "tcpInSegs", counters.tcp_in_segs);
    push_counter(&mut vars, "1.3.6.1.2.1.6.11", "tcpOutSegs", counters.tcp_out_segs);
    push_counter(&mut vars, "1.3.6.1.2.1.6.12", "tcpRetransSegs", counters.tcp_retrans_segs);
    push_counter(&mut vars, "1.3.6.1.2.1.7.1", "udpInDatagrams", counters.udp_in_datagrams);
    push_counter(&mut vars, "1.3.6.1.2.1.7.4", "udpOutDatagrams", counters.udp_out_datagrams);
    push_counter(&mut vars, "1.3.6.1.2.1.7.3", "udpNoPorts", counters.udp_no_ports);
    push_counter(&mut vars, "1.3.6.1.2.1.7.2", "udpInErrors", counters.udp_in_errors);

    SnmpAgentSnapshot { vars }
}

pub fn get_agent_snapshot() -> SnmpAgentSnapshot {
    let counters = get_snmp_counters();
    snapshot_from_counters(&counters)
}

pub fn render_agent_snapshot() -> String {
    get_agent_snapshot().render()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_contains_core_oids() {
        let counters = SnmpSnapshot {
            ip_in_receives: 10,
            tcp_active_opens: 2,
            udp_out_datagrams: 7,
            ..SnmpSnapshot::default()
        };
        let snapshot = snapshot_from_counters(&counters);
        assert!(snapshot.vars.iter().any(|v| v.oid == "1.3.6.1.2.1.4.3"));
        assert!(snapshot.vars.iter().any(|v| v.oid == "1.3.6.1.2.1.6.5"));
        assert!(snapshot.vars.iter().any(|v| v.oid == "1.3.6.1.2.1.7.4"));
    }

    #[test]
    fn render_emits_name_and_value() {
        let counters = SnmpSnapshot {
            ip_in_receives: 42,
            ..SnmpSnapshot::default()
        };
        let rendered = snapshot_from_counters(&counters).render();
        assert!(rendered.contains("ipInReceives"));
        assert!(rendered.contains("42"));
    }
}
