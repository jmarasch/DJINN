## Which file do I download?

| You have | Download |
| --- | --- |
| **Windows 10 or 11** | **`djinn-setup-windows-x86_64.exe`**, and run it. That's the installer. |
| **Linux** (64-bit PC) | Run `curl -fsSL https://github.com/jmarasch/DJINN/releases/latest/download/install.sh \| sh`, or download **`djinn-linux-x86_64.tar.gz`**, unpack it, and run `./djinn/install.sh`. |

Both install for your user only, with no admin rights. Already have DJINN? It updates itself from Admin > About; no download needed.

The other files are for DJINN's own use, so you don't need them:

- `djinn-windows-x86_64.exe` and `djinn-linux-x86_64`: the program on its own, without the installer. This is what DJINN downloads when it updates itself.
- `gpu-...zst`: the optional GPU pack for NVIDIA cards. DJINN downloads it from Admin > AI when you turn it on.
- `install.sh`: the Linux installer used by the one-line install above.
- `SHA256SUMS` and `SHA256SUMS.sig`: checksums and their signature, which DJINN and `install.sh` use to check downloads.

The Windows installer isn't code signed yet, so Windows may say "Windows protected your PC". Click **More info**, then **Run anyway**.

<!-- end of the download guide: DJINN shows only what follows -->
