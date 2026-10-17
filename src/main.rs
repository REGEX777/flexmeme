mod db;
slint::include_modules!();

use slint::{ModelRc, VecModel, Image, SharedString};
use std::{path::Path, result};


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

    ui.run()
}