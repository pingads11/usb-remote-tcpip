# USB Passthrough: macOS → Windows (Open Source)
### Build Plan + Reusable AI Prompt

---

## Project Goal

Share a USB device physically plugged into a **Mac** so that a **Windows PC** on the same network
can use it natively — with the first scenario being an Android phone accessible via `adb` on Windows.

**Open source only. No VirtualHere. No USB Network Gate.**

---

## How It Works (One Paragraph)

USB/IP is a protocol that forwards raw USB traffic over TCP. The Mac runs a userspace USB/IP
**server** (using `libusb` — no kernel changes needed). The Windows PC runs `usbipd-win` as a
**client**, which installs a virtual USB host controller driver and makes the remote device appear
as if it were physically plugged in locally. ADB on Windows then talks to the virtual Android
device exactly as it normally would.

```
[Android phone]──USB──▶[Mac: pyusbip server :3240]──TCP/LAN──▶[Windows: usbipd-win client]──▶[adb.exe]
```

---

## Key Libraries & Tools

| Component | Tool | Notes |
|---|---|---|
| macOS USB/IP server | `tumayt/pyusbip` (Python) | Fork of jwise/pyusbip, tested on macOS with libusb1 |
| Windows USB/IP client | `dorssel/usbipd-win` | Microsoft-backed, open source, mature |
| USB access on macOS | `libusb1` (Python binding) | Userspace, no SIP changes needed |
| ADB on Windows | Google Platform Tools | Standard install |
| Google USB drivers | Google USB Driver | Needed for Windows to recognize Android over USB/IP |

---

## Phase 1 — Manual MVP (get `adb devices` working, no GUI)

**Goal:** Prove the full chain works end-to-end with terminal commands only.
**Time estimate:** 1–2 hours.

### Mac setup

```bash
# 1. Install libusb (required by pyusbip)
brew install libusb

# 2. Clone the working macOS fork of pyusbip
git clone https://github.com/tumayt/pyusbip
cd pyusbip
python3 -m venv .venv
source .venv/bin/activate
pip install libusb1

# 3. Kill any local ADB server that might be holding the Android device
adb kill-server

# 4. Plug in your Android phone (USB debugging ON, trust this computer)
# 5. Start the USB/IP server — it will expose all USB devices on TCP 3240
python3 pyusbip.py

# You should see output listing your Android device by VID:PID
# e.g. "found device 18d1:4ee7 (Google Android)"
```

> **Firewall note:** Allow TCP 3240 inbound on your Mac:
> System Settings → Firewall → Options → add Python / allow incoming.

### Windows setup

```powershell
# 1. Install usbipd-win (run as Administrator)
winget install --interactive --exact dorssel.usbipd-win

# 2. Install Google USB Driver (for ADB to recognise the Android device)
# Download from: https://developer.android.com/studio/run/win-usb
# Install via Device Manager after the device appears

# 3. List devices exported from the Mac (replace with your Mac's LAN IP)
usbipd list --remote 192.168.1.x

# 4. Attach the Android device (use the busid shown in the list output)
usbipd attach --remote 192.168.1.x --busid <BUSID>

# 5. Verify — Android should appear in Device Manager
# Then run:
adb devices
# Expected: your device listed as "device" or "unauthorized"
```

### Troubleshooting Phase 1

| Symptom | Fix |
|---|---|
| `pyusbip` can't open device | Run `adb kill-server` on Mac first; local ADB holds the device |
| `usbipd list` shows nothing | Check Mac firewall, confirm pyusbip is running, ping Mac from Windows |
| Device shows as "unknown" in Device Manager | Install Google USB Driver, then re-attach |
| `adb devices` shows "unauthorized" | Unlock phone, accept the "Allow USB debugging" prompt on screen |
| Device disconnects after a few seconds | Android device screen may have locked; keep screen on during testing |

---

## Phase 2 — Scripted & Reliable (1–2 weeks)

**Goal:** Replace manual steps with scripts. Auto-detect Android. Handle reconnects.

### Mac: `server.py` (enhanced pyusbip wrapper)

Key additions to write on top of pyusbip:

```python
# server.py responsibilities:
# 1. Kill local adb server before starting
# 2. Filter to only share Android devices (VID list: 18d1, 04e8, 0b05, etc.)
# 3. Watch for device plug/unplug via libusb hotplug API
# 4. Restart server automatically when Android reconnects
# 5. Print current Mac LAN IP so user can copy it to Windows easily
# 6. Optionally broadcast via mDNS so Windows can find Mac without knowing IP

import subprocess, libusb1, socket, threading

ANDROID_VIDS = {0x18d1, 0x04e8, 0x0b05, 0x12d1, 0x2717, 0x1004}  # Google, Samsung, Asus, Huawei, Xiaomi, LG

def get_local_ip():
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.connect(("8.8.8.8", 80))
    return s.getsockname()[0]

def kill_local_adb():
    subprocess.run(["adb", "kill-server"], capture_output=True)

# Then wrap pyusbip's UsbIpServer with the above helpers
```

