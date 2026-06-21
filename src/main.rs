// USB/IP Server — runs on Mac, exposes local USB devices over TCP.
#![cfg(not(target_os = "windows"))]

mod protocol;

use log::{error, info, warn};
use protocol::{
    build_op_header, UsbIpDevice, USBIP_BUSID_SIZE, USBIP_PATH_SIZE, ANDROID_VIDS,
    OP_REQ_DEVLIST, OP_REP_DEVLIST, OP_REQ_IMPORT, OP_REP_IMPORT,
};
use rusb::UsbContext;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn scan_usb_devices() -> anyhow::Result<Vec<UsbIpDevice>> {
    let ctx = rusb::Context::new()?;
    let mut devices = Vec::new();
    for usb_dev in ctx.devices()?.iter() {
        let desc = match usb_dev.device_descriptor() { Ok(d) => d, Err(_) => continue };
        let b_configuration_value = match usb_dev.open() { Ok(h) => h.active_configuration().unwrap_or(1), Err(_) => 1 };
        let busnum = usb_dev.bus_number() as u32;
        let devnum = usb_dev.address() as u32;
        let busid_str = format!("{}-{}", busnum, devnum);
        let path_str = format!("/sys/devices/pci0000:00/0000:00:00.0/usb{}/{}", busnum, devnum);
        let mut busid = [0u8; USBIP_BUSID_SIZE];
        let mut path = [0u8; USBIP_PATH_SIZE];
        busid[..busid_str.len().min(USBIP_BUSID_SIZE)].copy_from_slice(&busid_str.as_bytes()[..busid_str.len().min(USBIP_BUSID_SIZE)]);
        path[..path_str.len().min(USBIP_PATH_SIZE)].copy_from_slice(&path_str.as_bytes()[..path_str.len().min(USBIP_PATH_SIZE)]);
        let vid = desc.vendor_id(); let pid = desc.product_id();
        if ANDROID_VIDS.contains(&vid) {
            info!("📱 Android: {:04x}:{:04x} (bus={}, dev={}, busid={})", vid, pid, busnum, devnum, busid_str);
        }
        let v = desc.device_version();
        let bcd: u16 = ((v.0 as u16) << 8) | ((v.1 as u16) << 4) | (v.2 as u16);
        devices.push(UsbIpDevice {
            path, busid, busnum, devnum, speed: usb_dev.speed() as u32,
            id_vendor: vid, id_product: pid, bcd_device: bcd,
            b_device_class: desc.class_code(), b_device_sub_class: desc.sub_class_code(),
            b_device_protocol: desc.protocol_code(), b_configuration_value,
            b_num_configurations: desc.num_configurations(), b_num_interfaces: 0,
        });
    }
    Ok(devices)
}

fn open_device(busid: &str) -> Option<rusb::DeviceHandle<rusb::Context>> {
    let ctx = rusb::Context::new().ok()?;
    for usb_dev in ctx.devices().ok()?.iter() {
        let this_busid = format!("{}-{}", usb_dev.bus_number(), usb_dev.address());
        if this_busid == busid {
            let handle = usb_dev.open().ok()?;
            let desc = usb_dev.device_descriptor().ok()?;
            for config_idx in 0..desc.num_configurations() {
                if let Ok(cfg) = usb_dev.config_descriptor(config_idx) {
                    for iface in cfg.interfaces() {
                        let num = iface.number();
                        let _ = handle.detach_kernel_driver(num);
                        let _ = handle.claim_interface(num);
                    }
                }
            }
            return Some(handle);
        }
    }
    None
}

async fn handle_devlist(socket: &mut tokio::net::TcpStream) -> anyhow::Result<()> {
    info!("→ OP_REQ_DEVLIST");
    let devices = scan_usb_devices()?;
    info!("  Found {} USB device(s)", devices.len());
    socket.write_all(&build_op_header(OP_REP_DEVLIST, 0)).await?;
    socket.write_all(&(devices.len() as u32).to_be_bytes()).await?;
    for dev in &devices { socket.write_all(&dev.to_wire()).await?; }
    info!("  ← OP_REP_DEVLIST sent ({} devices)", devices.len());
    Ok(())
}

