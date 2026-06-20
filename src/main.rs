// USB/IP Server — runs on Mac, exposes local USB devices over TCP.

#![cfg(not(target_os = "windows"))]

mod protocol;

use log::{error, info, warn};
use protocol::{
    build_op_header, UsbIpDevice, USBIP_BUSID_SIZE, USBIP_PATH_SIZE, ANDROID_VIDS,
};
use protocol::{OP_REQ_DEVLIST, OP_REP_DEVLIST, OP_REQ_IMPORT, OP_REP_IMPORT};
use protocol::{USBIP_CMD_SUBMIT, USBIP_CMD_UNLINK};
use rusb::UsbContext;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn scan_usb_devices() -> anyhow::Result<Vec<UsbIpDevice>> {
    let ctx = rusb::Context::new()?;
    let mut devices = Vec::new();

    for usb_dev in ctx.devices()?.iter() {
        let desc = match usb_dev.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        let b_configuration_value = match usb_dev.open() {
            Ok(handle) => handle.active_configuration().unwrap_or(1),
            Err(_) => 1,
        };

        let busnum = usb_dev.bus_number() as u32;
        let devnum = usb_dev.address() as u32;

        let busid_str = format!("{}-{}", busnum, devnum);
        let path_str = format!("/sys/devices/pci0000:00/0000:00:00.0/usb{}/{}", busnum, devnum);

        let mut busid = [0u8; USBIP_BUSID_SIZE];
        let busid_bytes = busid_str.as_bytes();
        let len = busid_bytes.len().min(USBIP_BUSID_SIZE);
        busid[..len].copy_from_slice(&busid_bytes[..len]);

        let mut path = [0u8; USBIP_PATH_SIZE];
        let path_bytes = path_str.as_bytes();
        let len = path_bytes.len().min(USBIP_PATH_SIZE);
        path[..len].copy_from_slice(&path_bytes[..len]);

        let vid = desc.vendor_id();
        let pid = desc.product_id();

        if ANDROID_VIDS.contains(&vid) {
            info!("📱 Android: {:04x}:{:04x} (bus={}, dev={}, busid={})", vid, pid, busnum, devnum, busid_str);
        } else {
            info!("USB: {:04x}:{:04x} class={:02x} bus={} dev={} busid={}", vid, pid, desc.class_code(), busnum, devnum, busid_str);
        }

        let v = desc.device_version();
        let bcd: u16 = ((v.0 as u16) << 8) | ((v.1 as u16) << 4) | (v.2 as u16);

        devices.push(UsbIpDevice {
            path,
            busid,
            busnum,
            devnum,
            speed: usb_dev.speed() as u32,
            id_vendor: vid,
            id_product: pid,
            bcd_device: bcd,
            b_device_class: desc.class_code(),
            b_device_sub_class: desc.sub_class_code(),
            b_device_protocol: desc.protocol_code(),
            b_configuration_value,
            b_num_configurations: desc.num_configurations(),
            b_num_interfaces: 0,
        });
    }

    Ok(devices)
}

async fn handle_devlist(socket: &mut tokio::net::TcpStream) -> anyhow::Result<()> {
    info!("→ OP_REQ_DEVLIST received");
    let devices = scan_usb_devices()?;
    info!("  Found {} USB device(s)", devices.len());

    socket.write_all(&build_op_header(OP_REP_DEVLIST, 0)).await?;
    socket.write_all(&(devices.len() as u32).to_be_bytes()).await?;

    for dev in &devices {
        socket.write_all(&dev.to_wire()).await?;
    }

    info!("  ← OP_REP_DEVLIST sent ({} devices)", devices.len());
    Ok(())
}

async fn handle_import(socket: &mut tokio::net::TcpStream) -> anyhow::Result<()> {
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
        }
        None => {
            warn!("  ❌ Device \"{}\" not found", busid_str);
            socket.write_all(&build_op_header(OP_REP_IMPORT, 1)).await?;
        }
    }

    Ok(())
}

async fn handle_client(mut socket: tokio::net::TcpStream, peer: std::net::SocketAddr) {
    info!("=== Client connected from {} ===", peer);

    let result: anyhow::Result<()> = async {
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
            let _status = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

            info!("[{}] PDU: version={:#06x} code={:#06x}", peer, version, code);

            if code == OP_REQ_DEVLIST {
                handle_devlist(&mut socket).await?;
            } else if code == OP_REQ_IMPORT {
                handle_import(&mut socket).await?;
            } else if code == USBIP_CMD_SUBMIT || code == USBIP_CMD_UNLINK {
                let mut discard = [0u8; 40];
                socket.read_exact(&mut discard).await?;
            } else {
                warn!("  Unknown code {:#06x} from {}", code, peer);
            }
        }
    }.await;

    if let Err(e) = result {
        error!("Error with client {}: {}", peer, e);
    }
    info!("=== Client {} disconnected ===", peer);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_micros()
        .init();

    info!("🚀 USB/IP Server v{} starting on port 3240", env!("CARGO_PKG_VERSION"));
    info!("  To find your Mac's LAN IP: ifconfig | grep 'inet '");
    info!("  Windows client: usbip-client.exe <MAC_IP>");

    let addr: std::net::SocketAddr = "0.0.0.0:3240".parse()?;
    let listener = TcpListener::bind(addr).await?;
    info!("✅ Listening on {} — waiting for connections...", addr);

    loop {
        let (socket, peer) = listener.accept().await?;
        tokio::spawn(handle_client(socket, peer));
    }
}