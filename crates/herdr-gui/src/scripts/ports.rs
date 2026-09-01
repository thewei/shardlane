//! [INPUT]: Main-crate imports and sibling-module shared items passed through the scripts module root (super) via the `use super::*` chain
//! [OUTPUT]: Provides listening-port probing (lsof parsing and PID→port grouping)
//! [POS]: The port-probing slice of the scripts module, mechanically split out of scripts.rs
use super::*;

pub(super) fn listening_ports_for_pids(pids: &[u32]) -> Vec<u16> {
    listening_ports_by_pid(pids)
        .into_values()
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn listening_ports_by_pid(pids: &[u32]) -> HashMap<u32, Vec<u16>> {
    let pids = pids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if pids.is_empty() {
        return HashMap::new();
    }
    let pid_list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let output = Command::new("lsof")
        .args([
            "-Pan",
            "-a",
            "-p",
            pid_list.as_str(),
            "-iTCP",
            "-sTCP:LISTEN",
        ])
        .output();
    let Ok(output) = output else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }
    parse_lsof_listening_ports_by_pid(&String::from_utf8_lossy(&output.stdout))
}

fn parse_lsof_listening_ports_by_pid(output: &str) -> HashMap<u32, Vec<u16>> {
    let mut ports = HashMap::<u32, BTreeSet<u16>>::new();
    for line in output.lines().skip(1) {
        if !line.contains("(LISTEN)") {
            continue;
        }
        let columns = line.split_whitespace().collect::<Vec<_>>();
        let Some(pid) = columns.get(1).and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(port) = columns.iter().find_map(|token| {
            let (_, port) = token.rsplit_once(':')?;
            port.parse::<u16>().ok()
        }) else {
            continue;
        };
        ports.entry(pid).or_default().insert(port);
    }
    ports
        .into_iter()
        .map(|(pid, ports)| (pid, ports.into_iter().collect()))
        .collect()
}

#[cfg(test)]
fn parse_lsof_listening_ports(output: &str) -> Vec<u16> {
    parse_lsof_listening_ports_by_pid(output)
        .into_values()
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lsof_parser_extracts_only_listening_tcp_ports() {
        let output = "COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME\nnode 10 u 22u IPv6 0 0t0 TCP *:5173 (LISTEN)\nnode 10 u 23u IPv4 0 0t0 TCP 127.0.0.1:3000 (LISTEN)\nnode 10 u 24u IPv4 0 0t0 TCP 127.0.0.1:54321->127.0.0.1:443 (ESTABLISHED)\n";
        assert_eq!(parse_lsof_listening_ports(output), vec![3000, 5173]);
    }

    #[test]
    fn lsof_parser_groups_listening_ports_by_process() {
        let output = "COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME\nnode 10 u 22u IPv6 0 0t0 TCP *:5173 (LISTEN)\npython 20 u 23u IPv4 0 0t0 TCP 127.0.0.1:8000 (LISTEN)\nnode 10 u 24u IPv4 0 0t0 TCP 127.0.0.1:3000 (LISTEN)\n";
        let grouped = parse_lsof_listening_ports_by_pid(output);
        assert_eq!(grouped.get(&10), Some(&vec![3000, 5173]));
        assert_eq!(grouped.get(&20), Some(&vec![8000]));
    }
}