async fn handle_import(socket: &mut tokio::net::TcpStream) -> anyhow::Result<String> {
    let mut busid_buf = [0u8; USBIP_BUSID_SIZE];
    socket.read_exact(&mut busid_buf).await?;
    let busid_str = String::from_utf8_lossy(&busid_buf).trim_end_matches('\0').to_string();
    info!("→ OP_REQ_IMPORT for busid: \"{}\"", busid_str);
    let devices = scan_usb_devices()?;
    let matching = devices.iter().find(|d: &&UsbIpDevice| d.busid_str() == busid_str);
    match matching {
        Some(dev) => {
            info!("  ✅ Device found — sending OP_REP_IMPORT (SUCCESS)");
            socket.write_all(&build_op_header(OP_REP_IMPORT, 0)).await?;
            socket.write_all(&dev.to_wire()).await?;
            info!("  ← OP_REP_IMPORT sent");
            Ok(busid_str)
        }
        None => {
            warn!("  ❌ Device \"{}\" not found", busid_str);
            socket.write_all(&build_op_header(OP_REP_IMPORT, 1)).await?;
            Ok(String::new())
        }
    }
}

fn get_u32_be(bytes: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([bytes[off], bytes[off+1], bytes[off+2], bytes[off+3]])
}

/// Handle USBIP_CMD_SUBMIT.
/// The 8-byte OP header consumed command(4) + seqnum(4) from the full 48-byte usbip_header.
/// The remaining 40-byte payload starts at devid.
///
/// 40-byte payload layout:
///   0-3:   devid
///   4-7:   direction  (0=OUT, 1=IN)
///   8-11:  ep (endpoint number, e.g. 0x81 for EP1 IN)
///   12-15: transfer_flags
///   16-19: transfer_buffer_length
///   20-23: start_frame
///   24-27: number_of_packets
///   28-31: interval
///   32-39: setup[8]
async fn handle_urb_submit(
    socket: &mut tokio::net::TcpStream,
    seqnum: u32,
    device: &mut Option<rusb::DeviceHandle<rusb::Context>>,
) -> anyhow::Result<()> {
    let mut payload = [0u8; 40];
    socket.read_exact(&mut payload).await?;

    let _devid = get_u32_be(&payload, 0);
    let direction = get_u32_be(&payload, 4);
    let ep_raw = get_u32_be(&payload, 8);
    let transfer_flags = get_u32_be(&payload, 12);
    let transfer_len = get_u32_be(&payload, 16);
    let setup_bytes: &[u8; 8] = payload[32..40].try_into().unwrap();
    let ep_num = (ep_raw & 0x7F) as u8; // strip direction bit
    let is_in = direction == 1;
    let is_setup = transfer_flags & 0x01 != 0;

    info!("  URB seq={} dir={} ep_raw={:#06x} ep_num={:#04x} flags={:#x} len={} in={} setup={}",
        seqnum, direction, ep_raw, ep_num, transfer_flags, transfer_len, is_in, is_setup);

    let actual_len: i32;
    let status: i32;

    if let Some(ref handle) = device {
        if is_setup {
            let req_type = setup_bytes[0];
            let request = setup_bytes[1];
            let value = u16::from_le_bytes([setup_bytes[2], setup_bytes[3]]);
            let index = u16::from_le_bytes([setup_bytes[4], setup_bytes[5]]);
            let length = u16::from_le_bytes([setup_bytes[6], setup_bytes[7]]);
            let mut buf = vec![0u8; length as usize];
            let timeout = std::time::Duration::from_secs(5);
            match handle.read_control(req_type, request, value, index, &mut buf, timeout) {
                Ok(n) => { actual_len = n as i32; status = 0; }
                Err(e) => { actual_len = 0; status = -(e as i32); warn!("  ctrl fail: {:?}", e); }
            }
        } else if is_in {
            let len = if transfer_len > 0 { transfer_len } else { 512 };
            let timeout = std::time::Duration::from_secs(5);
            let mut buf = vec![0u8; len as usize];
            match handle.read_bulk(ep_num, &mut buf, timeout) {
                Ok(n) => { actual_len = n as i32; status = 0; }
                Err(e) => { actual_len = 0; status = -(e as i32); warn!("  bulk IN fail ep={:#04x}: {:?}", ep_num, e); }
            }
        } else {
            let mut out_data = vec![0u8; transfer_len as usize];
            if transfer_len > 0 { socket.read_exact(&mut out_data).await?; }
            let timeout = std::time::Duration::from_secs(5);
            match handle.write_bulk(ep_num, &out_data, timeout) {
                Ok(n) => { actual_len = n as i32; status = 0; }
                Err(e) => { actual_len = 0; status = -(e as i32); warn!("  bulk OUT fail ep={:#04x}: {:?}", ep_num, e); }
            }
        }
    } else {
        actual_len = 0;
        status = 0;
    }

    // Build USBIP_RET_SUBMIT (48 bytes total)
    // The first 8 bytes are the "OP header" (version/code/status), matching what client expects
    let mut reply = vec![0u8; 48];
    reply[0..2].copy_from_slice(&0x0111u16.to_be_bytes());               // version
    reply[2..4].copy_from_slice(&0x0003u16.to_be_bytes());               // code=RET_SUBMIT
    reply[4..8].copy_from_slice(&(status as u32).to_be_bytes());         // status
    reply[8..12].copy_from_slice(&seqnum.to_be_bytes());                 // seqnum
    reply[12..16].copy_from_slice(&[0u8; 4]);                             // devid=0
    reply[16..20].copy_from_slice(&direction.to_be_bytes());             // direction
    reply[20..24].copy_from_slice(&ep_raw.to_be_bytes());                // ep (raw, with dir bit)
    reply[24..28].copy_from_slice(&(actual_len as u32).to_be_bytes());   // actual_length
    reply[28..40].fill(0);                                                // start_frame, num_packets, error_count
    reply[40..48].copy_from_slice(setup_bytes);                           // setup mirror

    socket.write_all(&reply).await?;
    Ok(())
}

