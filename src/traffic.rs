// Most contents of this file are taken from Hoppscotch's tauri app.
// I think there is work to be done to improve it, but I'm happy with it for now.
// Reference source code is linked below.
// https://github.com/hoppscotch/hoppscotch/blob/286fcd2bb08a84f027b10308d1e18da368f95ebf/packages/hoppscotch-selfhost-desktop/src-tauri/src/mac/window.rs

use objc::{msg_send, sel, sel_impl};
use rand::{distr::Alphanumeric, rng, Rng};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};
use tauri::{Emitter, Runtime, Webview, Window};

const WINDOW_CONTROL_PAD_X: f64 = 12.0;
const WINDOW_CONTROL_PAD_Y: f64 = 16.0;
const WINDOW_CONTROL_CLEARANCE_GAP: f64 = 8.0;
const TRAFFIC_LIGHT_LEFT_CSS_VAR: &str = "--decoration-traffic-light-left";

#[cfg(target_os = "macos")]
fn traffic_light_left_clearance(cluster_right_edge: f64) -> f64 {
    if cluster_right_edge > 0.0 {
        cluster_right_edge + WINDOW_CONTROL_CLEARANCE_GAP
    } else {
        0.0
    }
}

#[cfg(test)]
fn traffic_light_left_css_script(cluster_right_edge: f64) -> String {
    let left_clearance = traffic_light_left_clearance(cluster_right_edge);
    format!(
        "document.documentElement.style.setProperty('{TRAFFIC_LIGHT_LEFT_CSS_VAR}','{left_clearance}px')"
    )
}

// The traffic-light CSS updater script is stored as a raw string literal
// with unique placeholder tokens, then substituted at call time via
// `.replace()`. This avoids the `{{`/`}}` brace-escaping that `format!`
// would require for every JS brace, keeping the embedded JS readable and
// syntax-highlightable. The placeholders use `__UPPERCASE__` tokens that
// cannot appear naturally in the JS body.
#[cfg(target_os = "macos")]
const TRAFFIC_LIGHT_UPDATER_JS: &str = r#"(function () {
  const cssVar = "__CSS_VAR_NAME__";
  const stateKey = "__TAURI_DECORATION_TRAFFIC_LIGHT__";
  const state = window[stateKey] || (window[stateKey] = { installed: false, normalLeft: 0 });
  const normalLeft = __NORMAL_LEFT__;
  state.normalLeft = normalLeft;

  const currentWindow = () => {
    const tauri = window.__TAURI__;
    return tauri?.window?.getCurrentWindow?.()
      || tauri?.webviewWindow?.getCurrentWebviewWindow?.()
      || tauri?.webviewWindow?.getCurrent?.();
  };

  const setTrafficLightLeft = (collapsed) => {
    const normalLeft = Number(state.normalLeft || 0);
    document.documentElement.style.setProperty(cssVar, `${collapsed ? 0 : normalLeft}px`);
  };

  const beginEnterFullscreen = () => {
    state.fullscreenTransition = "enter";
    setTrafficLightLeft(true);
  };

  const finishEnterFullscreen = () => {
    state.fullscreenTransition = null;
    setTrafficLightLeft(true);
  };

  const beginExitFullscreen = () => {
    state.fullscreenTransition = "exit";
    setTrafficLightLeft(false);
  };

  const finishExitFullscreen = () => {
    state.fullscreenTransition = null;
    setTrafficLightLeft(false);
  };

  // Note: maximize (the green zoom button's non-fullscreen action) does
  // NOT hide the traffic-light cluster on macOS, so it is intentionally
  // not handled here — only fullscreen collapses the cluster.
  const applyTrafficLightLeft = async () => {
    if (state.fullscreenTransition === "enter") {
      setTrafficLightLeft(true);
      return;
    }
    if (state.fullscreenTransition === "exit") {
      setTrafficLightLeft(false);
      return;
    }

    const win = currentWindow();
    if (!win) {
      setTrafficLightLeft(false);
      return;
    }

    try {
      const fullscreen = typeof win.isFullscreen === "function" ? await win.isFullscreen() : false;
      // Re-check the transition after the await: a will-enter/will-exit
      // event may have fired while the IPC round-trip was in flight and
      // already committed the correct collapsed/expanded value. Without
      // this guard, the stale fullscreen result would overwrite it.
      if (state.fullscreenTransition === "enter") {
        setTrafficLightLeft(true);
        return;
      }
      if (state.fullscreenTransition === "exit") {
        setTrafficLightLeft(false);
        return;
      }
      setTrafficLightLeft(Boolean(fullscreen));
    } catch (_) {
      setTrafficLightLeft(false);
    }
  };

  if (state.installed) {
    applyTrafficLightLeft();
    return;
  }

  state.installed = true;

  const installTrafficLightLeftUpdater = async () => {
    const win = currentWindow();
    if (win) {
      if (typeof win.onResized === "function") await win.onResized(applyTrafficLightLeft);
      if (typeof win.onMoved === "function") await win.onMoved(applyTrafficLightLeft);
      if (typeof win.listen === "function") {
        await win.listen("will-enter-fullscreen", beginEnterFullscreen);
        await win.listen("did-enter-fullscreen", finishEnterFullscreen);
        await win.listen("will-exit-fullscreen", beginExitFullscreen);
        await win.listen("did-exit-fullscreen", finishExitFullscreen);
      }
    }

    window.addEventListener("resize", applyTrafficLightLeft);
    applyTrafficLightLeft();
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", installTrafficLightLeftUpdater, { once: true });
  } else {
    installTrafficLightLeftUpdater();
  }
})()"#;

