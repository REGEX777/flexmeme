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


fn set_screen_reader_flag(on: bool) {
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETSCREENREADER, on as u32, None, SPIF_SENDCHANGE);
    }
}


fn make_non_activating(){
    let hwnd = match unsafe { FindWindowW(PCWSTR::null(), w!("flexmeme-popup")) } {
        Ok(h) => h,
        Err(e) => {
            println!("window not found: {:?}", e);
            return;
        }
    };
    println!("hwnd = {:?}", hwnd);

    unsafe {
        let old = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE,
            old | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize);

        let now = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        println!("noactivate: {}", (now & WS_EX_NOACTIVATE.0 as isize) != 0);
    }
}


fn show_popup(ui: &Picker, manager: &GlobalHotKeyManager, nav: &[HotKey]) {
    // let before = unsafe { GetForegroundWindow() };
    if let Err(e) = manager.register_all(nav) {
        println!("registering nav keys failed: {:?}", e);
    }
    ui.set_selected_index(0);
    ui.show().ok();
    // let after = unsafe { GetForegroundWindow() };
    // println!("foreground before={:?} after={:?} changed={}", before, after, before != after);

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

    println!("{:?}", path);

    let mut enigo = Enigo::new(&Settings::default())?;

    enigo.key(Key::Control, Direction::Press)?;
    enigo.key(Key::Other(0x56), Direction::Click)?;
    enigo.key(Key::Control, Direction::Release)?;

    Ok(())
}

type Rect4 = (i32, i32, i32, i32);

fn find_input(auto: &UIAutomation) -> Option<UIElement> {
    let el = auto.get_focused_element().ok()?;
    matches!(
        el.get_control_type(),
        Ok(ControlType::Edit) | Ok(ControlType::Document)
    )
    .then_some(el)
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

fn spawn_uia_worker() -> (mpsc::Sender<isize>, mpsc::Receiver<Option<Rect4>>) {
    let (req_tx, req_rx) = mpsc::channel::<isize>();
    let (res_tx, res_rx) = mpsc::channel::<Option<Rect4>>();

    std::thread::spawn(move || {
        let Ok(auto) = UIAutomation::new() else {
            eprintln!("uia init failed");
            return;
        };
        while let Ok(mut hwnd) = req_rx.recv() {
            while let Ok(newer) = req_rx.try_recv() { hwnd = newer; } // skip inactive stale baasi requests

            let t = std::time::Instant::now();
            println!("uia request start");
            let mut rect = None;
            for _ in 0..2 {
                if let Some(el) = find_input(&auto) {
                    if let Ok(r) = el.get_bounding_rectangle() {
                        rect = Some((r.get_left(), r.get_top(), r.get_right(), r.get_bottom()));
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(60));
            }
            if rect.is_none() {
                println!("no focused input, using window bottom");
                rect = window_bottom_rect(hwnd);
            }
            println!("uia took {:?}", t.elapsed());
            let _ = res_tx.send(rect);
        }
    });

    (req_tx, res_rx)
}

fn main() -> Result<(), slint::PlatformError> {
    let t = std::time::Instant::now();
    set_screen_reader_flag(true);
    let ui = Picker::new()?;
    
    let conn = db::open().unwrap();
    let results = db::find(&conn).unwrap();
    println!("db done: {:?}", t.elapsed());


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
    println!("images done: {:?}", t.elapsed());

    // let memes = vec![
    //     for result in results {
    //         Meme {
    //             name: SharedString::from(result.name),
    //             thumb: Image::load_from_path(Path::new(result.path)).unwrap(),
    //         }
    //     }
    // ];
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
        println!("{}", index);
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
    
    // let auto = UIAutomation::new().map_err(|e| e.to_string())?;
    let (uia_tx, uia_rx) = spawn_uia_worker();
    let mut pending_open: Option<std::time::Instant> = None;
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(15), move || {
        if let Some(since) = pending_open {
            if since.elapsed() > Duration::from_millis(600) {
                pending_open = None; // give up, open at the cursor
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
            let (l, t, r, b) = rect;
            println!("input box: left={l} top={t} right={r} bottom={b} ({}x{})", r - l, b - t);

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
                    println!("picked index {idx}");
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