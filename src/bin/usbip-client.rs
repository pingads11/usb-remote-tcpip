// usbip-client — Windows USB/IP client
// Builds on Windows: cargo build --bin usbip-client
// Usage:  usbip-client.exe <MAC_IP> [busid]
//         If busid omitted, discovers devices and lists them.

#[cfg(target_os = "windows")]
mod win {
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    // VHCI driver device path
    const VHCI_DEVICE_PATH: &str = "\\\\.\\usbip_vhci";

    // IOCTL codes from usbip_vhci_api.h (part of usbipd-win)
    // These are the same IOCTL codes usbipd-win uses internally.
    const IOCTL_VHCI_PLUGIN_HARDWARE: u32 = 0x220004; // CTL_CODE(FILE_DEVICE_UNKNOWN, 0x800, METHOD_BUFFERED, FILE_ANY_ACCESS)
    const IOCTL_VHCI_UNPLUG_HARDWARE: u32 = 0x220008;

    pub fn open_vhci() -> io::Result<std::fs::File> {
        let path = to_wstring(VHCI_DEVICE_PATH);
        use std::fs::OpenOptions;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(Path::new(VHCI_DEVICE_PATH))
    }

    pub fn attach_device(handle: &std::fs::File, busid: &str) -> io::Result<()> {
        // Build IOCTL input: usbip_vhci_plugin_hardware struct
        // struct { u32 busid_len; char busid[32]; u32 sockfd; }
        let mut input = vec![0u8; 40]; // 4 + 32 + 4
        let busid_bytes = busid.as_bytes();
        let busid_len = busid_bytes.len().min(31);

        input[0..4].copy_from_slice(&(busid_len as u32).to_le_bytes());
        input[4..4 + busid_len].copy_from_slice(&busid_bytes[..busid_len]);
        // sockfd = 0 (will be set by the VHCI driver from the TCP socket we already connected)

        unsafe {
            device_io_control(handle, IOCTL_VHCI_PLUGIN_HARDWARE, &input, &mut [])?;
        }
        Ok(())
    }

    pub fn detach_device(handle: &std::fs::File, busid: &str) -> io::Result<()> {
        let mut input = vec![0u8; 36];
        let busid_bytes = busid.as_bytes();
        let busid_len = busid_bytes.len().min(31);
        input[0..4].copy_from_slice(&(busid_len as u32).to_le_bytes());
        input[4..4 + busid_len].copy_from_slice(&busid_bytes[..busid_len]);

        unsafe {
            device_io_control(handle, IOCTL_VHCI_UNPLUG_HARDWARE, &input, &mut [])?;
        }
        Ok(())
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn DeviceIoControl(
            hDevice: *mut std::ffi::c_void,
            dwIoControlCode: u32,
            lpInBuffer: *const std::ffi::c_void,
            nInBufferSize: u32,
            lpOutBuffer: *mut std::ffi::c_void,
            nOutBufferSize: u32,
            lpBytesReturned: *mut u32,
            lpOverlapped: *mut std::ffi::c_void,
        ) -> i32;
    }

    unsafe fn device_io_control(
        file: &std::fs::File,
        code: u32,
        input: &[u8],
        output: &mut [u8],
    ) -> io::Result<u32> {
        let mut bytes_returned: u32 = 0;
        let ret = DeviceIoControl(
            file.as_raw_handle() as *mut std::ffi::c_void,
            code,
            input.as_ptr() as *const std::ffi::c_void,
            input.len() as u32,
            output.as_mut_ptr() as *mut std::ffi::c_void,
            output.len() as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
        );
        if ret == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(bytes_returned)
    }

    fn to_wstring(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(target_os = "windows"))]
mod win {
    pub fn open_vhci() -> std::io::Result<std::fs::File> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "VHCI driver is only available on Windows. Build and run this on Windows.",
        ))
    }
}

use std::io::{Read, Write};
use std::net::{SocketAddrV4, TcpStream};

