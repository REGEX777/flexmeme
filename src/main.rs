mod db;
slint::include_modules!();

use image::ImageReader;
use slint::{Image, ModelRc, SharedString, VecModel};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::{HotKey, Modifiers, Code}};
use std::{path::Path, time::Duration};
use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
// use arboard::Clipboard;
use std::sync::mpsc;
use enigo::{Enigo, Keyboard, Key, Settings, Direction};
use clipboard_win::{Clipboard, Setter, formats};
use uiautomation::{types::{ControlType, Handle}, UIAutomation, UIElement};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_SETSCREENREADER, SPIF_SENDCHANGE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, GetForegroundWindow
};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use std::rc::Rc;
use uiautomation::events::{CustomFocusChangedEventHandler, UIFocusChangedEventHandler};
use std::{collections::HashMap, sync::{Arc, Mutex}};
use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO, GetWindowThreadProcessId};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use tray_icon::{TrayIconBuilder, Icon, menu::{Menu, MenuItem, MenuEvent}};

fn set_screen_reader_flag(on: bool) {
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETSCREENREADER, on as u32, None, SPIF_SENDCHANGE);
    }
}


fn make_non_activating(){
    let hwnd = match unsafe { FindWindowW(PCWSTR::null(), w!("flexmeme-popup")) } {
        Ok(h) => h,
        Err(e) => {
            eprintln!("popup window not found: {:?}", e);
            return;
        }
    };

    unsafe {
        let old = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE,
            old | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize);
    }
}


fn show_popup(ui: &Picker, manager: &GlobalHotKeyManager, nav: &[HotKey]) {
    if let Err(e) = manager.register_all(nav) {
        eprintln!("registering nav keys failed: {:?}", e);
    }
    ui.set_selected_index(0);
    ui.show().ok();
}

fn hide_popup(ui: &Picker, manager: &GlobalHotKeyManager, nav: &[HotKey]) {
    manager.unregister_all(nav).ok();   // ALWAYS give the keys back brochacho mueheheh
    ui.hide().ok();
}

fn paste_meme(path: &Path) -> Result<(),  Box<dyn std::error::Error>>{
    // let mut clipboard = Clipboard::new()?;
    // clipboard.set_image(arboard::ImageData { width: img.width() as usize, height: img.height() as usize, bytes: img.into_raw().into() })?;

    let _clip = Clipboard::new_attempts(10)?;

    let files = [path.to_str().ok_or("Invalid Path")?];

    clipboard_win::raw::set_file_list(&files)?;

    let mut enigo = Enigo::new(&Settings::default())?;

    enigo.key(Key::Control, Direction::Press)?;
    enigo.key(Key::Other(0x56), Direction::Click)?;
    enigo.key(Key::Control, Direction::Release)?;

    Ok(())
}

type Rect4 = (i32, i32, i32, i32);

fn find_input(auto: &UIAutomation) -> Option<UIElement> {
    let el = auto.get_focused_element().ok()?;
    let ct = el.get_control_type().ok();
    // debug: shows what UIA reports as focused (remove once placement is solid)
    println!(
        "focused: {:?} name={:?} class={:?} rect={:?}",
        ct,
        el.get_name().ok(),
        el.get_classname().ok(),
        el.get_bounding_rectangle()
            .ok()
            .map(|r| (r.get_left(), r.get_top(), r.get_right(), r.get_bottom()))
    );
    matches!(ct, Some(ControlType::Edit) | Some(ControlType::Document) | Some(ControlType::ComboBox)).then_some(el)
}

fn find_input_in_window(auto: &UIAutomation, hwnd: isize) -> Option<UIElement> {
    let root = auto.element_from_handle(Handle::from(hwnd)).ok()?;
    let all = auto
        .create_matcher()
        .from(root)
        .depth(40)
        .timeout(0)
        .filter_fn(Box::new(|e: &UIElement| {
            let ct = e.get_control_type().ok();
            if !matches!(ct, Some(ControlType::Edit) | Some(ControlType::Document) | Some(ControlType::ComboBox)) { return Ok(false); }
            if e.get_classname().map(|c| c.contains("Omnibox")).unwrap_or(false) { return Ok(false); }
            let Ok(r) = e.get_bounding_rectangle() else { return Ok(false) };
            let (w, h) = (r.get_right() - r.get_left(), r.get_bottom() - r.get_top());
            Ok(w > 100 && h > 0 && h <= 200)
        }))
        .find_all()
        .ok()?;
    // compose bars sit at the bottom, so take the lowest one
    all.into_iter().max_by_key(|e| e.get_bounding_rectangle().map(|r| r.get_bottom()).unwrap_or(0))
}