async fn handle_client(mut socket: tokio::net::TcpStream, peer: std::net::SocketAddr) {
    info!("=== Client connected from {} ===", peer);

    let result: anyhow::Result<()> = async {
        let mut device: Option<rusb::DeviceHandle<rusb::Context>> = None;

        loop {
            let mut header = [0u8; 8];
            match socket.read_exact(&mut header).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    info!("Client {} disconnected (EOF)", peer);
                    return Ok(());
                }
                Err(e) => return Err(anyhow::anyhow!("Read error: {}", e)),
            }

            let version = u16::from_be_bytes([header[0], header[1]]);
            let code = u16::from_be_bytes([header[2], header[3]]);
            // For CMD_SUBMIT/CMD_UNLINK, bytes 4-7 are actually seqnum (not status)
            let extra = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

            info!("[{}] PDU: version={:#06x} code={:#06x} extra={}", peer, version, code, extra);

            const CMD_SUBMIT: u16 = 0x0001;
            const CMD_UNLINK: u16 = 0x0002;

            if code == OP_REQ_DEVLIST {
                handle_devlist(&mut socket).await?;
            } else if code == OP_REQ_IMPORT {
                let busid = handle_import(&mut socket).await?;
                if !busid.is_empty() {
                    device = open_device(&busid);
                    if device.is_some() { info!("  🔌 Device {} opened", busid); }
                    else { warn!("  ⚠️  Could not open device {}", busid); }
                }
            } else if code == CMD_SUBMIT {
                handle_urb_submit(&mut socket, extra, &mut device).await?;
            } else if code == CMD_UNLINK {
                let mut discard = [0u8; 40];
                socket.read_exact(&mut discard).await?;
            } else {
                warn!("  Unknown code {:#06x}", code);
            }
        }
    }.await;

    if let Err(e) = result { error!("Error with client {}: {}", peer, e); }
    info!("=== Client {} disconnected ===", peer);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder().filter_level(log::LevelFilter::Info).format_timestamp_micros().init();
    info!("🚀 USB/IP Server v{} starting on port 3240", env!("CARGO_PKG_VERSION"));
    info!("  Windows WSL2: sudo usbip attach -r <IP> -b <BUSID>");
    let addr: std::net::SocketAddr = "0.0.0.0:3240".parse()?;
    let listener = TcpListener::bind(addr).await?;
    info!("✅ Listening on {} — waiting for connections...", addr);
    loop {
        let (socket, peer) = listener.accept().await?;
        tokio::spawn(handle_client(socket, peer));
    }
}