//! **Wi**ndows **B**luetooth **L**ow **E**nergy access.
//!
//! For usage, please look at the examples.
//!
//! A quick way to use this library would be to create a [AdvertisementWatcher],
//! wait until you find a device you are interested in, enumerate the services
//! and characteristics until you find the interfaces you need, then getting
//! a [CharacteristicIO] to [Read](std::io::Read) and [Write](std::io::Write) on
//! the device.

use std::sync::mpsc;

use winrt::import;

import!(
    dependencies
        os
    types
        windows::devices::bluetooth::*
        windows::devices::bluetooth::advertisement::*
        windows::devices::bluetooth::generic_attribute_profile::*
        windows::storage::streams::{DataReader, DataWriter}
);

use windows::devices::bluetooth::advertisement::{
    BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher,
};
use windows::devices::bluetooth::generic_attribute_profile::{
    GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue, GattDescriptor,
    GattDeviceService, GattValueChangedEventArgs,
};
use windows::devices::bluetooth::{BluetoothCacheMode, BluetoothLEDevice};
use windows::foundation::TypedEventHandler;
use windows::storage::streams::{DataReader, DataWriter};
use winrt::AbiTransferable;

/// BLE advertisement.
pub struct Advertisement {
    inner: BluetoothLEAdvertisementReceivedEventArgs,
}

impl std::ops::Deref for Advertisement {
    type Target = BluetoothLEAdvertisementReceivedEventArgs;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Advertisement {
    fn new(inner: BluetoothLEAdvertisementReceivedEventArgs) -> Self {
        Self { inner }
    }

    /// Get the MAC address of the device which sent this advertisement.
    pub fn address(&self) -> winrt::Result<BluetoothAddress> {
        let address = self.inner.bluetooth_address()?;
        let address = BluetoothAddress(address);
        Ok(address)
    }

    /// Device signal strength as seen for this advertisement as dBm.
    pub fn signal_strength(&self) -> winrt::Result<i16> {
        self.inner.raw_signal_strength_in_dbm()
    }

    /// Get a connection to the device which sent this advertisement.
    pub fn device(&self) -> winrt::Result<Device> {
        Device::from_address(self.address()?)
    }
}

impl std::fmt::Debug for Advertisement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Advertisement")
            .field("address", &self.address())
            .finish()
    }
}

/// Discovered BLE device.
#[derive(Debug)]
pub struct Device {
    inner: BluetoothLEDevice,
}

impl std::ops::Deref for Device {
    type Target = BluetoothLEDevice;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Device {
    /// Get a device connection by MAC address.
    pub fn from_address(addr: BluetoothAddress) -> winrt::Result<Self> {
        let inner = BluetoothLEDevice::from_bluetooth_address_async(addr.0)?.get()?;

        Ok(Device { inner })
    }

    /// Get a list of services provided by this device.
    pub fn services(&self) -> winrt::Result<Vec<Service>> {
        let services = self.inner.get_gatt_services_async()?.get()?.services()?;

        Ok(services.into_iter().map(Service::new).collect())
    }
}

/// Discovered BLE service.
pub struct Service {
    inner: GattDeviceService,
}

impl Service {
    fn new(inner: GattDeviceService) -> Self {
        Self { inner }
    }

    /// Get the list of available characteristics on this service.
    pub fn characteristics(&self) -> winrt::Result<Vec<Characteristic>> {
        let characteristics = self
            .inner
            .get_characteristics_async()?
            .get()?
            .characteristics()?;

        Ok(characteristics
            .into_iter()
            .map(Characteristic::new)
            .collect())
    }
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Service")
            .field("uuid", &self.inner.uuid().unwrap_or_default())
            .finish()
    }
}

impl std::ops::Deref for Service {
    type Target = GattDeviceService;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Discovered BLE characteristic.
pub struct Characteristic {
    inner: GattCharacteristic,
}

// These values are in GattCharacteristicProperties but not accessible in a
// const way.
bitflags::bitflags! {
    /// Properties about a characteristic, contains information such as if the
    /// characteristic can be read or written, or if it can notify on changes.
    pub struct CharacteristicProperties: u32 {
        const BROADCAST = 1;
        const READ = 2;
        const WRITE_WITHOUT_RESPONSE = 4;
        const WRITE = 8;
        const NOTIFY = 16;
        const INDICATE = 32;
        const AUTHENTICATED_SIGNED_WRITES = 64;
        const EXTENDED_PROPERTIES = 128;
        const RELIABLE_WRITES = 256;
        const WRITABLE_AUXILIARIES = 512;
    }
}

/// Discovered BLE characteristic.
impl Characteristic {
    fn new(inner: GattCharacteristic) -> Self {
        Self { inner }
    }

