# Kurogane Community Recipes 🚀

Welcome to the Kurogane Community Appliance Catalog!

This directory hosts community-maintained appliance recipes. Anyone can contribute a single-purpose, instant-on Linux appliance in under 15 minutes.

---

## How to Add Your Favorite Tool

1. Copy `template.yaml` to `your-tool.yaml`.
2. Specify the Arch or Alpine packages you need (`packages:`).
3. Set your entrypoint binary and compositor (`display:`).
4. Validate your manifest:
   ```bash
   ./kurogane validate recipes/community/your-tool.yaml
   ```
5. Test it in QEMU:
   ```bash
   ./kurogane run recipes/community/your-tool.yaml
   ```
6. Open a Pull Request! We merge clean, working recipes quickly.

---

## Curated Community Starters

* [`editor-helix.yaml`](editor-helix.yaml) — Instant-on modal editor (Helix)
* [`media-cmus.yaml`](media-cmus.yaml) — Offline console music player on ALSA dmix
* [`terminal-weechat.yaml`](terminal-weechat.yaml) — Distraction-free IRC terminal