#[cfg(target_os = "macos")]
pub fn traffic_light_left_css_updater_script(cluster_right_edge: f64) -> String {
    let normal_left = traffic_light_left_clearance(cluster_right_edge);
    TRAFFIC_LIGHT_UPDATER_JS
        .replace("__CSS_VAR_NAME__", TRAFFIC_LIGHT_LEFT_CSS_VAR)
        .replace("__NORMAL_LEFT__", &normal_left.to_string())
}

#[derive(Clone, Copy, Debug)]
pub struct UnsafeWindowHandle(pub *mut std::ffi::c_void);
unsafe impl Send for UnsafeWindowHandle {}
unsafe impl Sync for UnsafeWindowHandle {}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy)]
struct TrafficLightInset {
    x: f64,
    y: f64,
    last_normal_cluster_right_edge: f64,
}

#[cfg(target_os = "macos")]
static PENDING_TRAFFIC_LIGHT_INSETS: OnceLock<Mutex<HashMap<usize, TrafficLightInset>>> =
    OnceLock::new();

#[cfg(target_os = "macos")]
fn pending_traffic_light_insets() -> &'static Mutex<HashMap<usize, TrafficLightInset>> {
    PENDING_TRAFFIC_LIGHT_INSETS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_os = "macos")]
fn ns_window_key(ns_window_handle: UnsafeWindowHandle) -> usize {
    ns_window_handle.0 as usize
}

#[cfg(target_os = "macos")]
fn remember_pending_traffic_light_inset(
    ns_window_handle: UnsafeWindowHandle,
    x: f64,
    y: f64,
    measured_cluster_right_edge: f64,
) -> TrafficLightInset {
    let stored = pending_traffic_light_insets().lock().map(|mut pending| {
        let entry = pending
            .entry(ns_window_key(ns_window_handle))
            .or_insert(TrafficLightInset {
                x,
                y,
                last_normal_cluster_right_edge: 0.0,
            });
        entry.x = x;
        entry.y = y;
        if measured_cluster_right_edge > 0.0 {
            entry.last_normal_cluster_right_edge = measured_cluster_right_edge;
        }
        *entry
    });

    stored.unwrap_or(TrafficLightInset {
        x,
        y,
        last_normal_cluster_right_edge: measured_cluster_right_edge.max(0.0),
    })
}

#[cfg(target_os = "macos")]
fn take_pending_traffic_light_inset(
    ns_window_handle: UnsafeWindowHandle,
) -> Option<TrafficLightInset> {
    pending_traffic_light_insets()
        .lock()
        .ok()
        .and_then(|mut pending| pending.remove(&ns_window_key(ns_window_handle)))
}