// Protocol constants (shared with server)
const OP_REQ_DEVLIST: u16 = 0x8005;
const OP_REP_DEVLIST: u16 = 0x0005;
const OP_REQ_IMPORT: u16 = 0x8003;
const OP_REP_IMPORT: u16 = 0x0003;
const USBIP_VERSION: u16 = 0x0111;
const USBIP_BUSID_SIZE: usize = 32;
const DEVICE_WIRE_SIZE: usize = 256 + 32 + 12 + 6 + 6;

const ANDROID_VIDS: &[u16] = &[
    0x18d1, 0x04e8, 0x0b05, 0x12d1, 0x2717, 0x1004, 0x22b8, 0x0fce, 0x2a96,
];

/// Connect to server, request device list, return devices.
fn list_devices(addr: SocketAddrV4) -> Result<Vec<DeviceInfo>, String> {
    let mut stream =
        TcpStream::connect(addr).map_err(|e| format!("Failed to connect to {}: {}", addr, e))?;

    // Send OP_REQ_DEVLIST
    let mut request = Vec::with_capacity(8);
    request.extend_from_slice(&USBIP_VERSION.to_be_bytes());
    request.extend_from_slice(&OP_REQ_DEVLIST.to_be_bytes());
    request.extend_from_slice(&0u32.to_be_bytes());
    stream.write_all(&request).map_err(|e| format!("Send error: {}", e))?;

    // Read response header
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).map_err(|e| format!("Read header: {}", e))?;

    let code = u16::from_be_bytes([header[2], header[3]]);
    let status = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

    if code != OP_REP_DEVLIST {
        return Err(format!("Expected OP_REP_DEVLIST ({:#06x}), got {:#06x}", OP_REP_DEVLIST, code));
    }
    if status != 0 {
        return Err(format!("Server error status: {}", status));
    }

    // Read device count
    let mut ndev_bytes = [0u8; 4];
    stream.read_exact(&mut ndev_bytes).map_err(|e| format!("Read count: {}", e))?;
    let ndev = u32::from_be_bytes(ndev_bytes);

    let mut devices = Vec::new();
    for _ in 0..ndev {
        let mut dev_bytes = vec![0u8; DEVICE_WIRE_SIZE];
        stream.read_exact(&mut dev_bytes).map_err(|e| format!("Read device: {}", e))?;

        let busid_end = dev_bytes[256..288]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(USBIP_BUSID_SIZE);
        let busid = String::from_utf8_lossy(&dev_bytes[256..256 + busid_end]).to_string();

        let vid = u16::from_be_bytes([dev_bytes[300], dev_bytes[301]]);
        let pid = u16::from_be_bytes([dev_bytes[302], dev_bytes[303]]);

        let is_android = ANDROID_VIDS.contains(&vid);

        devices.push(DeviceInfo { busid, vid, pid, is_android });
    }

    Ok(devices)
}

/// Request import of a specific device from the server.
fn import_device(addr: SocketAddrV4, busid: &str) -> Result<DeviceInfo, String> {
    let mut stream =
        TcpStream::connect(addr).map_err(|e| format!("Failed to connect: {}", e))?;

    // Send OP_REQ_IMPORT
    let mut request = Vec::with_capacity(8 + USBIP_BUSID_SIZE);
    request.extend_from_slice(&USBIP_VERSION.to_be_bytes());
    request.extend_from_slice(&OP_REQ_IMPORT.to_be_bytes());
    request.extend_from_slice(&0u32.to_be_bytes());

    // Busid padded to 32 bytes
    let mut busid_buf = [0u8; USBIP_BUSID_SIZE];
    let bytes = busid.as_bytes();
    let len = bytes.len().min(USBIP_BUSID_SIZE);
    busid_buf[..len].copy_from_slice(&bytes[..len]);
    request.extend_from_slice(&busid_buf);

    stream.write_all(&request).map_err(|e| format!("Send error: {}", e))?;

    // Read response header
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).map_err(|e| format!("Read header: {}", e))?;

    let code = u16::from_be_bytes([header[2], header[3]]);
    let status = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

    if code != OP_REP_IMPORT {
        return Err(format!("Expected OP_REP_IMPORT ({:#06x}), got {:#06x}", OP_REP_IMPORT, code));
    }
    if status != 0 {
        return Err(format!("Server rejected import: status {}", status));
    }

    // Read device descriptor
    let mut dev_bytes = vec![0u8; DEVICE_WIRE_SIZE];
    stream.read_exact(&mut dev_bytes).map_err(|e| format!("Read device: {}", e))?;

    let vid = u16::from_be_bytes([dev_bytes[300], dev_bytes[301]]);
    let pid = u16::from_be_bytes([dev_bytes[302], dev_bytes[303]]);
    let is_android = ANDROID_VIDS.contains(&vid);

    Ok(DeviceInfo { busid: busid.to_string(), vid, pid, is_android })
}

