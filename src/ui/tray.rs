#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrayAction {
    Show,
    OpenBrowser,
    SoftUnload,
    StopAll,
    Quit,
}

#[cfg(windows)]
mod imp {
    use super::TrayAction;
    use eframe::egui;
    use std::ffi::c_void;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;

    const WM_USER: u32 = 0x0400;
    const WM_TRAY_ICON: u32 = WM_USER + 1;
    const WM_TRAY_EXIT: u32 = WM_USER + 2;
    const WM_DESTROY: u32 = 0x0002;
    const WM_LBUTTONUP: isize = 0x0202;
    const WM_LBUTTONDBLCLK: isize = 0x0203;
    const WM_RBUTTONUP: isize = 0x0205;
    const WM_CONTEXTMENU: isize = 0x007B;

    const NIM_ADD: u32 = 0x0000_0000;
    const NIM_DELETE: u32 = 0x0000_0002;
    const NIF_MESSAGE: u32 = 0x0000_0001;
    const NIF_ICON: u32 = 0x0000_0002;
    const NIF_TIP: u32 = 0x0000_0004;

    const MF_STRING: u32 = 0x0000_0000;
    const MF_SEPARATOR: u32 = 0x0000_0800;
    const TPM_RIGHTALIGN: u32 = 0x0008;
    const TPM_BOTTOMALIGN: u32 = 0x0020;
    const TPM_RETURNCMD: u32 = 0x0100;

    const IDM_SHOW: usize = 1001;
    const IDM_BROWSER: usize = 1002;
    const IDM_SOFT_UNLOAD: usize = 1003;
    const IDM_STOP_ALL: usize = 1004;
    const IDM_QUIT: usize = 1005;

    const IDI_APPLICATION: usize = 32512;
    const HWND_MESSAGE: *mut c_void = (-3isize) as *mut c_void;

    #[repr(C)]
    struct Guid {
        d1: u32,
        d2: u16,
        d3: u16,
        d4: [u8; 8],
    }

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct Msg {
        hwnd: *mut c_void,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        pt: Point,
    }

    #[repr(C)]
    struct WndClassW {
        style: u32,
        lpfn_wnd_proc: unsafe extern "system" fn(*mut c_void, u32, usize, isize) -> isize,
        cb_cls_extra: i32,
        cb_wnd_extra: i32,
        h_instance: *mut c_void,
        h_icon: *mut c_void,
        h_cursor: *mut c_void,
        hbr_background: *mut c_void,
        lpsz_menu_name: *const u16,
        lpsz_class_name: *const u16,
    }

    #[repr(C)]
    struct NotifyIconDataW {
        cb_size: u32,
        h_wnd: *mut c_void,
        u_id: u32,
        u_flags: u32,
        u_callback_message: u32,
        h_icon: *mut c_void,
        sz_tip: [u16; 128],
        dw_state: u32,
        dw_state_mask: u32,
        sz_info: [u16; 256],
        u_timeout_or_version: u32,
        sz_info_title: [u16; 64],
        dw_info_flags: u32,
        guid_item: Guid,
        h_balloon_icon: *mut c_void,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(lp_module_name: *const u16) -> *mut c_void;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterClassW(lp_wnd_class: *const WndClassW) -> u16;
        fn CreateWindowExW(
            dw_ex_style: u32,
            lp_class_name: *const u16,
            lp_window_name: *const u16,
            dw_style: u32,
            x: i32,
            y: i32,
            n_width: i32,
            n_height: i32,
            h_wnd_parent: *mut c_void,
            h_menu: *mut c_void,
            h_instance: *mut c_void,
            lp_param: *mut c_void,
        ) -> *mut c_void;
        fn DefWindowProcW(hwnd: *mut c_void, msg: u32, w_param: usize, l_param: isize) -> isize;
        fn GetMessageW(lp_msg: *mut Msg, h_wnd: *mut c_void, w_min: u32, w_max: u32) -> i32;
        fn TranslateMessage(lp_msg: *const Msg) -> i32;
        fn DispatchMessageW(lp_msg: *const Msg) -> isize;
        fn PostQuitMessage(n_exit_code: i32);
        fn PostMessageW(h_wnd: *mut c_void, msg: u32, w_param: usize, l_param: isize) -> i32;
        fn DestroyWindow(h_wnd: *mut c_void) -> i32;
        fn CreatePopupMenu() -> *mut c_void;
        fn AppendMenuW(
            h_menu: *mut c_void,
            u_flags: u32,
            u_id_new_item: usize,
            lp_new_item: *const u16,
        ) -> i32;
        fn TrackPopupMenu(
            h_menu: *mut c_void,
            u_flags: u32,
            x: i32,
            y: i32,
            n_reserved: i32,
            h_wnd: *mut c_void,
            prc_rect: *const c_void,
        ) -> i32;
        fn DestroyMenu(h_menu: *mut c_void) -> i32;
        fn SetForegroundWindow(h_wnd: *mut c_void) -> i32;
        fn GetCursorPos(lp_point: *mut Point) -> i32;
        fn LoadIconW(h_instance: *mut c_void, lp_icon_name: *const u16) -> *mut c_void;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn Shell_NotifyIconW(dw_message: u32, lp_data: *mut NotifyIconDataW) -> i32;
    }