#[cfg(target_os = "macos")]
fn clear_pending_traffic_light_inset(ns_window_handle: UnsafeWindowHandle) {
    if let Ok(mut pending) = pending_traffic_light_insets().lock() {
        pending.remove(&ns_window_key(ns_window_handle));
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrafficLightWindowState {
    is_fullscreen: bool,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrafficLightRefreshAction {
    ExposeCollapsed,
    PositionAndExpose,
}

#[cfg(target_os = "macos")]
pub(crate) fn traffic_light_refresh_action(
    state: TrafficLightWindowState,
) -> TrafficLightRefreshAction {
    if state.is_fullscreen {
        TrafficLightRefreshAction::ExposeCollapsed
    } else {
        TrafficLightRefreshAction::PositionAndExpose
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn traffic_light_window_state(
    ns_window_handle: UnsafeWindowHandle,
) -> TrafficLightWindowState {
    use cocoa::appkit::{NSWindow, NSWindowStyleMask};

    let ns_window = ns_window_handle.0 as cocoa::base::id;
    unsafe {
        let style_mask = ns_window.styleMask();
        let is_fullscreen = style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask);

        TrafficLightWindowState { is_fullscreen }
    }
}

/// Reposition the macOS traffic-light buttons to the given inset.
///
/// Returns the right-edge x coordinate of the last positioned button
/// (in window-content space), or `0.0` if the buttons could not be found.
/// Callers can use this to expose a CSS custom property so the webview
/// content can offset itself to avoid overlapping the traffic lights.
#[cfg(target_os = "macos")]
pub fn position_traffic_lights(ns_window_handle: UnsafeWindowHandle, x: f64, y: f64) -> f64 {
    use cocoa::appkit::{NSView, NSWindow, NSWindowButton};
    use cocoa::foundation::NSRect;
    let ns_window = ns_window_handle.0 as cocoa::base::id;
    unsafe {
        let close = ns_window.standardWindowButton_(NSWindowButton::NSWindowCloseButton);
        let miniaturize =
            ns_window.standardWindowButton_(NSWindowButton::NSWindowMiniaturizeButton);
        let zoom = ns_window.standardWindowButton_(NSWindowButton::NSWindowZoomButton);

        // Check if close button exists and has a valid superview
        if close.is_null() {
            return 0.0;
        }

        let close_hidden: cocoa::base::BOOL = msg_send![close, isHidden];
        if close_hidden {
            return 0.0;
        }

        let close_superview = close.superview();
        if close_superview.is_null() {
            return 0.0;
        }

        let title_bar_container_view = close_superview.superview();
        if title_bar_container_view.is_null() {
            return 0.0;
        }

        let title_bar_hidden: cocoa::base::BOOL = msg_send![title_bar_container_view, isHidden];
        if title_bar_hidden {
            return 0.0;
        }

        let close_rect: NSRect = msg_send![close, frame];
        let button_height = close_rect.size.height;
        let button_width = close_rect.size.width;

        let title_bar_frame_height = button_height + y;
        let mut title_bar_rect = NSView::frame(title_bar_container_view);
        title_bar_rect.size.height = title_bar_frame_height;
        title_bar_rect.origin.y = NSView::frame(ns_window).size.height - title_bar_frame_height;
        let _: () = msg_send![title_bar_container_view, setFrame: title_bar_rect];

        // `close` is non-null here (we returned 0.0 above if it was), so it
        // is always the first button. Only the other two need null checks.
        let mut window_buttons = vec![close];
        if !miniaturize.is_null() {
            window_buttons.push(miniaturize);
        }
        if !zoom.is_null() {
            window_buttons.push(zoom);
        }

        let space_between = 20.0; // Fixed space between buttons

        let last_index = window_buttons.len() - 1;
        let mut last_button_width = button_width;
        for (i, button) in window_buttons.into_iter().enumerate() {
            let mut rect: NSRect = NSView::frame(button);
            rect.origin.x = x + (i as f64 * space_between);
            // Vertically center the button within the titlebar container.
            // The container uses standard (non-flipped) AppKit coordinates,
            // so origin.y is measured from the container's bottom edge.
            rect.origin.y = (title_bar_frame_height - button_height) / 2.0;
            button.setFrameOrigin(rect.origin);
            if i == last_index {
                last_button_width = rect.size.width;
            }
        }

        // Right edge of the last button = its origin x + its own width.
        // Use the last button's measured width rather than assuming all
        // buttons share the close button's width.
        x + (last_index as f64 * space_between) + last_button_width
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct WindowState<R: Runtime> {
    window: Window<R>,
    ns_window: UnsafeWindowHandle,
    traffic_light_x: f64,
    traffic_light_y: f64,
    last_normal_cluster_right_edge: Arc<Mutex<f64>>,
    frontend_ready: Arc<Mutex<bool>>,
}

#[cfg(target_os = "macos")]
fn remember_normal_cluster_right_edge(
    last_normal_cluster_right_edge: &Arc<Mutex<f64>>,
    measured_cluster_right_edge: f64,
) -> f64 {
    if measured_cluster_right_edge > 0.0 {
        if let Ok(mut last) = last_normal_cluster_right_edge.lock() {
            *last = measured_cluster_right_edge;
        }
        measured_cluster_right_edge
    } else {
        last_normal_cluster_right_edge
            .lock()
            .map(|last| *last)
            .unwrap_or(0.0)
    }
}

#[cfg(target_os = "macos")]
fn refresh_traffic_lights(
    ns_window: UnsafeWindowHandle,
    traffic_light_x: f64,
    traffic_light_y: f64,
    last_normal_cluster_right_edge: &Arc<Mutex<f64>>,
    context: &str,
) -> f64 {
    match traffic_light_refresh_action(traffic_light_window_state(ns_window)) {
        TrafficLightRefreshAction::ExposeCollapsed => {
            let _ = context;
            0.0
        }
        TrafficLightRefreshAction::PositionAndExpose => {
            let measured_cluster_right_edge =
                position_traffic_lights(ns_window, traffic_light_x, traffic_light_y);
            remember_normal_cluster_right_edge(
                last_normal_cluster_right_edge,
                measured_cluster_right_edge,
            )
        }
    }
}

#[cfg(target_os = "macos")]
fn refresh_traffic_lights_for<R: Runtime>(state: &WindowState<R>, context: &str) {
    let _ = refresh_traffic_lights(
        state.ns_window,
        state.traffic_light_x,
        state.traffic_light_y,
        &state.last_normal_cluster_right_edge,
        context,
    );
}

#[cfg(target_os = "macos")]
fn with_wry_window_state<T>(
    ns_window_handle: UnsafeWindowHandle,
    func: impl FnOnce(&mut WindowState<tauri::Wry>) -> T,
) -> Option<T> {
    use objc::runtime::Object;
    use std::ffi::c_void;

    // SAFETY: `setup_traffic_light_positioner` rejects any runtime whose
    // TypeId is not `tauri::Wry`, so the `WindowState<R>` stored in `app_box`
    // is always a `WindowState<tauri::Wry>` in practice. The public
    // `WebviewWindowExt` trait is likewise implemented only for
    // `WebviewWindow` (which defaults to `Wry`), so every reader reaches the
    // state through a Wry-typed entry point. Keep this in one helper so the
    // assumption is not duplicated across call sites.
    unsafe {
        let ns_win = ns_window_handle.0 as cocoa::base::id;
        let delegate: *mut Object = msg_send![ns_win, delegate];
        if delegate.is_null() {
            return None;
        }

        let app_box: *mut c_void =
            match std::panic::catch_unwind(|| *(*delegate).get_ivar::<*mut c_void>("app_box")) {
                Ok(ptr) if !ptr.is_null() => ptr,
                _ => return None,
            };

        let state: &mut WindowState<tauri::Wry> = &mut *(app_box as *mut WindowState<tauri::Wry>);
        Some(func(state))
    }
}

#[cfg(target_os = "macos")]
fn current_cached_cluster_right_edge(last_normal_cluster_right_edge: &Arc<Mutex<f64>>) -> f64 {
    last_normal_cluster_right_edge
        .lock()
        .map(|last| *last)
        .unwrap_or(0.0)
}

#[cfg(target_os = "macos")]
pub fn inject_traffic_light_css_updater<R: Runtime>(webview: &Webview<R>) {
    let script = Arc::new(Mutex::new(None));
    let script_out = script.clone();

    if let Err(e) = webview.with_webview(move |platform_webview| {
        let ns_window = UnsafeWindowHandle(platform_webview.ns_window());
        let script = with_wry_window_state(ns_window, |state| {
            if let Ok(mut frontend_ready) = state.frontend_ready.lock() {
                *frontend_ready = true;
            }

            let cluster_right_edge = refresh_traffic_lights(
                state.ns_window,
                state.traffic_light_x,
                state.traffic_light_y,
                &state.last_normal_cluster_right_edge,
                "page load",
            );
            let cluster_right_edge = if cluster_right_edge > 0.0 {
                cluster_right_edge
            } else {
                current_cached_cluster_right_edge(&state.last_normal_cluster_right_edge)
            };

            traffic_light_left_css_updater_script(cluster_right_edge)
        });

        if let Ok(mut slot) = script_out.lock() {
            *slot = script;
        }
    }) {
        eprintln!(
            "decoration: failed to read native traffic-light state on page load: {:?}",
            e
        );
    }

    let script = script.lock().ok().and_then(|mut script| script.take());
    if let Some(script) = script {
        if let Err(e) = webview.eval(script) {
            eprintln!(
                "decoration: failed to install traffic-light CSS updater: {:?}",
                e
            );
        }
    }
}

#[cfg(target_os = "macos")]
pub fn setup_traffic_light_positioner<R: Runtime>(window: Window<R>) {
    use cocoa::appkit::{NSWindow, NSWindowButton};
    use cocoa::base::{id, BOOL};
    use cocoa::foundation::NSUInteger;
    use objc::runtime::{Object, Sel};
    use std::any::TypeId;
    use std::ffi::c_void;

    // The delegate state is read back through `with_wry_window_state`, which
    // casts `app_box` to `WindowState<tauri::Wry>`. That cast is only sound
    // when the stored runtime is Wry, so reject any other runtime here.
    // Tauri ships only the Wry runtime and `WebviewWindowExt` is implemented
    // for `WebviewWindow` (which defaults to Wry), so this guard is a
    // belt-and-suspenders safety check rather than a practical restriction.
    if TypeId::of::<R>() != TypeId::of::<tauri::Wry>() {
        eprintln!(
            "decoration: this plugin only supports the default Wry runtime on macOS; skipping traffic-light positioner"
        );
        return;
    }

    let ns_window = match window.ns_window() {
        Ok(win) => UnsafeWindowHandle(win),
        Err(e) => {
            eprintln!(
                "decoration: failed to get ns_window to mount delegate: {:?}",
                e
            );
            return;
        }
    };

    // Check if this window has traffic lights before setting up positioning.
    unsafe {
        let ns_win = ns_window.0 as id;
        // Quick check: if close button doesn't exist, this window probably doesn't have decorations
        let close = ns_win.standardWindowButton_(NSWindowButton::NSWindowCloseButton);
        if close.is_null() {
            return;
        }
    }

    let stored_inset = take_pending_traffic_light_inset(ns_window).unwrap_or(TrafficLightInset {
        x: WINDOW_CONTROL_PAD_X,
        y: WINDOW_CONTROL_PAD_Y,
        last_normal_cluster_right_edge: 0.0,
    });
    let last_normal_cluster_right_edge =
        Arc::new(Mutex::new(stored_inset.last_normal_cluster_right_edge));
    let frontend_ready = Arc::new(Mutex::new(false));

    // Do the initial positioning
    let _ = refresh_traffic_lights(
        ns_window,
        stored_inset.x,
        stored_inset.y,
        &last_normal_cluster_right_edge,
        "initial positioning",
    );

    // Ensure they stay in place while resizing the window.
    // Returns `None` (and skips `func`) if `app_box` has already been freed
    // by `on_window_will_close`, so post-close access is a no-op instead of UB.
    fn with_window_state<R: Runtime, F: FnOnce(&mut WindowState<R>) -> T, T>(
        this: &Object,
        func: F,
    ) -> Option<T> {
        let ptr = unsafe {
            let x: *mut c_void = *this.get_ivar("app_box");
            if x.is_null() {
                return None;
            }
            &mut *(x as *mut WindowState<R>)
        };
        Some(func(ptr))
    }

    unsafe {
        let ns_win = ns_window.0 as id;

        let current_delegate: id = ns_win.delegate();

        extern "C" fn on_window_should_close(this: &Object, _cmd: Sel, sender: id) -> BOOL {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                msg_send![super_del, windowShouldClose: sender]
            }
        }
        extern "C" fn on_window_will_close<R: Runtime>(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowWillClose: notification];

                // Free the heap-allocated `WindowState` so the `Window` clone,
                // the `Arc<Mutex<...>>` fields, and the box itself are dropped.
                // `windowWillClose` is the last delegate callback AppKit
                // delivers for a closing window, so the state is safe to
                // release here. Null the ivar so any later access through
                // `with_window_state` / `with_wry_window_state` is a no-op
                // rather than a use-after-free.
                let app_box: *mut c_void = *this.get_ivar::<*mut c_void>("app_box");
                if !app_box.is_null() {
                    let _ = Box::from_raw(app_box as *mut WindowState<R>);
                    // ObjC delegate receivers are mutable; the `&Object`
                    // binding is just the objc crate's convention. Cast
                    // through the raw pointer to write the ivar.
                    let this_mut = this as *const Object as *mut Object;
                    (*this_mut).set_ivar::<*mut c_void>("app_box", std::ptr::null_mut());
                }
            }
        }
        extern "C" fn on_window_did_resize<R: Runtime>(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidResize: notification];

                // Already on AppKit's main thread; refresh synchronously so
                // the traffic lights track the frame without run-loop lag.
                with_window_state(&*this, |state: &mut WindowState<R>| {
                    refresh_traffic_lights_for(state, "resize");
                });
            }
        }
        extern "C" fn on_window_did_move<R: Runtime>(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidMove: notification];

                with_window_state(&*this, |state: &mut WindowState<R>| {
                    refresh_traffic_lights_for(state, "move");
                });
            }
        }
        extern "C" fn on_window_did_change_backing_properties(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidChangeBackingProperties: notification];
            }
        }
        extern "C" fn on_window_did_become_key(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidBecomeKey: notification];
            }
        }
        extern "C" fn on_window_did_resign_key(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidResignKey: notification];
            }
        }
        extern "C" fn on_dragging_entered(this: &Object, _cmd: Sel, notification: id) -> BOOL {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                msg_send![super_del, draggingEntered: notification]
            }
        }
        extern "C" fn on_prepare_for_drag_operation(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) -> BOOL {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                msg_send![super_del, prepareForDragOperation: notification]
            }
        }
        extern "C" fn on_perform_drag_operation(this: &Object, _cmd: Sel, sender: id) -> BOOL {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                msg_send![super_del, performDragOperation: sender]
            }
        }
        extern "C" fn on_conclude_drag_operation(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, concludeDragOperation: notification];
            }
        }
        extern "C" fn on_dragging_exited(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, draggingExited: notification];
            }
        }
        extern "C" fn on_window_will_use_full_screen_presentation_options(
            this: &Object,
            _cmd: Sel,
            window: id,
            proposed_options: NSUInteger,
        ) -> NSUInteger {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                msg_send![super_del, window: window willUseFullScreenPresentationOptions: proposed_options]
            }
        }
        extern "C" fn on_window_did_enter_full_screen<R: Runtime>(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                with_window_state(&*this, |state: &mut WindowState<R>| {
                    if let Err(e) = state.window.emit("did-enter-fullscreen", ()) {
                        eprintln!("decoration: failed to emit did-enter-fullscreen: {:?}", e);
                    }
                    refresh_traffic_lights_for(state, "enter fullscreen");
                });

                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidEnterFullScreen: notification];
            }
        }
        extern "C" fn on_window_will_enter_full_screen<R: Runtime>(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                with_window_state(&*this, |state: &mut WindowState<R>| {
                    if let Err(e) = state.window.emit("will-enter-fullscreen", ()) {
                        eprintln!("decoration: failed to emit will-enter-fullscreen: {:?}", e);
                    }
                });

                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowWillEnterFullScreen: notification];
            }
        }
        extern "C" fn on_window_did_exit_full_screen<R: Runtime>(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                with_window_state(&*this, |state: &mut WindowState<R>| {
                    if let Err(e) = state.window.emit("did-exit-fullscreen", ()) {
                        eprintln!("decoration: failed to emit did-exit-fullscreen: {:?}", e);
                    }

                    refresh_traffic_lights_for(state, "exit fullscreen");
                });

                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidExitFullScreen: notification];
            }
        }
        extern "C" fn on_window_will_exit_full_screen<R: Runtime>(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                with_window_state(&*this, |state: &mut WindowState<R>| {
                    if let Err(e) = state.window.emit("will-exit-fullscreen", ()) {
                        eprintln!("decoration: failed to emit will-exit-fullscreen: {:?}", e);
                    }
                });

                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowWillExitFullScreen: notification];
            }
        }
        extern "C" fn on_window_did_fail_to_enter_full_screen(
            this: &Object,
            _cmd: Sel,
            window: id,
        ) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidFailToEnterFullScreen: window];
            }
        }
        extern "C" fn on_effective_appearance_did_change(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, effectiveAppearanceDidChange: notification];
            }
        }
        extern "C" fn on_effective_appearance_did_changed_on_main_thread(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![
                    super_del,
                    effectiveAppearanceDidChangedOnMainThread: notification
                ];
            }
        }

        // Are we deallocing this properly ? (I miss safe Rust :(  )
        let window_label = window.label().to_string();

        let app_state = WindowState {
            window,
            ns_window,
            traffic_light_x: stored_inset.x,
            traffic_light_y: stored_inset.y,
            last_normal_cluster_right_edge,
            frontend_ready,
        };
        let app_box = Box::into_raw(Box::new(app_state)) as *mut c_void;
        let random_str: String = rng()
            .sample_iter(Alphanumeric)
            .take(20)
            .map(char::from)
            .collect();

        // We need to ensure we have a unique delegate name, otherwise we will panic while trying to create a duplicate
        // delegate with the same name.
        let delegate_name = format!("windowDelegate_{}_{}", window_label, random_str);

        ns_win.setDelegate_(cocoa::delegate!(&delegate_name, {
            window: id = ns_win,
            app_box: *mut c_void = app_box,
            toolbar: id = cocoa::base::nil,
            super_delegate: id = current_delegate,
            (windowShouldClose:) => on_window_should_close as extern fn(&Object, Sel, id) -> BOOL,
            (windowWillClose:) => on_window_will_close::<R> as extern fn(&Object, Sel, id),
            (windowDidResize:) => on_window_did_resize::<R> as extern fn(&Object, Sel, id),
            (windowDidMove:) => on_window_did_move::<R> as extern fn(&Object, Sel, id),
            (windowDidChangeBackingProperties:) => on_window_did_change_backing_properties as extern fn(&Object, Sel, id),
            (windowDidBecomeKey:) => on_window_did_become_key as extern fn(&Object, Sel, id),
            (windowDidResignKey:) => on_window_did_resign_key as extern fn(&Object, Sel, id),
            (draggingEntered:) => on_dragging_entered as extern fn(&Object, Sel, id) -> BOOL,
            (prepareForDragOperation:) => on_prepare_for_drag_operation as extern fn(&Object, Sel, id) -> BOOL,
            (performDragOperation:) => on_perform_drag_operation as extern fn(&Object, Sel, id) -> BOOL,
            (concludeDragOperation:) => on_conclude_drag_operation as extern fn(&Object, Sel, id),
            (draggingExited:) => on_dragging_exited as extern fn(&Object, Sel, id),
            (window:willUseFullScreenPresentationOptions:) => on_window_will_use_full_screen_presentation_options as extern fn(&Object, Sel, id, NSUInteger) -> NSUInteger,
            (windowDidEnterFullScreen:) => on_window_did_enter_full_screen::<R> as extern fn(&Object, Sel, id),
            (windowWillEnterFullScreen:) => on_window_will_enter_full_screen::<R> as extern fn(&Object, Sel, id),
            (windowDidExitFullScreen:) => on_window_did_exit_full_screen::<R> as extern fn(&Object, Sel, id),
            (windowWillExitFullScreen:) => on_window_will_exit_full_screen::<R> as extern fn(&Object, Sel, id),
            (windowDidFailToEnterFullScreen:) => on_window_did_fail_to_enter_full_screen as extern fn(&Object, Sel, id),
            (effectiveAppearanceDidChange:) => on_effective_appearance_did_change as extern fn(&Object, Sel, id),
            (effectiveAppearanceDidChangedOnMainThread:) => on_effective_appearance_did_changed_on_main_thread as extern fn(&Object, Sel, id)
        }))
    }
}