struct DeviceInfo {
    busid: String,
    vid: u16,
    pid: u16,
    is_android: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: usbip-client <MAC_IP> [busid]");
        eprintln!("  If busid omitted, lists available devices from the Mac server.");
        eprintln!("  If busid provided, attaches that device via VHCI driver.");
        eprintln!("\nExample: usbip-client 10.6.0.2");
        eprintln!("         usbip-client 10.6.0.2 1-2");
        std::process::exit(1);
    }

    let ip: std::net::Ipv4Addr = args[1].parse().unwrap_or_else(|e| {
        eprintln!("Invalid IP address '{}': {}", args[1], e);
        std::process::exit(1);
    });
    let addr = SocketAddrV4::new(ip, 3240);

    if args.len() == 2 {
        // List mode
        println!("🔍 Querying {} for USB devices...", addr);
        match list_devices(addr) {
            Ok(devices) => {
                if devices.is_empty() {
                    println!("No USB devices found on the server.");
                } else {
                    println!("Devices on Mac server:");
                    for d in &devices {
                        let tag = if d.is_android { "📱 ANDROID" } else { "" };
                        println!(
                            "  busid={:<12} VID:PID={:04x}:{:04x}  {}",
                            d.busid, d.vid, d.pid, tag
                        );
                    }
                    println!(
                        "\nTo attach a device: usbip-client {} <busid>",
                        args[1]
                    );
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        // Attach mode
        let busid = &args[2];
        println!("📡 Connecting to {} and importing busid={}...", addr, busid);

        match import_device(addr, busid) {
            Ok(dev) => {
                println!(
                    "✅ Server acknowledged device {:04x}:{:04x} {}",
                    dev.vid, dev.pid,
                    if dev.is_android { "(Android)" } else { "" }
                );

                // Platform-specific VHCI driver attachment
                #[cfg(target_os = "windows")]
                {
                    match win::open_vhci() {
                        Ok(vhci) => {
                            match win::attach_device(&vhci, &dev.busid) {
                                Ok(()) => {
                                    println!("✅ Device attached via VHCI driver!");
                                    println!("   Check Device Manager — the device should appear.");
                                    println!("   For Android: run 'adb devices' to verify.");
                                    println!("\n   Press Ctrl+C to detach and exit.");
                                    loop {
                                        std::thread::sleep(std::time::Duration::from_secs(1));
                                    }
                                }
                                Err(e) => {
                                    eprintln!("❌ VHCI attach failed: {}", e);
                                    eprintln!("   Is usbipd-win installed? The VHCI driver must be present.");
                                    std::process::exit(1);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ Cannot open VHCI driver: {}", e);
                            eprintln!("   Install usbipd-win first (the driver is needed):");
                            eprintln!("   winget install dorssel.usbipd-win");
                            std::process::exit(1);
                        }
                    }
                }

                #[cfg(not(target_os = "windows"))]
                {
                    println!("⚠️  VHCI driver only available on Windows.");
                    println!("   The protocol handshake with the server succeeded.");
                    println!("   To attach the device, run this binary on Windows.");
                }
            }
            Err(e) => {
                eprintln!("❌ Import failed: {}", e);
                std::process::exit(1);
            }
        }
    }
}