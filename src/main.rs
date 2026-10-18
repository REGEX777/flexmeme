mod db;
slint::include_modules!();

use slint::{Image, ModelRc, SharedString, VecModel, platform::Key::{Alt, Control}};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::{HotKey, Modifiers, Code}};
use std::{path::Path, time::Duration};


fn main() -> Result<(), slint::PlatformError> {
    let ui = Picker::new()?;
    
    let conn = db::open().unwrap();
    let results = db::find(&conn).unwrap();
    println!("{:?}", results);


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
        if let Some(ui) = weak.upgrade(){
            ui.hide().ok();
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
            if(ev.id == open_id){
                if let Some(ui) = weak.upgrade(){
                    if ui.window().is_visible(){
                        ui.hide().ok();
                    }else{
                        ui.window().show();
                    }
                }
            }
        }
    });


    ui.show()?;
    slint::run_event_loop_until_quit()
}