    /// Get the properties of this characteristic.
    ///
    /// Essential to discover before attempting to read or write data.
    pub fn properties(&self) -> Option<CharacteristicProperties> {
        let props = match self.inner.characteristic_properties().ok() {
            Some(props) => props,
            None => return None,
        };

        let value = props.get_abi();
        CharacteristicProperties::from_bits(value)
    }

    /// Read data from device without using cache. Returns as much data as is
    /// currently available, does not block.
    ///
    /// If this characteristic does not support reading, this will return an
    /// error.
    fn read(&self) -> winrt::Result<Vec<u8>> {
        log::trace!("Reading data from {:?}", &self);

        let value = self
            .inner
            .read_value_with_cache_mode_async(BluetoothCacheMode::Uncached)?
            .get()?
            .value()?;

        let reader = DataReader::from_buffer(&value)?;
        let mut buf = vec![0u8; value.length()? as usize];
        reader.read_bytes(&mut buf)?;

        Ok(buf)
    }

    /// Write data to a device.
    ///
    /// If this characteristic does not support writing, this will return an
    /// error.
    fn write(&self, data: &[u8]) -> winrt::Result<()> {
        log::trace!("Writing data to {:?}", &self);

        let writer = DataWriter::new()?;
        writer.write_bytes(&data)?;
        let buf = writer.detach_buffer()?;
        self.inner.write_value_async(&buf).map(|_val| ())
    }

    /// Get a [CharacteristicIO] instance for this characteristic which provides
    /// the [Read](std::io::Read) and [Write](std::io::Write) traits.
    ///
    /// It also configures notifications for characteristics that support it.
    pub fn io(&self) -> winrt::Result<CharacteristicIO> {
        CharacteristicIO::new(&self)
    }

    /// Get the list of descriptors on this characteristic.
    pub fn descriptors(&self) -> winrt::Result<Vec<Descriptor>> {
        let descriptors = self.inner.get_descriptors_async()?.get()?.descriptors()?;

        Ok(descriptors.into_iter().map(Descriptor::new).collect())
    }
}

impl std::fmt::Debug for Characteristic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Characteristic")
            .field("uuid", &self.inner.uuid().unwrap_or_default())
            .field("properties", &self.properties())
            .finish()
    }
}

impl std::ops::Deref for Characteristic {
    type Target = GattCharacteristic;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// An accessible way to read and write data from a characteristic.
///
/// It provides the [Read](std::io::Read) and [Write](std::io::Write) traits
/// for easy access to device I/O. It also configures notifications when
/// supported by the characteristic and cleans up after itself on drop.
///
/// Reading currently has a non-configurable 1 second timeout when waiting for
/// notifications. If no data is received, it may return a 0-length response.
/// This does not mean EOF, just that no data is currently available.
///
/// # Panics
///
/// Will panic if read or write is used on a characteristic that does not
/// support reading or writing, respectively.
pub struct CharacteristicIO<'a> {
    characteristic: &'a Characteristic,
    buf: Vec<u8>,

    rx: Option<mpsc::Receiver<Vec<u8>>>,
}

impl<'a> CharacteristicIO<'a> {
    /// Create a new instance, configuring notifications if supported.
    fn new(characteristic: &'a Characteristic) -> winrt::Result<Self> {
        let rx = match characteristic.properties() {
            Some(props) if props.contains(CharacteristicProperties::NOTIFY) => {
                Some(Self::configure_notify(&characteristic)?)
            }
            _ => None,
        };

        Ok(Self {
            characteristic,
            buf: Default::default(),
            rx,
        })
    }

    /// Create a channel for getting updates from notifications.
    fn configure_notify(characteristic: &Characteristic) -> winrt::Result<mpsc::Receiver<Vec<u8>>> {
        type Handler = TypedEventHandler<GattCharacteristic, GattValueChangedEventArgs>;
        let notify = GattClientCharacteristicConfigurationDescriptorValue::Notify;

        let (tx, rx) = mpsc::channel();

        let handler = Handler::new(move |_characteristic, value| {
            log::trace!("Got subscribe notify {:?}", value);

            let value = value.characteristic_value()?;
            let reader = DataReader::from_buffer(&value)?;
            let mut buf = vec![0u8; value.length()? as usize];
            reader.read_bytes(&mut buf)?;

            if let Err(err) = tx.send(buf) {
                log::error!("Unable to send subscribed notify: {:?}", err);
            }

            Ok(())
        });

        log::debug!(
            "Setting notify configuration descriptor on {:?}",
            &characteristic
        );

        characteristic
            .write_client_characteristic_configuration_descriptor_async(notify)?
            .get()?;
        characteristic.value_changed(handler)?;

        Ok(rx)
    }
}

