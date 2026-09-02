fn main() {
    let registry = shardlane_host::mux::MuxRegistry::with_builtins();
    for b in registry.backends() {
        eprintln!("backend: {}", b.id());
    }
    for l in registry.list_instances() {
        println!(
            "{} {} running={} display={:?}",
            l.backend, l.name, l.running, l.display_name
        );
    }
}