### Windows: `attach.ps1` (auto-attach script)

```powershell
# attach.ps1
param([string]$MacIP = "")

# Auto-discover Mac via mDNS if IP not given (Phase 3 feature)
# For now, prompt if empty
if (-not $MacIP) {
    $MacIP = Read-Host "Enter Mac IP address"
}

Write-Host "Listing devices on $MacIP..."
$devices = usbipd list --remote $MacIP 2>&1

# Parse output, find Android by VID
$androidLine = $devices | Select-String "18d1|04e8|0b05|12d1"
if (-not $androidLine) {
    Write-Error "No Android device found on $MacIP"
    exit 1
}

$busId = ($androidLine -split "\s+")[1]
Write-Host "Attaching Android device at bus $busId..."
usbipd attach --remote $MacIP --busid $busId

Write-Host "Done. Run: adb devices"
```

---

## Phase 3 — Simple GUI (4–8 weeks)

**Goal:** A menu bar app on Mac and a system tray app on Windows. One-click share/attach.

### Recommended stack: Tauri (Rust + WebView)

Why Tauri: single Rust codebase compiles to both macOS `.app` and Windows `.exe`, shares
all the core USB/IP and mDNS logic, and ships as a small native binary (~5MB).

```
usb-passthrough/
├── src-tauri/          # Rust: USB/IP server, mDNS, device detection
│   ├── main.rs
│   ├── usbip_server.rs  # Port pyusbip logic to Rust using rusb crate
│   ├── mdns.rs          # Bonjour/mDNS using mdns-sd crate
│   └── android.rs       # Android VID list, adb kill-server
├── src/                # Web frontend: React or plain HTML
│   ├── App.jsx          # Device list, toggle share, show IP/QR
│   └── tray.html        # Minimal tray popup
├── package.json
└── Cargo.toml
```

**macOS menu bar behaviour:**
- Shows list of connected USB devices
- Toggle switch per device to "Share"
- Shows LAN IP + copy button
- Status: connected clients count

**Windows tray behaviour:**
- Auto-discovers Mac servers on LAN via mDNS
- Lists available devices from each server
- "Attach" button per device
- Auto-reattach on device reconnect

### Rust crates to use

```toml
[dependencies]
rusb = "0.9"           # libusb bindings — USB device access on macOS
mdns-sd = "0.11"       # mDNS/Bonjour — zero-config discovery
tokio = { version = "1", features = ["full"] }  # async runtime
serde = { version = "1", features = ["derive"] }
tauri = { version = "2", features = ["tray-icon"] }
```

---

## Repository Structure (suggested)

```
usb-passthrough-macos/
├── README.md
├── server/              # Mac side
│   ├── server.py        # Phase 1–2: Python pyusbip wrapper
│   ├── requirements.txt # libusb1
│   └── install.sh       # brew install libusb + pip install
├── client/              # Windows side
│   ├── attach.ps1       # Phase 1–2: PowerShell auto-attach
│   └── install.ps1      # winget install usbipd-win
├── app/                 # Phase 3: Tauri GUI (both platforms)
│   ├── src-tauri/
│   └── src/
└── docs/
    ├── android-adb.md   # This scenario
    └── known-issues.md
```

---

## Known Gotchas for Android / ADB Specifically

1. **Local ADB server on Mac grabs the device first.** Always run `adb kill-server` on the Mac before starting pyusbip. If you don't, libusb cannot claim the device.

2. **Android screen lock disconnects ADB.** During testing keep the screen on (`Settings → Developer Options → Stay awake`).

3. **"Unauthorized" on Windows.** The trust prompt appears on the Android screen, not on Windows. Unlock phone and tap "Allow" after attaching.

