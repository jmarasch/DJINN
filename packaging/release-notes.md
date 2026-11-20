## Which file do I download?

| You have | Download |
| --- | --- |
| **Windows 10 or 11** | **`djinn-setup-windows-x86_64.exe`**, and run it. That's the installer. |
| **Linux** (64-bit PC) | Run `curl -fsSL https://github.com/jmarasch/DJINN/releases/latest/download/install.sh \| sh`, or download **`djinn-linux-x86_64.tar.gz`**, unpack it, and run `./djinn/install.sh`. |
| **A spare PC** to give over to DJINN | **`djinn-installer-pc-x86_64.iso`**: write it to a USB stick and start the PC from it. It erases that PC. |
| **A Raspberry Pi** | **`djinn-pi-arm64.img.xz`** for a Pi 3, 4 or 5, or **`djinn-pi-armhf.img.xz`** for any Pi. Write it to a memory card with Raspberry Pi Imager. |

The Windows and Linux installs are for your user only, with no admin rights. Every install comes with the user guide, which DJINN shows under Admin > User Guide. Already have DJINN? It updates itself from Admin > System > Updates; no download needed.

The other files are for DJINN's own use, so you don't need them:

- `djinn-windows-x86_64.exe`, `djinn-linux-x86_64`, `djinn-linux-aarch64` and `djinn-linux-arm`: the program on its own, without an installer. This is what DJINN downloads when it updates itself.
- `djinn-content.zip`: the user guide and language files, which updates bring along with the program.
- `djinn-linux-aarch64.tar.gz` and `djinn-linux-arm.tar.gz`: Linux downloads for 64-bit and 32-bit ARM, which the one-line install picks by itself.
- `djinn-go-android.apk`: the DJINN GO app for Android. Your DJINN offers it to phones; no need to download it here.
- `gpu-...zst`: the optional GPU pack for NVIDIA cards. DJINN downloads it from Admin > AI when you turn it on.
- `install.sh`: the Linux installer used by the one-line install above.
- `SHA256SUMS` and `SHA256SUMS.sig`: checksums and their signature, which DJINN and `install.sh` use to check downloads.

The Windows installer isn't code signed, so Windows may say "Windows protected your PC". Click **More info**, then **Run anyway**.

<!-- end of the download guide: DJINN shows only what follows -->
