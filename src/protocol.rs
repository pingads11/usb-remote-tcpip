// Shared USB/IP protocol types and constants — used by both server and client.

// USB/IP protocol constants (from Linux kernel include/uapi/linux/usbip.h)
pub const USBIP_VERSION: u16 = 0x0111;
pub const OP_REQ_DEVLIST: u16 = 0x8005;
pub const OP_REP_DEVLIST: u16 = 0x0005;
pub const OP_REQ_IMPORT: u16 = 0x8003;
pub const OP_REP_IMPORT: u16 = 0x0003;
pub const USBIP_CMD_SUBMIT: u16 = 0x0001;
pub const USBIP_CMD_UNLINK: u16 = 0x0002;

pub const USBIP_BUSID_SIZE: usize = 32;
pub const USBIP_PATH_SIZE: usize = 256;

/// Represents a USB device as defined by the USB/IP protocol's usbip_usb_device struct.
/// Wire format: path[256] + busid[32] + busnum:u32 + devnum:u32 + speed:u32 +
///              idVendor:u16 + idProduct:u16 + bcdDevice:u16 +
///              bDeviceClass:u8 + bDeviceSubClass:u8 + bDeviceProtocol:u8 +
///              bConfigurationValue:u8 + bNumConfigurations:u8 + bNumInterfaces:u8
/// Total: 256 + 32 + 12 + 6 + 6 = 312 bytes
pub const DEVICE_WIRE_SIZE: usize = USBIP_PATH_SIZE + USBIP_BUSID_SIZE + 12 + 6 + 6;

#[derive(Debug, Clone)]
pub struct UsbIpDevice {
    pub path: [u8; USBIP_PATH_SIZE],
    pub busid: [u8; USBIP_BUSID_SIZE],
    pub busnum: u32,
    pub devnum: u32,
    pub speed: u32,
    pub id_vendor: u16,
    pub id_product: u16,
    pub bcd_device: u16,
    pub b_device_class: u8,
    pub b_device_sub_class: u8,
    pub b_device_protocol: u8,
    pub b_configuration_value: u8,
    pub b_num_configurations: u8,
    pub b_num_interfaces: u8,
}

impl UsbIpDevice {
    /// Serialize to network byte order bytes.
    pub fn to_wire(&self) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::with_capacity(DEVICE_WIRE_SIZE);
        buf.extend_from_slice(&self.path);
        buf.extend_from_slice(&self.busid);
        buf.extend_from_slice(&self.busnum.to_be_bytes());
        buf.extend_from_slice(&self.devnum.to_be_bytes());
        buf.extend_from_slice(&self.speed.to_be_bytes());
        buf.extend_from_slice(&self.id_vendor.to_be_bytes());
        buf.extend_from_slice(&self.id_product.to_be_bytes());
        buf.extend_from_slice(&self.bcd_device.to_be_bytes());
        buf.push(self.b_device_class);
        buf.push(self.b_device_sub_class);
        buf.push(self.b_device_protocol);
        buf.push(self.b_configuration_value);
        buf.push(self.b_num_configurations);
        buf.push(self.b_num_interfaces);
        buf
    }

    /// Deserialize from network byte order bytes.
    pub fn from_wire(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < DEVICE_WIRE_SIZE {
            return None;
        }
        let mut path = [0u8; USBIP_PATH_SIZE];
        let mut busid = [0u8; USBIP_BUSID_SIZE];
        path.copy_from_slice(&bytes[0..USBIP_PATH_SIZE]);
        busid.copy_from_slice(&bytes[USBIP_PATH_SIZE..USBIP_PATH_SIZE + USBIP_BUSID_SIZE]);

        let off = USBIP_PATH_SIZE + USBIP_BUSID_SIZE;
        let busnum = u32::from_be_bytes([bytes[off], bytes[off+1], bytes[off+2], bytes[off+3]]);
        let devnum = u32::from_be_bytes([bytes[off+4], bytes[off+5], bytes[off+6], bytes[off+7]]);
        let speed = u32::from_be_bytes([bytes[off+8], bytes[off+9], bytes[off+10], bytes[off+11]]);
        let id_vendor = u16::from_be_bytes([bytes[off+12], bytes[off+13]]);
        let id_product = u16::from_be_bytes([bytes[off+14], bytes[off+15]]);
        let bcd_device = u16::from_be_bytes([bytes[off+16], bytes[off+17]]);

        let b_device_class = bytes[off+18];
        let b_device_sub_class = bytes[off+19];
        let b_device_protocol = bytes[off+20];
        let b_configuration_value = bytes[off+21];
        let b_num_configurations = bytes[off+22];
        let b_num_interfaces = bytes[off+23];

        Some(UsbIpDevice {
            path,
            busid,
            busnum,
            devnum,
            speed,
            id_vendor,
            id_product,
            bcd_device,
            b_device_class,
            b_device_sub_class,
            b_device_protocol,
            b_configuration_value,
            b_num_configurations,
            b_num_interfaces,
        })
    }

    pub fn busid_str(&self) -> String {
        String::from_utf8_lossy(&self.busid)
            .trim_end_matches('\0')
            .to_string()
    }
}

/// Build an 8-byte USB/IP OP header: u16 version(BE) + u16 code(BE) + u32 status(BE).
pub fn build_op_header(code: u16, status: u32) -> [u8; 8] {
    let mut h = [0u8; 8];
    h[0..2].copy_from_slice(&USBIP_VERSION.to_be_bytes());
    h[2..4].copy_from_slice(&code.to_be_bytes());
    h[4..8].copy_from_slice(&status.to_be_bytes());
    h
}

/// Known Android vendor IDs for device detection.
pub const ANDROID_VIDS: &[u16] = &[
    0x18d1, 0x04e8, 0x0b05, 0x12d1, 0x2717, 0x1004, 0x22b8, 0x0fce, 0x2a96,
];