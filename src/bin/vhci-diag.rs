// vhci-diag — diagnostic tool to find the VHCI device path on Windows
// Run on Windows: cargo run --bin vhci-diag

use std::fs;

fn main() {
    println!("=== VHCI Device Path Diagnostic ===\n");

    // 1. Try common device paths
    let paths = [
        "\\\\.\\USBIP-VHCI",
        "\\\\.\\usbip_vhci",
        "\\\\.\\usbip_vhci0",
        "\\\\.\\usbip_vhci1",
        "\\\\.\\Global\\USBIP-VHCI",
        "\\\\.\\USB#VHCI",
        "\\\\.\\USB#ROOT_HUB30",
    ];

    println!("1. Testing common device paths:");
    for p in &paths {
        match fs::OpenOptions::new().read(true).write(true).open(p) {
            Ok(f) => {
                println!("   ✅ FOUND: {}", p);
                drop(f);
            }
            Err(e) => {
                let kind = e.kind();
                println!("   ❌ {} (kind={:?}, msg={})", p, kind, e);
            }
        }
    }

    // 2. Try named pipes
    println!("\n2. Named pipes:");
    let pipe_paths = [
        "\\\\.\\pipe\\usbipd",
        "\\\\.\\pipe\\usbipd-win",
        "\\\\.\\pipe\\usbip-vhci",
    ];
    for p in &pipe_paths {
        match fs::OpenOptions::new().read(true).write(true).open(p) {
            Ok(_) => println!("   ✅ FOUND: {}", p),
            Err(e) => println!("   ❌ {}: {}", e),
        }
    }

    println!("\n=== Diagnostic Complete ===");
    println!("\nAdditional manual checks:");
    println!("  1. Run: sc query usbipd    (check if service is running)");
    println!("  2. Run: usbipd list         (check if driver is loaded)");
    println!("  3. Look in Device Manager under 'Universal Serial Bus devices' for VHCI entries");
}