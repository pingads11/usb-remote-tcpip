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
    ];

    println!("1. Testing common device paths:");
    for p in &paths {
        match fs::OpenOptions::new().read(true).write(true).open(p) {
            Ok(f) => {
                println!("   ✅ FOUND: {}", p);
                let _ = f;
            }
            Err(e) => {
                println!("   ❌ {}: {}", p, e);
            }
        }
    }

    // 2. Try to list named pipes
    println!("\n2. Named pipes (may indicate VHCI service):");
    let pipe_paths = [
        "\\\\.\\pipe\\usbipd",
        "\\\\.\\pipe\\usbipd-win",
        "\\\\.\\pipe\\usbip-vhci",
    ];
    for p in &pipe_paths {
        match fs::OpenOptions::new().read(true).write(true).open(p) {
            Ok(_) => println!("   ✅ FOUND: {}", p),
            Err(e) => println!("   ❌ {}: {}", p, e),
        }
    }

    // 3. Try raw Windows API enumeration
    #[cfg(target_os = "windows")]
    {
        println!("\n3. SetupAPI enumeration (requires admin):");
        use std::os::windows::ffi::OsStrExt;

        extern "system" {
            fn SetupDiGetClassDevsW(
                class_guid: *const u8,
                enumerator: *const u16,
                hwnd_parent: *mut std::ffi::c_void,
                flags: u32,
            ) -> *mut std::ffi::c_void;

            fn SetupDiEnumDeviceInfo(
                device_info_set: *mut std::ffi::c_void,
                index: u32,
                device_info_data: *mut std::ffi::c_void,
            ) -> i32;
        }

        // This is a simplified diagnostic — just check if SetupAPI is accessible
        unsafe {
            let handle = SetupDiGetClassDevsW(
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                0x00000002, // DIGCF_ALLCLASSES
            );
            if handle.is_null() {
                println!("   SetupAPI inaccessible (run as Admin?)");
            } else {
                println!("   SetupAPI accessible (handle: {:?})", handle);
            }
        }
    }

    println!("\n=== Diagnostic Complete ===");
    println!("\nIf no device paths found, the VHCI driver may not be loaded.");
    println!("Run: sc query usbipd");
    println!("And try: usbipd list");
    println!("If usbipd works, the driver IS loaded — just under a different namespace.");
}