fn caret_rect(hwnd: isize) -> Option<Rect4> {
    unsafe {
        let h = HWND(hwnd as *mut core::ffi::c_void);
        let tid = GetWindowThreadProcessId(h, None);
        let mut gti = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(tid, &mut gti).ok()?;
        if gti.hwndCaret.0.is_null() { return None; }
        let mut tl = POINT { x: gti.rcCaret.left, y: gti.rcCaret.top };
        let mut br = POINT { x: gti.rcCaret.right, y: gti.rcCaret.bottom };
        if !ClientToScreen(gti.hwndCaret, &mut tl).as_bool() { return None; }
        if !ClientToScreen(gti.hwndCaret, &mut br).as_bool() { return None; }
        Some((tl.x, tl.y, br.x, br.y))
    }
}
// fallback just render it near a thin strip end of the screen like bottom of the screen
fn window_bottom_rect(hwnd: isize) -> Option<Rect4> {
    let mut rc = RECT::default();
    unsafe { GetWindowRect(HWND(hwnd as *mut core::ffi::c_void), &mut rc).ok()?; }
    let cx = (rc.left + rc.right) / 2;
    Some((cx - 250, rc.bottom - 80, cx + 250, rc.bottom - 40))
}

fn cursor_rect() -> Rect4 {
    let mut p = POINT::default();
    unsafe { let _ = GetCursorPos(&mut p); }
    (p.x, p.y, p.x, p.y)
}