impl std::fmt::Debug for CharacteristicIO<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CharacterIO")
            .field("characteristic", &self.characteristic)
            .field("buf", &self.buf)
            .finish()
    }
}

impl std::io::Read for CharacteristicIO<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Some(props) = self.characteristic.properties() {
            if !props.contains(CharacteristicProperties::READ) {
                panic!("Characteristic does not have read");
            }
        }

        // If we're using notifications, always pull new data into buffer.
        // Otherwise, only attempt to read if the buffer is empty.
        if let Some(rx) = &self.rx {
            // We only _need_ to get new data if the buffer is empty. Otherwise,
            // be perfectly content with the data currently available.
            let data = if self.buf.is_empty() {
                // TODO: configurable timeout
                rx.recv_timeout(std::time::Duration::from_secs(1)).ok()
            } else {
                rx.try_recv().ok()
            };

            if let Some(data) = data {
                self.buf.extend(data);
            }
        } else if self.buf.is_empty() {
            let data = self
                .characteristic
                .read()
                .map_err(|_err| std::io::Error::from(std::io::ErrorKind::Other))?;
            self.buf.extend(data);
        }

        let len = std::cmp::min(buf.len(), self.buf.len());
        let data: Vec<_> = self.buf.drain(..len).collect();

        buf[..len].copy_from_slice(&data);
        Ok(len)
    }
}

impl std::io::Write for CharacteristicIO<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(props) = self.characteristic.properties() {
            if !props.contains(CharacteristicProperties::WRITE) {
                panic!("Characteristic does not have write");
            }
        }

        self.characteristic
            .write(&buf)
            .map_err(|_err| std::io::Error::from(std::io::ErrorKind::Other))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(props) = self.characteristic.properties() {
            if !props.contains(CharacteristicProperties::WRITE) {
                panic!("Characteristic does not have write");
            }
        }

        Ok(())
    }
}

impl Drop for CharacteristicIO<'_> {
    fn drop(&mut self) {
        if self.rx.is_none() {
            return;
        }

        log::debug!("Dropping CharacteristicIO, removing notify");

        let err = self
            .characteristic
            .write_client_characteristic_configuration_descriptor_async(
                GattClientCharacteristicConfigurationDescriptorValue::None,
            )
            .map(|val| val.get())
            .err();

        if let Some(err) = err {
            log::error!(
                "Unable to remove notify on CharacteristicIO drop: {:?}",
                err
            );
        }
    }
}

/// Discovered BLE descriptor.
pub struct Descriptor {
    inner: GattDescriptor,
}

impl Descriptor {
    fn new(inner: GattDescriptor) -> Self {
        Self { inner }
    }

    /// Read the descriptor from the device.
    pub fn read(&self) -> winrt::Result<Vec<u8>> {
        log::trace!("Reading data from {:?}", &self);

        let value = self.inner.read_value_async()?.get()?.value()?;
        let reader = DataReader::from_buffer(&value)?;
        let mut buf = vec![0u8; value.length()? as usize];
        reader.read_bytes(&mut buf)?;

        Ok(buf)
    }
}

impl std::fmt::Debug for Descriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Descriptor")
            .field("uuid", &self.inner.uuid().unwrap_or_default())
            .finish()
    }
}

/// Utility to allow iteration through advertisements.
///
/// # Example
///
/// A simple way of watching for advertisements is as follows. Note that you
/// will need to filter devices before connecting, as some devices will not
/// allow you to connect.
///
/// ```no_run
/// use wible::AdvertisementWatcher;
///
/// let watcher = AdvertisementWatcher::new().expect("Unable to create AdvertisementWatcher");
///
/// for advertisement in &watcher {
///     println!("Saw advertisement: {:?}", advertisement);
///     println!(
///         "Saw {} services",
///         advertisement
///             .device()
///             .expect("Unable to get device")
///             .services()
///             .map(|services| services.len())
///             .unwrap_or(0)
///     );
/// }
/// ```
pub struct AdvertisementWatcher {
    watcher: BluetoothLEAdvertisementWatcher,
    rx: mpsc::Receiver<Advertisement>,
}

