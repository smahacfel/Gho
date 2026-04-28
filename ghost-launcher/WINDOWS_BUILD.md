# Building Ghost Launcher for Windows

This guide provides detailed instructions for building the Ghost Launcher as a standalone Windows executable (.exe).

## Prerequisites

### Option 1: Building on Windows

Install Rust on Windows:
1. Download and run [rustup-init.exe](https://rustup.rs/)
2. Follow the installation wizard
3. Restart your terminal/PowerShell

Verify installation:
```powershell
rustc --version
cargo --version
```

### Option 2: Cross-Compiling from Linux

Install the Windows target:
```bash
rustup target add x86_64-pc-windows-gnu
```

Install MinGW cross-compiler:
```bash
# Ubuntu/Debian
sudo apt-get install mingw-w64

# Fedora/RHEL
sudo dnf install mingw64-gcc

# Arch Linux
sudo pacman -S mingw-w64-gcc
```

## Building on Windows

### Release Build (Optimized)

Open PowerShell or Command Prompt in the project directory:

```powershell
cargo build --release --bin ghost-launcher
```

The executable will be at: `target\release\ghost-launcher.exe`

### Debug Build (For Development)

```powershell
cargo build --bin ghost-launcher
```

The executable will be at: `target\debug\ghost-launcher.exe`

### Build with Maximum Optimization

For the smallest and fastest executable:

```powershell
$env:RUSTFLAGS="-C target-cpu=native"
cargo build --release --bin ghost-launcher
```

## Cross-Compiling from Linux

### Build for 64-bit Windows

```bash
cargo build --release --bin ghost-launcher --target x86_64-pc-windows-gnu
```

The executable will be at: `target/x86_64-pc-windows-gnu/release/ghost-launcher.exe`

### Troubleshooting Cross-Compilation

If you encounter linker errors, create or edit `~/.cargo/config.toml`:

```toml
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
ar = "x86_64-w64-mingw32-ar"
```

## Optimizing the Build

### Reducing Executable Size

1. Strip debug symbols (already done in release mode):
```powershell
cargo build --release --bin ghost-launcher
strip target\release\ghost-launcher.exe  # On Linux/macOS
```

On Windows, use:
```powershell
cargo install cargo-strip
cargo strip --release --bin ghost-launcher
```

2. Enable additional optimizations in `Cargo.toml`:
```toml
[profile.release]
opt-level = "z"     # Optimize for size
lto = true          # Enable Link Time Optimization
codegen-units = 1   # Better optimization
strip = true        # Strip symbols
panic = "abort"     # Smaller panic handling
```

3. Use UPX compression (optional):
```powershell
# Download UPX from https://upx.github.io/
upx --best --lzma target\release\ghost-launcher.exe
```

Note: Some antivirus software may flag UPX-compressed executables as suspicious.

## Deployment Package

### Creating a Deployment Package

Create a directory structure for distribution:

```
ghost-launcher-windows/
├── ghost-launcher.exe
├── config.toml.example
├── README.md
└── logs/
    └── .gitkeep
```

PowerShell script to create the package:

```powershell
# Create deployment directory
New-Item -ItemType Directory -Force -Path ghost-launcher-windows
New-Item -ItemType Directory -Force -Path ghost-launcher-windows\logs

# Copy files
Copy-Item target\release\ghost-launcher.exe ghost-launcher-windows\
Copy-Item config.toml.example ghost-launcher-windows\
Copy-Item ghost-launcher\README.md ghost-launcher-windows\

# Create archive
Compress-Archive -Path ghost-launcher-windows\* -DestinationPath ghost-launcher-windows.zip
```

Bash script (for Linux/macOS):

```bash
#!/bin/bash

# Create deployment directory
mkdir -p ghost-launcher-windows/logs

# Copy files
cp target/x86_64-pc-windows-gnu/release/ghost-launcher.exe ghost-launcher-windows/
cp config.toml.example ghost-launcher-windows/
cp ghost-launcher/README.md ghost-launcher-windows/

# Create archive
zip -r ghost-launcher-windows.zip ghost-launcher-windows/
```

## Installation Instructions for End Users

Include these instructions in your deployment package:

### Quick Start

1. **Extract the Archive**
   - Unzip `ghost-launcher-windows.zip` to a directory of your choice
   - Example: `C:\Ghost\`

2. **Create Configuration**
   - Copy `config.toml.example` to `config.toml`
   - Edit `config.toml` with your settings (RPC endpoints, etc.)

3. **Run the Launcher**
   - Double-click `ghost-launcher.exe`
   - Or run from Command Prompt: `ghost-launcher.exe`

4. **Access the GUI**
   - Open your browser to: `http://localhost:8800`

### Running from Command Line

```cmd
cd C:\Ghost
ghost-launcher.exe
```

With custom config file:
```cmd
ghost-launcher.exe C:\path\to\config.toml
```

Generate default config:
```cmd
ghost-launcher.exe --generate-config
```

### Stopping the Application

Press `Ctrl+C` in the terminal window, or close the window.

## Running as a Windows Service

### Using NSSM (Non-Sucking Service Manager)

1. Download NSSM from https://nssm.cc/download
2. Extract and run as administrator:

```cmd
nssm install GhostLauncher
```

3. Configure the service:
   - **Path**: `C:\Ghost\ghost-launcher.exe`
   - **Startup directory**: `C:\Ghost`
   - **Arguments**: (leave empty to use default config.toml)

4. Set service to start automatically:
```cmd
nssm set GhostLauncher Start SERVICE_AUTO_START
```

5. Start the service:
```cmd
nssm start GhostLauncher
```

6. Check service status:
```cmd
nssm status GhostLauncher
```

### Managing the Service

View logs:
```cmd
type C:\Ghost\logs\ghost.log
```

Stop the service:
```cmd
nssm stop GhostLauncher
```

Remove the service:
```cmd
nssm remove GhostLauncher confirm
```

## Troubleshooting

### Build Errors

**"error: linker `link.exe` not found"**
- Install Visual Studio Build Tools or use the `x86_64-pc-windows-gnu` target

**"error: failed to run custom build command"**
- Some dependencies may require additional system libraries
- Try building with `--no-default-features` if available

### Runtime Errors

**"VCRUNTIME140.dll was not found"**
- Install [Microsoft Visual C++ Redistributable](https://aka.ms/vs/17/release/vc_redist.x64.exe)

**"The application was unable to start correctly (0xc000007b)"**
- Ensure you're using the 64-bit executable on a 64-bit system

**Antivirus Blocking Executable**
- Add exception in your antivirus software
- If using UPX compression, consider building without it

**Port Already in Use**
- Change ports in `config.toml`
- Check for other applications using ports 8800, 9090, 9091

## Advanced Configuration

### Static Linking

For a fully standalone executable without DLL dependencies:

```toml
# In Cargo.toml
[profile.release]
# ... existing settings ...

[target.x86_64-pc-windows-gnu]
rustflags = ["-C", "target-feature=+crt-static"]
```

Build:
```powershell
cargo build --release --bin ghost-launcher --target x86_64-pc-windows-gnu
```

### Environment Variables

Set Rust flags for optimization:

```powershell
# PowerShell
$env:RUSTFLAGS="-C target-cpu=native -C link-arg=-s"
cargo build --release --bin ghost-launcher

# Command Prompt
set RUSTFLAGS=-C target-cpu=native -C link-arg=-s
cargo build --release --bin ghost-launcher
```

## Verification

### Check Executable Properties

```powershell
# File size
(Get-Item target\release\ghost-launcher.exe).Length / 1MB

# Version info
.\target\release\ghost-launcher.exe --help
```

### Test Run

```powershell
# Generate test config
.\target\release\ghost-launcher.exe --generate-config

# Run with test config
.\target\release\ghost-launcher.exe
```

Press `Ctrl+C` to stop after verifying all components start correctly.

## Distribution Checklist

Before distributing the Windows executable:

- [ ] Built in release mode with optimizations
- [ ] Tested on clean Windows installation
- [ ] Included `config.toml.example` with sensible defaults
- [ ] Included comprehensive README with usage instructions
- [ ] Documented all required configuration options
- [ ] Created logs directory in package
- [ ] Verified all components start correctly
- [ ] Tested graceful shutdown (Ctrl+C)
- [ ] Checked executable is not flagged by Windows Defender
- [ ] Included license information
- [ ] Added version number to executable or package name

## Build Automation

### PowerShell Build Script

Save as `build-windows.ps1`:

```powershell
#!/usr/bin/env pwsh

Write-Host "Building Ghost Launcher for Windows..." -ForegroundColor Green

# Clean previous builds
if (Test-Path target\release\ghost-launcher.exe) {
    Remove-Item target\release\ghost-launcher.exe
}

# Build
cargo build --release --bin ghost-launcher

if ($LASTEXITCODE -eq 0) {
    Write-Host "Build successful!" -ForegroundColor Green
    Write-Host "Executable: target\release\ghost-launcher.exe"
    
    $size = (Get-Item target\release\ghost-launcher.exe).Length
    Write-Host "Size: $([math]::Round($size/1MB, 2)) MB"
} else {
    Write-Host "Build failed!" -ForegroundColor Red
    exit 1
}
```

Run:
```powershell
.\build-windows.ps1
```

### Bash Build Script (for Cross-Compilation)

Save as `build-windows.sh`:

```bash
#!/bin/bash

echo "Cross-compiling Ghost Launcher for Windows..."

# Clean previous builds
rm -f target/x86_64-pc-windows-gnu/release/ghost-launcher.exe

# Build
cargo build --release --bin ghost-launcher --target x86_64-pc-windows-gnu

if [ $? -eq 0 ]; then
    echo "Build successful!"
    echo "Executable: target/x86_64-pc-windows-gnu/release/ghost-launcher.exe"
    
    size=$(stat -f%z target/x86_64-pc-windows-gnu/release/ghost-launcher.exe 2>/dev/null || \
           stat -c%s target/x86_64-pc-windows-gnu/release/ghost-launcher.exe)
    echo "Size: $(echo "scale=2; $size/1048576" | bc) MB"
else
    echo "Build failed!"
    exit 1
fi
```

Make executable and run:
```bash
chmod +x build-windows.sh
./build-windows.sh
```

## Support

For build issues, please check:
1. Rust version: `rustc --version` (should be 1.72+)
2. Cargo version: `cargo --version`
3. Target installed: `rustup target list | grep windows`

Report issues at: https://github.com/Mezoscope/ProjectSolanaGhost/issues