fn place_popup(ui: &Picker, (l, t, r, b): Rect4) {
    let size = ui.window().size(); // physical px
    let (w, h) = (size.width as i32, size.height as i32);
    let gap = 8;

    let rc = RECT { left: l, top: t, right: r, bottom: b };
    let work = unsafe {
        let mon = MonitorFromRect(&rc, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(mon, &mut mi).as_bool() { mi.rcWork } else { rc }
    };

    let x = l.clamp(work.left, (work.right - w).max(work.left));

    // if no spcae above box flip the box
    let mut y = t - h - gap;
    if y < work.top {
        y = b + gap;
    }
    let y = y.clamp(work.top, (work.bottom - h).max(work.top));

    ui.window().set_position(slint::PhysicalPosition::new(x, y));
}


fn prewarm(auto: &UIAutomation, hwnd: isize) {
    let Ok(root) = auto.element_from_handle(Handle::from(hwnd)) else { return };
    let _ = root.get_name();
    // walking some children is what makes Chromium build its a11y tree
    if let Ok(walker) = auto.get_control_view_walker() {
        let mut cur = walker.get_first_child(&root).ok();
        let mut n = 0;
        while let Some(el) = cur {
            n += 1;
            if n > 20 { break; }
            cur = walker.get_next_sibling(&el).ok();
        }
    }
}

fn spawn_uia_worker(cache: Cache) -> (mpsc::Sender<isize>, mpsc::Receiver<Option<Rect4>>) {
    let (req_tx, req_rx) = mpsc::channel::<isize>();
    let (res_tx, res_rx) = mpsc::channel::<Option<Rect4>>();

    std::thread::spawn(move || {
        let Ok(auto) = UIAutomation::new() else {
            eprintln!("uia init failed");
            return;
        };

        // background listener that fills the cache; must stay alive for the whole thread
        let handler = UIFocusChangedEventHandler::from(InputCacher { cache: cache.clone() });
        if let Err(e) = auto.add_focus_changed_event_handler(None, &handler) {
            eprintln!("focus handler failed: {e}");
        }

        let mut last_warm: isize = 0;
        loop {
            let mut hwnd = match req_rx.recv_timeout(Duration::from_millis(300)) {
                Ok(h) => h,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // idle: warm whatever window is in front, once per window
                    let fg = unsafe { GetForegroundWindow() }.0 as isize;
                    if fg != 0 && fg != last_warm {
                        last_warm = fg;
                        prewarm(&auto, fg);
                    }
                    continue;
                }
                Err(_) => break,
            };
            while let Ok(newer) = req_rx.try_recv() { hwnd = newer; } // skip stale requests

            let t = std::time::Instant::now();

            // 0) text caret: most precise, works in Chromium when an input is focused
            let mut src = "none";
            let mut rect = caret_rect(hwnd).filter(|r| {
                let (w, h) = (r.2 - r.0, r.3 - r.1);
                println!("[caret] raw rect={:?} ({}x{})", r, w, h);
                h > 0 && h < 100 && w < 100 && r.0 > 0 && r.1 > 0   // sane caret only
            });
            if rect.is_some() { src = "caret"; }

            if rect.is_none() {
                if let Some(r) = cached_input(&cache, hwnd) {
                    rect = Some(r);
                    src = "cache";
                }
            }

            // 1) focused element, but only if it's compose-bar sized
            if rect.is_none() {
                if let Some(el) = find_input(&auto) {
                    if let Ok(r) = el.get_bounding_rectangle() {
                        let (w, h) = (r.get_right() - r.get_left(), r.get_bottom() - r.get_top());
                        if w > 100 && h > 0 && h <= 200 {
                            rect = Some((r.get_left(), r.get_top(), r.get_right(), r.get_bottom()));
                            src = "focused";
                        }
                    }
                }
            }

            // 2) search the window's tree
            if rect.is_none() {
                for _ in 0..3 {
                    if let Some(el) = find_input_in_window(&auto, hwnd) {
                        if let Ok(r) = el.get_bounding_rectangle() {
                            rect = Some((r.get_left(), r.get_top(), r.get_right(), r.get_bottom()));
                            src = "tree";
                            break;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(300));
                }
            }
            
            if src == "tree" {
                if let (Some(r), Some(win)) = (rect, window_rect(hwnd)) {
                    cache.lock().unwrap().insert(hwnd, (r, win));
                }
            }
            // 3) last resort
            if rect.is_none() {
                rect = window_bottom_rect(hwnd);
                src = "window-bottom";
            }
            println!("[placement] src={src} rect={:?} took={:?}", rect, t.elapsed());

            let rect = rect.map(|r| if r.3 - r.1 > 200 { cursor_rect() } else { r });
            let _ = res_tx.send(rect);
        }
    });

    (req_tx, res_rx)
}


// hwnd -> (input rect, that window's rect when we cached it)
type Cache = Arc<Mutex<HashMap<isize, (Rect4, Rect4)>>>;

fn window_rect(hwnd: isize) -> Option<Rect4> {
    let mut rc = RECT::default();
    unsafe { GetWindowRect(HWND(hwnd as *mut core::ffi::c_void), &mut rc).ok()?; }
    Some((rc.left, rc.top, rc.right, rc.bottom))
}

struct InputCacher { cache: Cache }

impl CustomFocusChangedEventHandler for InputCacher {
    fn handle(&self, el: &UIElement) -> uiautomation::Result<()> {
        if !matches!(el.get_control_type(), Ok(ControlType::Edit) | Ok(ControlType::Document) | Ok(ControlType::ComboBox)) {
            return Ok(());
        }
        // skip browser address bars (Brave/Chrome/Edge all use "...Omnibox...")
        if el.get_classname().map(|c| c.contains("Omnibox")).unwrap_or(false) {
            return Ok(());
        }
        let Ok(r) = el.get_bounding_rectangle() else { return Ok(()) };
        let rect = (r.get_left(), r.get_top(), r.get_right(), r.get_bottom());
        let (w, h) = (rect.2 - rect.0, rect.3 - rect.1);
        if w < 100 || h <= 0 || h > 200 { return Ok(()); } // not a compose bar

        let hwnd = unsafe { GetForegroundWindow() }.0 as isize;
        if let Some(win) = window_rect(hwnd) {
            self.cache.lock().unwrap().insert(hwnd, (rect, win));
        }
        Ok(())
    }
}

// only trust the cache if the window hasn't moved or resized since
fn cached_input(cache: &Cache, hwnd: isize) -> Option<Rect4> {
    let (input, win_then) = *cache.lock().ok()?.get(&hwnd)?;
    (window_rect(hwnd)? == win_then).then_some(input)
}


fn main() -> Result<(), slint::PlatformError> {
    let t = std::time::Instant::now();
    set_screen_reader_flag(true);
    let ui = Picker::new()?;
    let settings_win = SettingsWindow::new()?;

    let conn = db::open().unwrap();
    let results = db::find(&conn).unwrap();

    let mut memes:Vec<Meme> = Vec::new();

    for row in results {
        match Image::load_from_path(Path::new(&row.path)){
            Ok(thumb)=>memes.push(Meme {
                name: SharedString::from(row.name),
                thumb,
                path: SharedString::from(row.path)
            }),
            Err(e) => eprintln!("skipping: {}: {:?}", row.path, e),
        }
    }
    println!("startup (db + images): {:?}", t.elapsed());

    let memes = Rc::new(memes);
    ui.set_memes(ModelRc::new(VecModel::from(memes.as_ref().clone())));

    let manager = Rc::new(GlobalHotKeyManager::new().unwrap());

    // ctrl + alt + m will open the popup
    let open = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyM);
    manager.register(open).unwrap();
    let open_id = open.id();


    let left = HotKey::new(None, Code::ArrowLeft);
    let right = HotKey::new(None, Code::ArrowRight);
    let up = HotKey::new(None, Code::ArrowUp);
    let down = HotKey::new(None, Code::ArrowDown);
    let enter = HotKey::new(None, Code::Enter);
    let esc = HotKey::new(None, Code::Escape);
    let nav = [left, right, up, down, enter, esc];
    // cross button
    let weak = ui.as_weak();
    let mgr = manager.clone();
    ui.on_close(move || {
        if let Some(ui) = weak.upgrade() {
            hide_popup(&ui, &mgr, &nav);
        }
    });

    let weak = ui.as_weak();
    let mgr2 = manager.clone();
    let memes_click = memes.clone();

    ui.on_memeclicked(move |index|{
        if let Some(ui) = weak.upgrade(){
            hide_popup(&ui, &mgr2, &nav);
            // pasting sequence next
            if let Some(m) = memes_click.get(index as usize){
                let path = m.path.to_string();
                slint::Timer::single_shot(Duration::from_millis(50), move || {
                    paste_meme(Path::new(&path)).unwrap_or_else(|e| eprintln!("Failed to paste the image: {e}"));
                });
            }
        }
    });

    // apply the style and then start hidden
    let weak = ui.as_weak();
    slint::Timer::single_shot(Duration::from_millis(300), move || {
        make_non_activating();
        if let Some(ui) = weak.upgrade() {
            ui.hide().ok();
        }
    });

    // 15 milisecond hotkey press checking
    let rx = GlobalHotKeyEvent::receiver();
    let weak = ui.as_weak();
    let mgr = manager.clone();
    let timer = slint::Timer::default();

    let cache: Cache = Arc::new(Mutex::new(HashMap::new()));
    let (uia_tx, uia_rx) = spawn_uia_worker(cache.clone());


    // tray stuff
    let settings_item = MenuItem::new("Settings", true, None);
    let exit_item = MenuItem::new("Exit", true, None);
    let tray_menu = Menu::new();
    tray_menu.append_items(&[&settings_item, &exit_item]).unwrap();
    let settings_id = settings_item.id().clone();
    let exit_id = exit_item.id().clone();

    // test icon square white 
    let rgba: Vec<u8> = (0..32 * 32).flat_map(|_| [138, 43, 226, 255]).collect();
    let icon = Icon::from_rgba(rgba, 32, 32).unwrap();


    let _tray = TrayIconBuilder::new().with_menu(Box::new(tray_menu)).with_tooltip("flexmeme").with_icon(icon).build().unwrap();



    let mut pending_open: Option<std::time::Instant> = None;
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(15), move || {

        // tray settings event listener
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == settings_id {
                // setting widnow will be shown here
                println!("show settings")
            } else if ev.id == exit_id {
                slint::quit_event_loop().ok();
            }
        }


        if let Some(since) = pending_open {
            if since.elapsed() > Duration::from_millis(3500) {
                pending_open = None; // give up, open at the cursor
                println!("[placement] timed out, using cursor");
                if let Some(ui) = weak.upgrade() {
                    place_popup(&ui, cursor_rect());
                    show_popup(&ui, &mgr, &nav);
                    place_popup(&ui, cursor_rect());
                }
            }
        }
        while let Ok(rect) = uia_rx.try_recv() {
            if pending_open.is_none() { continue; } // stale or already timed out
            pending_open = None;
            let Some(ui) = weak.upgrade() else { continue };

            let rect = rect.unwrap_or_else(cursor_rect);

            place_popup(&ui, rect);
            show_popup(&ui, &mgr, &nav);
            place_popup(&ui, rect);
        }

        while let Ok(ev) = rx.try_recv() {
            if ev.state != HotKeyState::Pressed { continue; }
            let Some(ui) = weak.upgrade() else { continue };
            let visible = ui.window().is_visible();
            if ev.id == open_id {
                if visible {
                    hide_popup(&ui, &mgr, &nav);
                } else if pending_open.is_none() {
                    let hwnd = unsafe { GetForegroundWindow() }.0 as isize;
                    if uia_tx.send(hwnd).is_ok() {
                        pending_open = Some(std::time::Instant::now());
                    } else {
                        place_popup(&ui, cursor_rect());
                        show_popup(&ui, &mgr, &nav);
                    }
                }
            } else if visible {
                if ev.id == left.id() { ui.invoke_select_prev(); }
                else if ev.id == right.id() { ui.invoke_select_next(); }
                else if ev.id == up.id() { ui.invoke_select_up(); }
                else if ev.id == down.id() { ui.invoke_select_down(); }
                else if ev.id == esc.id() { hide_popup(&ui, &mgr, &nav); }
                else if ev.id == enter.id() {
                    let idx = ui.get_selected_index();
                    hide_popup(&ui, &mgr, &nav);
                    // pasting sequence
                    if let Some(m) = memes.get(idx as usize){
                        let path = m.path.to_string();
                        slint::Timer::single_shot(Duration::from_millis(50), move || {
                            paste_meme(Path::new(&path)).unwrap_or_else(|e| eprintln!("Failed to paste the image: {e}"));
                        });
                    }
                }
            }
        }
    });

    ui.show()?;
    let result = slint::run_event_loop_until_quit();
    set_screen_reader_flag(false);
    result
}