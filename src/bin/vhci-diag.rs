// vhci-diag — diagnostic tool to find the VHCI device path on Windows

use std::fs;

fn main() {
    println!("=== VHCI Device Path Diagnostic ===\n");

    let paths = [
        "\\\\.\\USBIP-VHCI",
        "\\\\.\\usbip_vhci",
        "\\\\.\\usbip_vhci0",
        "\\\\.\\usbip_vhci1",
        "\\\\.\\Global\\USBIP-VHCI",
        "\\\\.\\USB#VHCI",
        "\\\\.\\USB#ROOT_HUB30",
    ];

    println!("1. Testing device paths:");
    for p in &paths {
        match fs::OpenOptions::new().read(true).write(true).open(p) {
            Ok(f) => {
                println!("   ✅ FOUND: {}", p);
                drop(f);
            }
            Err(e) => println!("   ❌ {} (kind={:?})", p, e.kind()),
        }
    }

    let pipe_paths = [
        "\\\\.\\pipe\\usbipd",
        "\\\\.\\pipe\\usbipd-win",
        "\\\\.\\pipe\\usbip-vhci",
    ];

    println!("\n2. Named pipes:");
    for p in &pipe_paths {
        match fs::OpenOptions::new().read(true).write(true).open(p) {
            Ok(_) => println!("   ✅ FOUND: {}", p),
            Err(e) => println!("   ❌ {} (kind={:?})", p, e.kind()),
        }
    }

    println!("\n=== Diagnostic Complete ===");
    println!("\nManual checks:");
    println!("  sc query usbipd");
    println!("  usbipd list");
    println!("  Device Manager → Universal Serial Bus devices → look for VHCI");
}