4. **Google USB Driver required on Windows.** Unlike Linux (which uses ADB's built-in kernel driver), Windows needs the Google USB Driver installed for the virtual USB device to be recognized by ADB. Download at `developer.android.com/studio/run/win-usb`.

5. **Multiple Android interfaces.** An Android device presents multiple USB interfaces (ADB, MTP, RNDIS). `pyusbip` attempts to claim all interfaces. If it can't claim one (e.g., the MTP interface), it currently fails rather than proceeding with just ADB. Short-term fix: set phone to "USB debugging" mode only (in Developer Options, set USB mode to "No data transfer" or "Charging only"), which suppresses MTP.

6. **ADB over TCP alternative for simple cases.** If you just need `adb shell` and not full USB (e.g., no fastboot, no sideloading), consider `adb tcpip 5555` + `adb connect <phone-ip>` — it requires the phone to be on the same WiFi and a one-time USB connection to enable TCP mode. Not a replacement for USB passthrough, but worth knowing.

---

---

# REUSABLE AI PROMPT
## Copy everything below this line into a new Claude or ChatGPT conversation

---

```
## Project: USB Passthrough macOS → Windows (Open Source)

I'm building an open source tool that lets a USB device physically connected to a Mac be
accessible on a Windows PC over the local network. The first use case is Android ADB: my
Android phone is plugged into my Mac, and I want to run `adb` commands from Windows.

### Architecture

- Mac (server): runs a USB/IP server using `pyusbip` (github.com/tumayt/pyusbip) with the
  `libusb1` Python library. This exposes USB devices over TCP port 3240 without any kernel
  changes or SIP modifications.
- Windows (client): uses `usbipd-win` (github.com/dorssel/usbipd-win) to attach to the Mac's
  USB/IP server. The device then appears as a virtual USB device on Windows and ADB works normally.
- Protocol: standard USB/IP over TCP, compatible between pyusbip and usbipd-win.

### Current project state

We are at [PASTE ONE OF: "Phase 1 — manual CLI setup" / "Phase 2 — writing Python/PowerShell
scripts" / "Phase 3 — building Tauri GUI app"].

### Tech stack decisions already made

- Server (Mac): Python 3.11+, pyusbip fork (tumayt/pyusbip), libusb1 pip package
- Client (Windows): usbipd-win (installed via winget), Google USB Driver for ADB
- Phase 3 GUI: Tauri 2 (Rust + WebView), rusb crate, mdns-sd crate
- Repo structure: monorepo with /server (Python), /client (PowerShell), /app (Tauri)

### Key gotchas already discovered

1. On Mac, local ADB server grabs the Android device before pyusbip can. Fix: `adb kill-server`
   before starting pyusbip.
2. Android must be in "Charging only" USB mode (not MTP) so pyusbip can claim all interfaces.
3. On Windows, Google USB Driver must be installed for ADB to recognise the virtual USB device.
4. Android trust prompt ("Allow USB debugging from this computer?") appears on phone screen after
   attaching on Windows — user must tap Allow.
5. Android screen lock drops the ADB connection. Keep screen awake during development.

### Android VID list (for filtering only Android devices)

0x18d1 (Google/Pixel), 0x04e8 (Samsung), 0x0b05 (Asus), 0x12d1 (Huawei),
0x2717 (Xiaomi), 0x1004 (LG), 0x22b8 (Motorola), 0x0fce (Sony), 0x2a96 (OnePlus)

### What I need help with right now

[DESCRIBE YOUR CURRENT TASK, e.g.:]
- "Write an enhanced server.py that wraps pyusbip, filters for Android devices only,
  kills the local adb server automatically, and restarts on hotplug"
- "Write the PowerShell attach.ps1 script for Windows"
- "Help me set up the Tauri project structure"
- "Debug: pyusbip starts but usbipd-win can't connect — here's the error: [PASTE ERROR]"

Please write production-quality code with error handling and comments.
Assume I'm comfortable with Python and basic Rust but new to USB/IP internals.
```

---

## Progress Checklist

Use this to track where you are:

- [ ] Phase 1: `pyusbip` running on Mac, `adb kill-server` done
- [ ] Phase 1: `usbipd-win` installed on Windows
- [ ] Phase 1: `usbipd list --remote <mac-ip>` shows Android device
- [ ] Phase 1: `usbipd attach` succeeds, device in Device Manager
- [ ] Phase 1: Google USB Driver installed, `adb devices` shows phone
- [ ] Phase 1: Full round-trip working — `adb shell uname` returns output
- [ ] Phase 2: `server.py` with Android filtering + hotplug + auto adb kill
- [ ] Phase 2: `attach.ps1` with auto-detection and re-attach loop
- [ ] Phase 2: Tested reconnect scenario (unplug/replug phone)
- [ ] Phase 3: Tauri project scaffolded
- [ ] Phase 3: Rust USB/IP server ported from Python
- [ ] Phase 3: mDNS discovery working (Mac announces, Windows finds)
- [ ] Phase 3: macOS menu bar app working
- [ ] Phase 3: Windows tray app working
- [ ] Phase 3: macOS `.app` signed and notarized
- [ ] Phase 3: Windows `.exe` / `.msi` installer built