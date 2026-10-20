mod db;
slint::include_modules!();

use slint::{Image, ModelRc, SharedString, VecModel};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::{HotKey, Modifiers, Code}};
use std::{path::Path, time::Duration};
use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
use arboard;

use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use std::rc::Rc;

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
    if let Err(e) = manager.register_all(nav) {
        println!("registering nav keys failed: {:?}", e);
    }
    ui.set_selected_index(0);
    ui.show().ok();
}

fn hide_popup(ui: &Picker, manager: &GlobalHotKeyManager, nav: &[HotKey]) {
    manager.unregister_all(nav).ok();   // ALWAYS give the keys back brochacho mueheheh
    ui.hide().ok();
}

fn main() -> Result<(), slint::PlatformError> {
    let t = std::time::Instant::now();
    let ui = Picker::new()?;
    
    let conn = db::open().unwrap();
    let results = db::find(&conn).unwrap();
    println!("db done: {:?}", t.elapsed());


    let mut memes:Vec<Meme> = Vec::new();

    for row in results {
        match Image::load_from_path(Path::new(&row.path)){
            Ok(thumb)=>memes.push(Meme {
                name: SharedString::from(row.name),
                thumb
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

    ui.set_memes(ModelRc::new(VecModel::from(memes)));

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
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(15), move || {
        while let Ok(ev) = rx.try_recv() {
            if ev.state != HotKeyState::Pressed { continue; }
            let Some(ui) = weak.upgrade() else { continue };
            let visible = ui.window().is_visible();

            if ev.id == open_id {
                if visible {
                    hide_popup(&ui, &mgr, &nav);
                } else {
                    show_popup(&ui, &mgr, &nav);
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
                    // paste stuff pending
                }
            }
        }
    });

    ui.show()?;
    slint::run_event_loop_until_quit()
}