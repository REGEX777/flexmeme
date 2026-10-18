mod db;
slint::include_modules!();

use slint::{Image, ModelRc, SharedString, VecModel};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::{HotKey, Modifiers, Code}};
use std::{path::Path, time::Duration};
use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};


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

    let weak = ui.as_weak();
    ui.on_close(move || {
        if let Some(ui) = weak.upgrade() {
            ui.hide().ok();
        }
    });


    let weak = ui.as_weak();
    slint::Timer::single_shot(Duration::from_millis(300), move || {
        println!("event loop running at: {:?}", t.elapsed());
        make_non_activating();
        if let Some(ui) = weak.upgrade() {
            ui.hide().ok();      // start hidden, Ctrl+Alt+M shows karegaa bhaiyeee
        }
    });

    // registering the ctrl alt and m key 

    let manager = GlobalHotKeyManager::new().unwrap();
    let open = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyM);
    manager.register(open).unwrap();

    let open_id = open.id();



    // checking if the hotkey was pressed every 15ms
    let rx = GlobalHotKeyEvent::receiver();
    let weak = ui.as_weak();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(15), move || {
        while let Ok(ev) = rx.try_recv() {
            if ev.state != HotKeyState::Pressed { continue; }
            if ev.id == open_id{
                if let Some(ui) = weak.upgrade(){
                    if ui.window().is_visible() {
                        ui.hide().ok();
                    } else {
                        ui.show().ok();
                    }
                }
            }
        }
    });


    ui.show()?;
    println!("starting loop");
    slint::run_event_loop_until_quit()
}

