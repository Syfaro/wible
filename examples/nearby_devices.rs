use wible::*;

fn main() {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::init();

    log::warn!("Note: this example runs forever, Ctrl+C when you are done.");

    let watcher = AdvertisementWatcher::new().unwrap();

    // Hold a set of previously discovered items. This allows us to filter out
    // items we have already seen.
    let mut previous = std::collections::HashSet::new();

    for advertisement in &watcher {
        let addr = advertisement.address().unwrap();

        if previous.contains(&addr) {
            continue;
        }
        previous.insert(addr);

        log::info!(
            "Discovered new device with MAC {} and signal strength {} dBm",
            addr,
            advertisement.signal_strength().unwrap()
        );
    }
}