#[cfg(target_os = "macos")]
pub fn update_traffic_light_positions(
    ns_window_handle: UnsafeWindowHandle,
    x: f64,
    y: f64,
    measured_cluster_right_edge: f64,
) -> (bool, f64) {
    let update = with_wry_window_state(ns_window_handle, |state| {
        state.traffic_light_x = x;
        state.traffic_light_y = y;
        let cluster_right_edge = remember_normal_cluster_right_edge(
            &state.last_normal_cluster_right_edge,
            measured_cluster_right_edge,
        );
        let frontend_ready = state
            .frontend_ready
            .lock()
            .map(|frontend_ready| *frontend_ready)
            .unwrap_or(false);
        (frontend_ready, cluster_right_edge)
    });

    if let Some(update) = update {
        clear_pending_traffic_light_inset(ns_window_handle);
        update
    } else {
        let stored = remember_pending_traffic_light_inset(
            ns_window_handle,
            x,
            y,
            measured_cluster_right_edge,
        );
        (false, stored.last_normal_cluster_right_edge)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        remember_normal_cluster_right_edge, remember_pending_traffic_light_inset,
        take_pending_traffic_light_inset, traffic_light_left_clearance,
        traffic_light_left_css_script, traffic_light_left_css_updater_script,
        traffic_light_refresh_action, TrafficLightRefreshAction, TrafficLightWindowState,
        UnsafeWindowHandle,
    };
    use std::sync::{Arc, Mutex};

    #[test]
    fn traffic_light_clearance_adds_content_gap_when_buttons_exist() {
        assert_eq!(traffic_light_left_clearance(64.0), 72.0);
    }

    #[test]
    fn traffic_light_clearance_collapses_when_buttons_are_missing() {
        assert_eq!(traffic_light_left_clearance(0.0), 0.0);
    }

    #[test]
    fn traffic_light_css_script_sets_zero_padding_when_buttons_are_missing() {
        assert_eq!(
            traffic_light_left_css_script(0.0),
            "document.documentElement.style.setProperty('--decoration-traffic-light-left','0px')"
        );
    }

    #[test]
    fn traffic_light_css_script_includes_gap_when_buttons_exist() {
        assert_eq!(
            traffic_light_left_css_script(64.0),
            "document.documentElement.style.setProperty('--decoration-traffic-light-left','72px')"
        );
    }

    #[test]
    fn traffic_light_refresh_positions_native_controls_when_window_is_not_fullscreen() {
        assert_eq!(
            traffic_light_refresh_action(TrafficLightWindowState {
                is_fullscreen: false,
            }),
            TrafficLightRefreshAction::PositionAndExpose
        );
    }

    #[test]
    fn traffic_light_refresh_skips_native_positioning_while_window_is_fullscreen() {
        assert_eq!(
            traffic_light_refresh_action(TrafficLightWindowState {
                is_fullscreen: true,
            }),
            TrafficLightRefreshAction::ExposeCollapsed
        );
    }

    #[test]
    fn normal_refresh_uses_last_measured_width_when_controls_are_transiently_hidden() {
        let last = Arc::new(Mutex::new(64.0));

        assert_eq!(remember_normal_cluster_right_edge(&last, 0.0), 64.0);
        assert_eq!(remember_normal_cluster_right_edge(&last, 80.0), 80.0);
        assert_eq!(remember_normal_cluster_right_edge(&last, 0.0), 80.0);
    }

    #[test]
    fn pending_inset_preserves_custom_position_until_delegate_mounts() {
        let window = UnsafeWindowHandle(0xdec0deusize as *mut std::ffi::c_void);

        let stored = remember_pending_traffic_light_inset(window, 16.0, 20.0, 64.0);
        assert_eq!(stored.x, 16.0);
        assert_eq!(stored.y, 20.0);
        assert_eq!(stored.last_normal_cluster_right_edge, 64.0);

        let stored = remember_pending_traffic_light_inset(window, 18.0, 22.0, 0.0);
        assert_eq!(stored.x, 18.0);
        assert_eq!(stored.y, 22.0);
        assert_eq!(stored.last_normal_cluster_right_edge, 64.0);

        let stored = take_pending_traffic_light_inset(window).unwrap();
        assert_eq!(stored.x, 18.0);
        assert_eq!(stored.y, 22.0);
        assert_eq!(stored.last_normal_cluster_right_edge, 64.0);
        assert!(take_pending_traffic_light_inset(window).is_none());
    }

    #[test]
    fn traffic_light_css_updater_collapses_only_for_fullscreen_and_uses_will_events() {
        let script = traffic_light_left_css_updater_script(64.0);

        assert!(script.contains("--decoration-traffic-light-left"));
        assert!(script.contains("const normalLeft = 72"));
        assert!(!script.contains("isMaximized"));
        assert!(script.contains("isFullscreen"));
        assert!(script.contains("\"will-enter-fullscreen\""));
        assert!(script.contains("\"will-exit-fullscreen\""));
        assert!(script.contains("onResized"));
        assert!(script.contains("onMoved"));
        assert!(script.contains("collapsed ? 0 : normalLeft"));
        // The post-await re-check is what prevents a stale isFullscreen()
        // result from overwriting a will-enter/will-exit update that landed
        // while the IPC round-trip was in flight.
        assert!(script.contains("Re-check the transition after the await"));
        assert!(script.contains("maximize (the green zoom button"));
    }
}
