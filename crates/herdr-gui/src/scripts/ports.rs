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

pub(super) fn all_listening_ports() -> HashMap<u32, Vec<u16>> {
    let output = Command::new("lsof")
        .args(["-Pan", "-iTCP", "-sTCP:LISTEN"])
        .output();
    let Ok(output) = output else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }
    parse_lsof_listening_ports_by_pid(&String::from_utf8_lossy(&output.stdout))
}

#[derive(Clone, Debug, Default)]
pub(super) struct ProcessTreeSnapshot {
    pub parents: HashMap<u32, u32>,
    pub commands: HashMap<u32, String>,
}

impl ProcessTreeSnapshot {
    pub(super) fn capture() -> Self {
        let output = Command::new("ps")
            .args(["-A", "-o", "pid=,ppid=,command="])
            .output();
        let Ok(output) = output else {
            return Self::default();
        };
        if !output.status.success() {
            return Self::default();
        }
        Self::parse(&String::from_utf8_lossy(&output.stdout))
    }

    pub(super) fn parse(output: &str) -> Self {
        let mut parents = HashMap::new();
        let mut commands = HashMap::new();
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut parts = trimmed.split_whitespace();
            let Some(pid) = parts.next().and_then(|p| p.parse::<u32>().ok()) else {
                continue;
            };
            let Some(ppid) = parts.next().and_then(|p| p.parse::<u32>().ok()) else {
                continue;
            };
            parents.insert(pid, ppid);

            let cmd = trimmed
                .split_once(char::is_whitespace)
                .and_then(|(_, rest)| rest.trim_start().split_once(char::is_whitespace))
                .map(|(_, cmd)| cmd.trim().to_string())
                .unwrap_or_default();
            if !cmd.is_empty() {
                commands.insert(pid, cmd);
            }
        }
        Self { parents, commands }
    }

    pub(super) fn is_descendant_of(&self, mut pid: u32, target_pids: &HashSet<u32>) -> bool {
        let mut visited = HashSet::new();
        while pid > 1 && visited.insert(pid) {
            if target_pids.contains(&pid) {
                return true;
            }
            let Some(&parent) = self.parents.get(&pid) else {
                break;
            };
            pid = parent;
        }
        false
    }
}

pub(super) fn find_listening_for_pane(
    target_pids: &HashSet<u32>,
    listening_ports: &HashMap<u32, Vec<u16>>,
    tree: &ProcessTreeSnapshot,
) -> Vec<(u32, Vec<u16>)> {
    let mut matches = Vec::new();
    for (&lpid, ports) in listening_ports {
        if target_pids.contains(&lpid) || tree.is_descendant_of(lpid, target_pids) {
            matches.push((lpid, ports.clone()));
        }
    }
    matches
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

    #[test]
    fn process_tree_detects_descendant_ancestry() {
        let ps_output =
            "  100     1 -zsh\n  105   100 pnpm dev\n  110   105 node /path/to/vite.js\n";
        let tree = ProcessTreeSnapshot::parse(ps_output);
        assert_eq!(tree.parents.get(&110), Some(&105));
        assert_eq!(tree.parents.get(&105), Some(&100));

        let mut shell_roots = HashSet::new();
        shell_roots.insert(100);
        assert!(tree.is_descendant_of(110, &shell_roots));
        assert!(tree.is_descendant_of(105, &shell_roots));
        assert!(!tree.is_descendant_of(200, &shell_roots));

        let mut listening = HashMap::new();
        listening.insert(110, vec![5173]);
        let matches = find_listening_for_pane(&shell_roots, &listening, &tree);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, 110);
        assert_eq!(matches[0].1, vec![5173]);
    }
}
