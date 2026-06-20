// usbip-probe — quick USB/IP protocol tester (no libusb dependency)
// Usage: cargo run --bin usbip-probe [host:port]   (default: 127.0.0.1:3240)

use std::io::{Read, Write};
use std::net::TcpStream;

// Minimal protocol constants (no external crate needed)
const OP_REQ_DEVLIST: u16 = 0x8005;
const OP_REP_DEVLIST: u16 = 0x0005;
const USBIP_VERSION: u16 = 0x0111;
const USBIP_BUSID_SIZE: usize = 32;
const DEVICE_WIRE_SIZE: usize = 256 + 32 + 12 + 6 + 6; // 312

const ANDROID_VIDS: &[u16] = &[
    0x18d1, 0x04e8, 0x0b05, 0x12d1, 0x2717, 0x1004, 0x22b8, 0x0fce, 0x2a96,
];

fn main() {
    let target = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:3240".to_string());

    println!("🔍 Connecting to USB/IP server at {}...", target);
    let mut stream = TcpStream::connect(&target).expect("Failed to connect");
    println!("✅ Connected!");

    // Send OP_REQ_DEVLIST
    let mut request = Vec::with_capacity(8);
    request.extend_from_slice(&USBIP_VERSION.to_be_bytes());
    request.extend_from_slice(&OP_REQ_DEVLIST.to_be_bytes());
    request.extend_from_slice(&0u32.to_be_bytes());
    stream.write_all(&request).expect("Failed to send OP_REQ_DEVLIST");
    println!("→ Sent OP_REQ_DEVLIST");

    // Read response header (8 bytes)
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).expect("Failed to read reply header");

    let resp_version = u16::from_be_bytes([header[0], header[1]]);
    let resp_code = u16::from_be_bytes([header[2], header[3]]);
    let resp_status = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

    println!(
        "← Reply: version={:#06x} code={:#06x} status={}",
        resp_version, resp_code, resp_status
    );

    if resp_code != OP_REP_DEVLIST {
        eprintln!("❌ Unexpected response code: {:#06x}", resp_code);
        std::process::exit(1);
    }

    if resp_status != 0 {
        eprintln!("❌ Server returned error status: {}", resp_status);
        std::process::exit(1);
    }

    // Read number of devices (u32 BE)
    let mut ndev_bytes = [0u8; 4];
    stream.read_exact(&mut ndev_bytes).expect("Failed to read device count");
    let ndev = u32::from_be_bytes(ndev_bytes);
    println!("  Devices reported: {}", ndev);

    if ndev == 0 {
        println!("  (No USB devices found on server)");
        return;
    }

    for i in 0..ndev {
        let mut dev_bytes = vec![0u8; DEVICE_WIRE_SIZE];
        stream.read_exact(&mut dev_bytes).expect("Failed to read device descriptor");

        let busid_end = dev_bytes[256..288]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(USBIP_BUSID_SIZE);
        let busid = String::from_utf8_lossy(&dev_bytes[256..256 + busid_end]);

        let vid = u16::from_be_bytes([dev_bytes[300], dev_bytes[301]]);
        let pid = u16::from_be_bytes([dev_bytes[302], dev_bytes[303]]);
        let class = dev_bytes[306];
        let subclass = dev_bytes[307];

        let tag = if ANDROID_VIDS.contains(&vid) { "📱 ANDROID" } else { "" };

        println!(
            "  [{}/{}] busid={:<12} VID:PID={:04x}:{:04x}  class={:02x}:{:02x}  {}",
            i + 1, ndev, busid, vid, pid, class, subclass, tag
        );
    }

    println!("✅ Protocol validated — {} device(s) enumerated successfully", ndev);
}