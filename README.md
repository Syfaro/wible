# wible

**Wi**ndows **B**luetooth **L**ow **E**nergy access.

Provides read and write access (including notifications) for BLE devices on
Windows with a simple API.

To see what information can easily be obtained, try running the examples.

* `cargo run --example nearby_devices` will discover devices sending advertisements and display MAC addresses
* `cargo run --example discover [MAC]` will enumerate all services, characteristics, and descriptors on the device with the provided MAC address

## Examples

This example watches for advertisements, connects to any discovered devices, and counts the available services on each.

```rust
use wible::AdvertisementWatcher;

let watcher = AdvertisementWatcher::new().expect("Unable to create AdvertisementWatcher");
for advertisement in &watcher {
    println!("Saw advertisement: {:?}", advertisement);
    println!(
        "Saw {} services",
        advertisement
            .device()
            .expect("Unable to get device")
            .services()
            .map(|services| services.len())
            .unwrap_or(0)
    );
}
```
