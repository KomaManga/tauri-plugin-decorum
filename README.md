# tauri-plugin-decoration

[![crates.io](https://img.shields.io/crates/v/tauri-plugin-decoration.svg)](https://crates.io/crates/tauri-plugin-decoration)

Native window controls, custom decorations, and Windows 11 Snap Layout for Tauri v2 apps.

## Features

- **Frameless window controls** — custom minimize/maximize/close buttons rendered as HTML, positioned over a transparent draggable titlebar area.
- **Windows 11 Snap Layout** — a native Win32 child HWND overlay returns `HTMAXBUTTON` from `WM_NCHITTEST`, triggering the OS-built Snap Layout flyout on hover. No keyboard or mouse simulation.
- **macOS traffic lights** — inset positioning for the native close/minimize/zoom buttons so they align with your app content. Also supports window transparency and window level control.
- **Linux system icons** — window control buttons use the current icon theme (via `linicon` + `dconf`).
- **Dark/light aware** — hover colors adapt to the user's color scheme preference.
- **Late-injection safe** — scripts check `document.readyState` so they work even when injected after `DOMContentLoaded`.

> [!NOTE]
> Windows 10: the overlay is created and positioned correctly, but the Snap Layout flyout is a Windows 11 feature. On Windows 10, hovering the maximize button still triggers maximize/restore — just no flyout.

## Usage

### 1. Add the plugin

```toml
# Cargo.toml
[dependencies]
tauri-plugin-decoration = "2"
```

### 2. Initialize in Rust

```rust
use tauri::Manager;
use tauri_plugin_decoration::WebviewWindowExt;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_decoration::init())
        .setup(|app| {
            let main_window = app.get_webview_window("main").unwrap();
            main_window.create_overlay_titlebar().unwrap();

            // macOS-only helpers
            #[cfg(target_os = "macos")]
            {
                main_window.set_traffic_lights_inset(12.0, 16.0).unwrap();
                main_window.make_transparent().unwrap();
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### 3. Configure window

In `tauri.conf.json`, set `withGlobalTauri: true` and use a transparent/overlay titlebar. **Do not set `decorations: false`** — the plugin manages decorations per-platform in `create_overlay_titlebar` (macOS needs `decorations: true` so the native traffic-light buttons exist; Windows/Linux are set to `false` automatically).

```json
{
  "app": {
    "withGlobalTauri": true,
    "windows": [
      {
        "titleBarStyle": "Overlay",
        "hiddenTitle": true
      }
    ]
  }
}
```

> [!NOTE]
> On macOS, `titleBarStyle: "Overlay"` + `hiddenTitle: true` gives the frameless look while keeping the native traffic-light buttons. Setting `decorations: false` on macOS removes the traffic lights entirely — the plugin's `set_traffic_lights_inset` would have nothing to reposition.

### 4. Set permissions

In `src-tauri/capabilities/default.json`:

```json
"core:window:allow-close",
"core:window:allow-center",
"core:window:allow-minimize",
"core:window:allow-maximize",
"core:window:allow-set-size",
"core:window:allow-set-focus",
"core:window:allow-is-maximized",
"core:window:allow-start-dragging",
"core:window:allow-toggle-maximize"
```

### 5. Style the controls (optional)

The plugin injects buttons with these selectors:

```css
button#decoration-tb-minimize,
button#decoration-tb-maximize,
button#decoration-tb-close,
div[data-tauri-decoration-tb] {}
```

### 6. Interactive content in the titlebar area

The plugin creates a fixed-position overlay (`div[data-tauri-decoration-tb]`) at `z-index: 100` covering the top 32px of the window. This overlay handles window dragging and hosts the window control buttons. Any app content in that area will be behind the overlay and won't receive clicks.

If you need interactive elements in the titlebar area (dropdown menus, navigation tabs, etc.), position your content above the overlay and use `pointer-events` to let clicks fall through to the drag region in empty areas:

```css
.titlebar-content {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  z-index: 200;          /* above the decoration overlay (z-index: 100) */
  height: 32px;
  pointer-events: none;  /* let clicks fall through to the drag region */
}

.titlebar-content .interactive-element {
  pointer-events: auto;  /* re-enable clicks on menus, buttons, etc. */
}
```

### 7. Avoiding overlap with macOS traffic lights (macOS only)

The macOS traffic-light buttons are native OS controls rendered on top of the webview — the plugin cannot move arbitrary app content out of their way automatically (it has no knowledge of your DOM layout). Instead, `set_traffic_lights_inset` exposes the cluster's right edge to the webview as a CSS custom property so your titlebar content can offset itself with a single line of CSS:

```css
.titlebar-content {
  /* Pushed past the traffic lights on macOS; falls back to 8px
     on Windows/Linux where the window controls are on the right. */
  padding-left: var(--decoration-traffic-light-left, 8px);
}
```

The variable is set on `:root` after `set_traffic_lights_inset(x, y)` runs, and equals `x + (button_count - 1) * 20 + button_width + 8` (a small breathing gap is included). If you never call `set_traffic_lights_inset`, the variable is unset and the fallback applies.

## Example App

The repository includes a demo app at [`examples/tauri-app`](examples/tauri-app) showing a custom titlebar with dropdown menus.

```sh
pnpm install
pnpm example:dev
```

## Platform Support

| Feature | Windows | macOS | Linux |
|---|---|---|---|
| Custom window controls | Native HTML buttons | Native HTML buttons | System icon theme |
| Snap Layout flyout | Windows 11 only | — | — |
| Traffic light inset | — | Yes | — |
| Window transparency | — | Yes | — |
| Draggable titlebar | Yes | Yes | Yes |

## Credits

Original author: [clearlysid/tauri-plugin-decorum](https://github.com/clearlysid/tauri-plugin-decorum).
Snap Layout implementation inspired by [clarifei/tauri-plugin-frame](https://github.com/clarifei/tauri-plugin-frame) and [Hyph-M/tauri-plugin-snap-layout](https://github.com/Hyph-M/tauri-plugin-snap-layout).
