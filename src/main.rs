slint::include_modules!();

use slint::{ModelRc, VecModel};



fn main() -> Result<(), slint::PlatformError> {
    let ui = Picker::new()?;

    let weak = ui.as_weak();
    ui.on_close(move || {
        if let Some(ui) = weak.upgrade(){
            ui.hide().ok();
        }
    });

    ui.run()
}