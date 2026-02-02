use std::{error::Error, thread, time::Duration};

use log::{error, info};

mod switcher;


fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    info!("Rust Switcher v1.2 запущен");
    let config = switcher::load_config();
    let mut state = switcher::Switcher::new(config.clone())?;

    loop {
        match switcher::find_keyboard(&state.config) {
            Ok(device_path) => {
                match switcher::run_main_loop(&device_path, &mut state){
                    Ok(_) => {},
                    Err(e) => {
                        error!("{e}");
                    },
                };
            }
            Err(e) => error!("{}", e),
        }
        thread::sleep(Duration::from_millis(state.config.retry_delay_ms));
    }
}

