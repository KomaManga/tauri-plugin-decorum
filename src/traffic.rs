// Most contents of this file are taken from Hoppscotch's tauri app.
// I think there is work to be done to improve it, but I'm happy with it for now.
// Reference source code is linked below.
// https://github.com/hoppscotch/hoppscotch/blob/286fcd2bb08a84f027b10308d1e18da368f95ebf/packages/hoppscotch-selfhost-desktop/src-tauri/src/mac/window.rs

use objc::{msg_send, sel, sel_impl};
use rand::{distributions::Alphanumeric, Rng, rng};
use tauri::{Emitter, Runtime, Window};

const WINDOW_CONTROL_PAD_X: f64 = 12.0;
const WINDOW_CONTROL_PAD_Y: f64 = 16.0;

pub struct UnsafeWindowHandle(pub *mut std::ffi::c_void);
unsafe impl Send for UnsafeWindowHandle {}
unsafe impl Sync for UnsafeWindowHandle {}

#[cfg(target_os = "macos")]
pub fn position_traffic_lights(ns_window_handle: UnsafeWindowHandle, x: f64, y: f64) {
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
            return;
        }
        
        let close_superview = close.superview();
        if close_superview.is_null() {
            return;
        }
        
        let title_bar_container_view = close_superview.superview();
        if title_bar_container_view.is_null() {
            return;
        }

        let close_rect: NSRect = msg_send![close, frame];
        let button_height = close_rect.size.height;

        let title_bar_frame_height = button_height + y;
        let mut title_bar_rect = NSView::frame(title_bar_container_view);
        title_bar_rect.size.height = title_bar_frame_height;
        title_bar_rect.origin.y = NSView::frame(ns_window).size.height - title_bar_frame_height;
        let _: () = msg_send![title_bar_container_view, setFrame: title_bar_rect];

        // Check if other buttons exist before using them
        let mut window_buttons = Vec::new();
        if !close.is_null() {
            window_buttons.push(close);
        }
        if !miniaturize.is_null() {
            window_buttons.push(miniaturize);
        }
        if !zoom.is_null() {
            window_buttons.push(zoom);
        }
        
        if window_buttons.is_empty() {
            return;
        }
        
        let space_between = 20.0; // Fixed space between buttons
        let vertical_offset = 4.0; // Adjust this value to push buttons down

        for (i, button) in window_buttons.into_iter().enumerate() {
            let mut rect: NSRect = NSView::frame(button);
            rect.origin.x = x + (i as f64 * space_between);
            // Adjust vertical positioning
            rect.origin.y = ((title_bar_frame_height - button_height) / 2.0) - vertical_offset;
            button.setFrameOrigin(rect.origin);
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct WindowState<R: Runtime> {
    window: Window<R>,
    traffic_light_x: f64,
    traffic_light_y: f64,
}

#[cfg(target_os = "macos")]
pub fn setup_traffic_light_positioner<R: Runtime>(window: Window<R>) {
    use cocoa::appkit::{NSWindow, NSWindowButton};
    use cocoa::base::{id, BOOL};
    use cocoa::foundation::NSUInteger;
    use objc::runtime::{Object, Sel};
    use std::ffi::c_void;

    // Check if this window has traffic lights before setting up positioning
    unsafe {
        let ns_win = match window.ns_window() {
            Ok(win) => win as id,
            Err(_) => return,
        };
        
        // Quick check: if close button doesn't exist, this window probably doesn't have decorations
        let close = ns_win.standardWindowButton_(NSWindowButton::NSWindowCloseButton);
        if close.is_null() {
            return;
        }
    }

    // Do the initial positioning
    let ns_window = match window.ns_window() {
        Ok(win) => win,
        Err(e) => {
            eprintln!("decorum: failed to get ns_window for initial positioning: {:?}", e);
            return;
        }
    };
    position_traffic_lights(
        UnsafeWindowHandle(ns_window),
        WINDOW_CONTROL_PAD_X,
        WINDOW_CONTROL_PAD_Y,
    );

    // Ensure they stay in place while resizing the window.
    fn with_window_state<R: Runtime, F: FnOnce(&mut WindowState<R>) -> T, T>(
        this: &Object,
        func: F,
    ) {
        let ptr = unsafe {
            let x: *mut c_void = *this.get_ivar("app_box");
            &mut *(x as *mut WindowState<R>)
        };
        func(ptr);
    }

    unsafe {
        let ns_win = match window.ns_window() {
            Ok(win) => win as id,
            Err(e) => {
                eprintln!("decorum: failed to get ns_window to mount delegate: {:?}", e);
                return;
            }
        };

        let current_delegate: id = ns_win.delegate();

        extern "C" fn on_window_should_close(this: &Object, _cmd: Sel, sender: id) -> BOOL {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                msg_send![super_del, windowShouldClose: sender]
            }
        }
        extern "C" fn on_window_will_close(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowWillClose: notification];
            }
        }
        extern "C" fn on_window_did_resize<R: Runtime>(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                with_window_state(&*this, |state: &mut WindowState<R>| {
                    // Guard against ns_window() failure — panicking inside an
                    // Objective-C delegate callback aborts the process or is
                    // silently swallowed, which stops controls from rendering
                    // (see GitHub issue #53).
                    let ns_win = match state.window.ns_window() {
                        Ok(win) => win as id,
                        Err(e) => {
                            eprintln!("decorum: failed to get ns_window on resize: {:?}", e);
                            return;
                        }
                    };

                    #[cfg(target_os = "macos")]
                    position_traffic_lights(
                        UnsafeWindowHandle(ns_win as *mut std::ffi::c_void),
                        state.traffic_light_x,
                        state.traffic_light_y,
                    );
                });

                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidResize: notification];
            }
        }
        extern "C" fn on_window_did_move(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidMove: notification];
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
                        eprintln!("decorum: failed to emit did-enter-fullscreen: {:?}", e);
                    }
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
                        eprintln!("decorum: failed to emit will-enter-fullscreen: {:?}", e);
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
                        eprintln!("decorum: failed to emit did-exit-fullscreen: {:?}", e);
                    }

                    let ns_win = match state.window.ns_window() {
                        Ok(win) => win as id,
                        Err(e) => {
                            eprintln!("decorum: failed to get ns_window on exit fullscreen: {:?}", e);
                            return;
                        }
                    };
                    position_traffic_lights(
                        UnsafeWindowHandle(ns_win as *mut std::ffi::c_void),
                        state.traffic_light_x,
                        state.traffic_light_y,
                    );
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
                        eprintln!("decorum: failed to emit will-exit-fullscreen: {:?}", e);
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
            traffic_light_x: WINDOW_CONTROL_PAD_X,
            traffic_light_y: WINDOW_CONTROL_PAD_Y,
        };
        let app_box = Box::into_raw(Box::new(app_state)) as *mut c_void;
        let random_str: String = rng()
            .sample_iter(&Alphanumeric)
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
            (windowWillClose:) => on_window_will_close as extern fn(&Object, Sel, id),
            (windowDidResize:) => on_window_did_resize::<R> as extern fn(&Object, Sel, id),
            (windowDidMove:) => on_window_did_move as extern fn(&Object, Sel, id),
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
pub fn update_traffic_light_positions(window: &tauri::WebviewWindow, x: f64, y: f64) {
    use objc::runtime::Object;
    use std::ffi::c_void;
    use tauri::Wry;

    // SAFETY: The `WebviewWindowExt` trait (which is the only caller of this
    // function via `set_traffic_lights_inset`) is implemented exclusively for
    // `WebviewWindow`, which is a type alias for `WebviewWindow<Wry>`.
    // Therefore the delegate's `WindowState<R>` was always created with
    // `R = Wry` in practice, making this cast sound.  If the plugin is ever
    // generalised to custom runtimes, the positions must be stored in a
    // runtime-erased way instead.
    unsafe {
        let ns_win = match window.ns_window() {
            Ok(win) => win as cocoa::base::id,
            Err(_) => return,
        };

        let delegate: *mut Object = msg_send![ns_win, delegate];
        if delegate.is_null() {
            return;
        }

        // Try to access the ivar directly with proper type annotation
        let app_box: *mut c_void = match std::panic::catch_unwind(|| {
            *(*delegate).get_ivar::<*mut c_void>("app_box")
        }) {
            Ok(ptr) if !ptr.is_null() => ptr,
            _ => return, // Either the ivar doesn't exist or it's null
        };

        let state: &mut WindowState<Wry> = &mut *(app_box as *mut WindowState<Wry>);
        state.traffic_light_x = x;
        state.traffic_light_y = y;
    }
}