    struct Bridge {
        actions: Arc<Mutex<Vec<TrayAction>>>,
        ctx: egui::Context,
    }

    static TRAY_BRIDGE: OnceLock<Mutex<Option<Bridge>>> = OnceLock::new();

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn emit_action(action: TrayAction) {
        if let Some(cell) = TRAY_BRIDGE.get()
            && let Ok(guard) = cell.lock()
            && let Some(bridge) = guard.as_ref()
        {
            if let Ok(mut q) = bridge.actions.lock() {
                q.push(action);
            }
            if action == TrayAction::Show {
                bridge
                    .ctx
                    .send_viewport_cmd(egui::ViewportCommand::Visible(true));
                bridge
                    .ctx
                    .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                bridge
                    .ctx
                    .send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            bridge.ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }

    unsafe fn show_context_menu(hwnd: *mut c_void) {
        unsafe {
            let hmenu = CreatePopupMenu();
            if hmenu.is_null() {
                return;
            }
            let lbl_show = to_wide("Show AI Control Center");
            let lbl_browser = to_wide("Open Frontend in Browser");
            let lbl_unload = to_wide("Soft Unload VRAM (llama-swap)");
            let lbl_stop = to_wide("Stop All Services");
            let lbl_quit = to_wide("Quit AI Control Center");

            AppendMenuW(hmenu, MF_STRING, IDM_SHOW, lbl_show.as_ptr());
            AppendMenuW(hmenu, MF_STRING, IDM_BROWSER, lbl_browser.as_ptr());
            AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(hmenu, MF_STRING, IDM_SOFT_UNLOAD, lbl_unload.as_ptr());
            AppendMenuW(hmenu, MF_STRING, IDM_STOP_ALL, lbl_stop.as_ptr());
            AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(hmenu, MF_STRING, IDM_QUIT, lbl_quit.as_ptr());

            let mut pt = Point { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            SetForegroundWindow(hwnd);
            let cmd = TrackPopupMenu(
                hmenu,
                TPM_RIGHTALIGN | TPM_BOTTOMALIGN | TPM_RETURNCMD,
                pt.x,
                pt.y,
                0,
                hwnd,
                std::ptr::null(),
            ) as usize;
            DestroyMenu(hmenu);

            match cmd {
                IDM_SHOW => emit_action(TrayAction::Show),
                IDM_BROWSER => emit_action(TrayAction::OpenBrowser),
                IDM_SOFT_UNLOAD => emit_action(TrayAction::SoftUnload),
                IDM_STOP_ALL => emit_action(TrayAction::StopAll),
                IDM_QUIT => emit_action(TrayAction::Quit),
                _ => {}
            }
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: *mut c_void,
        msg: u32,
        w_param: usize,
        l_param: isize,
    ) -> isize {
        unsafe {
            match msg {
                WM_TRAY_ICON => {
                    let evt = l_param & 0xFFFF;
                    if evt == WM_LBUTTONUP || evt == WM_LBUTTONDBLCLK {
                        emit_action(TrayAction::Show);
                    } else if evt == WM_RBUTTONUP || evt == WM_CONTEXTMENU {
                        show_context_menu(hwnd);
                    }
                    0
                }
                WM_TRAY_EXIT => {
                    DestroyWindow(hwnd);
                    0
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, w_param, l_param),
            }
        }
    }

    pub(crate) struct TrayController {
        actions: Arc<Mutex<Vec<TrayAction>>>,
        hwnd_raw: Arc<Mutex<usize>>,
        active: Arc<Mutex<bool>>,
    }

    impl TrayController {
        pub(crate) fn start(ctx: egui::Context, tooltip: String) -> Self {
            let actions = Arc::new(Mutex::new(Vec::new()));
            let hwnd_raw = Arc::new(Mutex::new(0usize));
            let active = Arc::new(Mutex::new(false));

            let cell = TRAY_BRIDGE.get_or_init(|| Mutex::new(None));
            if let Ok(mut guard) = cell.lock() {
                *guard = Some(Bridge {
                    actions: Arc::clone(&actions),
                    ctx,
                });
            }

            let hwnd_slot = Arc::clone(&hwnd_raw);
            let active_slot = Arc::clone(&active);

            thread::spawn(move || unsafe {
                let h_instance = GetModuleHandleW(std::ptr::null());
                let class_name = to_wide("AiControlCenterTrayClass");
                let wc = WndClassW {
                    style: 0,
                    lpfn_wnd_proc: wnd_proc,
                    cb_cls_extra: 0,
                    cb_wnd_extra: 0,
                    h_instance,
                    h_icon: std::ptr::null_mut(),
                    h_cursor: std::ptr::null_mut(),
                    hbr_background: std::ptr::null_mut(),
                    lpsz_menu_name: std::ptr::null(),
                    lpsz_class_name: class_name.as_ptr(),
                };
                RegisterClassW(&wc);

                let hwnd = CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    class_name.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    std::ptr::null_mut(),
                    h_instance,
                    std::ptr::null_mut(),
                );
                if hwnd.is_null() {
                    return;
                }
                if let Ok(mut h) = hwnd_slot.lock() {
                    *h = hwnd as usize;
                }

                let mut h_icon = LoadIconW(h_instance, std::ptr::dangling::<u16>());
                if h_icon.is_null() {
                    h_icon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION as *const u16);
                }

                let mut tip = [0u16; 128];
                for (i, ch) in tooltip.encode_utf16().take(127).enumerate() {
                    tip[i] = ch;
                }

                let mut nid = NotifyIconDataW {
                    cb_size: std::mem::size_of::<NotifyIconDataW>() as u32,
                    h_wnd: hwnd,
                    u_id: 1,
                    u_flags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                    u_callback_message: WM_TRAY_ICON,
                    h_icon,
                    sz_tip: tip,
                    dw_state: 0,
                    dw_state_mask: 0,
                    sz_info: [0u16; 256],
                    u_timeout_or_version: 0,
                    sz_info_title: [0u16; 64],
                    dw_info_flags: 0,
                    guid_item: Guid {
                        d1: 0,
                        d2: 0,
                        d3: 0,
                        d4: [0u8; 8],
                    },
                    h_balloon_icon: std::ptr::null_mut(),
                };

                if Shell_NotifyIconW(NIM_ADD, &mut nid) != 0
                    && let Ok(mut a) = active_slot.lock()
                {
                    *a = true;
                }

                let mut msg = Msg {
                    hwnd: std::ptr::null_mut(),
                    message: 0,
                    w_param: 0,
                    l_param: 0,
                    time: 0,
                    pt: Point { x: 0, y: 0 },
                };
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                Shell_NotifyIconW(NIM_DELETE, &mut nid);
                if let Ok(mut a) = active_slot.lock() {
                    *a = false;
                }
            });

            Self {
                actions,
                hwnd_raw,
                active,
            }
        }

        pub(crate) fn is_active(&self) -> bool {
            self.active.lock().map(|g| *g).unwrap_or(false)
        }

        pub(crate) fn poll_actions(&self) -> Vec<TrayAction> {
            if let Ok(mut q) = self.actions.lock() {
                std::mem::take(&mut *q)
            } else {
                Vec::new()
            }
        }
    }

    impl Drop for TrayController {
        fn drop(&mut self) {
            if let Ok(h) = self.hwnd_raw.lock()
                && *h != 0
            {
                unsafe {
                    PostMessageW(*h as *mut c_void, WM_TRAY_EXIT, 0, 0);
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::TrayAction;
    use eframe::egui;

    pub(crate) struct TrayController;

    impl TrayController {
        pub(crate) fn start(_ctx: egui::Context, _tooltip: String) -> Self {
            Self
        }

        pub(crate) fn is_active(&self) -> bool {
            false
        }

        pub(crate) fn poll_actions(&self) -> Vec<TrayAction> {
            Vec::new()
        }
    }
}

pub(crate) use imp::TrayController;