impl AdvertisementWatcher {
    /// Start listening for advertisements.
    pub fn new() -> winrt::Result<Self> {
        let (tx, rx) = mpsc::channel();

        type Handler = TypedEventHandler<
            BluetoothLEAdvertisementWatcher,
            BluetoothLEAdvertisementReceivedEventArgs,
        >;

        let handler = Handler::new(move |_sender, advertisement| {
            log::trace!("Got Bluetooth advertisement: {:?}", advertisement);

            if let Err(err) = tx.send(Advertisement::new(advertisement.to_owned())) {
                log::error!("Unable to send advertisement: {:?}", err);
            }

            Ok(())
        });

        log::debug!("Starting BluetoothLEAdvertisementWatcher");
        let watcher = BluetoothLEAdvertisementWatcher::new()?;
        watcher.received(handler)?;
        watcher.start()?;

        Ok(AdvertisementWatcher { watcher, rx })
    }
}

impl Iterator for &AdvertisementWatcher {
    type Item = Advertisement;

    fn next(&mut self) -> Option<Self::Item> {
        self.rx.recv().ok()
    }
}

impl Drop for AdvertisementWatcher {
    fn drop(&mut self) {
        log::debug!("Stopping BluetoothLEAdvertisementWatcher");
        if let Err(err) = self.watcher.stop() {
            log::error!("Error stopping BluetoothLEAdvertisementWatcher: {:?}", err);
        }
    }
}

/// A MAC address for Bluetooth devices.
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct BluetoothAddress(pub u64);

impl BluetoothAddress {
    /// Get the bytes contained within the address.
    pub fn bytes(&self) -> [u8; 6] {
        let mut buf = [0u8; 6];
        buf.copy_from_slice(&self.0.to_be_bytes()[2..]);

        buf
    }

    /// Get an uppercase and colon separated String representing the address.
    pub fn hex_string(&self) -> String {
        self.bytes()
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(":")
    }
}

/// Error generated when parsing a MAC address from a string.
#[derive(Debug, PartialEq)]
pub enum BluetoothAddressParseError {
    /// MAC address has a number of segments not equal to 6.
    IncorrectSegments,
    /// MAC segment contains a value that is not a valid base-16 number.
    InvalidNumber,
}

impl std::fmt::Display for BluetoothAddressParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BluetoothAddressParseError::IncorrectSegments => {
                write!(f, "MAC address has incorrect number of segments")
            }
            BluetoothAddressParseError::InvalidNumber => {
                write!(f, "MAC segment was not valid base-16 number")
            }
        }
    }
}

impl std::error::Error for BluetoothAddressParseError {}

impl std::str::FromStr for BluetoothAddress {
    type Err = BluetoothAddressParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0u8; 8];

        let parts: Vec<_> = s.split(':').collect();
        if parts.len() != 6 {
            return Err(BluetoothAddressParseError::IncorrectSegments);
        }

        for (index, part) in parts.iter().rev().enumerate() {
            let val = match u8::from_str_radix(part, 16) {
                Ok(val) => val,
                Err(_err) => {
                    return Err(BluetoothAddressParseError::InvalidNumber);
                }
            };

            bytes[index] = val;
        }

        Ok(BluetoothAddress(u64::from_le_bytes(bytes)))
    }
}

impl std::fmt::Display for BluetoothAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.hex_string())
    }
}

impl std::fmt::Debug for BluetoothAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.hex_string();

        f.debug_tuple("BluetoothAddress").field(&s).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{BluetoothAddress, BluetoothAddressParseError};

    #[test]
    fn test_parse_mac() {
        let input = "C8:FD:19:12:7F:CD";
        let expected = Ok(BluetoothAddress(220989372923853));
        let parsed = input.parse();
        assert_eq!(parsed, expected, "unable to parse MAC address");

        let input = "test";
        let expected = Err::<BluetoothAddress, _>(BluetoothAddressParseError::IncorrectSegments);
        let parsed = input.parse();
        assert_eq!(parsed, expected, "invalid mac was accepted");

        let input = "C8:FD:ZZ:12:7F:CD";
        let expected = Err::<BluetoothAddress, _>(BluetoothAddressParseError::InvalidNumber);
        let parsed = input.parse();
        assert_eq!(parsed, expected, "invalid mac was accepted");
    }
}
