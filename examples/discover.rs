use std::io::Read;
use wible::*;

fn main() {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::init();

    let mac = match std::env::args().nth(1) {
        Some(mac) => mac,
        None => {
            log::error!("Please run me with the MAC address of a device you wish to inspect.");
            return;
        }
    };

    let desired_addr: BluetoothAddress = match mac.parse() {
        Ok(addr) => addr,
        Err(err) => {
            log::error!("Invalid MAC: {}", err);
            return;
        }
    };

    log::info!("Waiting for device with MAC {} to appear", desired_addr);

    let watcher = AdvertisementWatcher::new().unwrap();

    // Hold a set of previously discovered items. This allows us to filter out
    // items we have already seen.
    let mut previous = std::collections::HashSet::new();

    'watcher: for advertisement in &watcher {
        let addr = advertisement.address().unwrap();

        if previous.contains(&addr) {
            continue;
        }
        previous.insert(addr);

        log::debug!("Advertisement from {}", addr);

        if addr != desired_addr {
            continue;
        }

        let device = advertisement.device().unwrap();
        let services = device.services().unwrap();
        for service in services {
            log::info!("{:?}", service);

            log::info!("- Characteristics:");
            for characteristic in service.characteristics().unwrap() {
                log::info!("\t{:?}", characteristic);

                if characteristic
                    .properties()
                    .unwrap()
                    .contains(CharacteristicProperties::READ)
                {
                    let mut io = characteristic.io().unwrap();

                    let mut buf = vec![0u8; 255];
                    let size = io.read(&mut buf).unwrap();
                    let mut ascii = Vec::with_capacity(size);
                    for byte in &buf[..size] {
                        ascii.extend(std::ascii::escape_default(*byte));
                    }
                    let s = String::from_utf8(ascii).unwrap();
                    log::info!("\t- Contents (ASCII) [{}]: {}", size, s.trim());
                    log::info!("\t- Contents (bytes) [{}]: {:?}", size, &buf[..size]);
                } else {
                    log::info!("\t- No read property");
                }

                let descriptors = characteristic.descriptors().unwrap();
                if !descriptors.is_empty() {
                    log::info!("\t- Descriptors:");
                }
                for descriptor in descriptors {
                    log::info!("\t\t{:?}", descriptor);
                    let data = descriptor.read().unwrap();
                    let mut ascii = Vec::with_capacity(data.len());
                    for byte in &data[..data.len()] {
                        ascii.extend(std::ascii::escape_default(*byte));
                    }
                    let s = String::from_utf8(ascii).unwrap();
                    log::info!("\t\t- Contents (ASCII) [{}]: {}", data.len(), s.trim());
                    log::info!(
                        "\t\t- Contents (bytes) [{}]: {:?}",
                        data.len(),
                        &data[..data.len()]
                    );
                }
            }
        }

        break 'watcher;
    }
}
