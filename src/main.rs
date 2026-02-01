use evdev::{Device, InputEventKind, Key as EvKey};
use serde::Deserialize;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

// Импортируем типы из uinput правильно
use uinput::event::keyboard::{
    Keyboard as UiKey,
    Key,
    KeyPad,
    Misc
};

#[macro_use]
extern crate log;

fn default_retry_delay() -> u64 { 1000 }
fn default_buffer_size() -> usize { 100 }

#[derive(Deserialize, Clone)]
struct Config {
    device_path: Option<String>,
    trigger_key: String,
    layout_switch_combo: Vec<String>,
    ignored_keywords: Vec<String>,
    #[serde(default = "default_buffer_size")]
    buffer_size: usize,
    #[serde(default = "default_retry_delay")]
    retry_delay_ms: u64,
}

struct Switcher {
    config: Config,
    buffer: Vec<(EvKey, bool)>,
    shift_pressed: bool,
    ctrl_pressed: bool,
    all_selected: bool,
    trigger_keycode: EvKey,
    switch_keys: Vec<UiKey>,
}

impl Switcher {
    fn new(config: Config) -> Result<Self, Box<dyn Error>> {
        let trigger_keycode = parse_evdev_key(&config.trigger_key).unwrap_or(EvKey::KEY_PAUSE);
        let mut switch_keys = Vec::new();
        for key_str in &config.layout_switch_combo {
            if let Some(key) = parse_ui_key(key_str) {
                switch_keys.push(key);
            }
        }
        Ok(Switcher {
            config,
            buffer: Vec::new(),
            shift_pressed: false,
            ctrl_pressed: false,
            all_selected: false,
            trigger_keycode,
            switch_keys
        })
    }

    fn debug_buffer_slice(&self, slice: &[(EvKey, bool)]) -> String {
        slice.iter().map(|(k, _)| {
            format!("{:?}", k).replace("KEY_", "").to_lowercase()
        }).collect::<Vec<_>>().join("")
    }

    fn fix_text(&mut self, dev: &mut uinput::Device) -> Result<(), Box<dyn Error>> {
        if self.buffer.is_empty() { return Ok(()); }

        let start_idx = if self.all_selected { 0 } else {
            self.buffer.iter()
                .rposition(|(k, _)| *k == EvKey::KEY_SPACE)
                .map(|pos| pos + 1)
                .unwrap_or(0)
        };

        let target_slice = &self.buffer[start_idx..];
        if target_slice.is_empty() { return Ok(()); }

        info!("Исправляю: [{}]", self.debug_buffer_slice(target_slice));

        let bs = UiKey::Key(Key::BackSpace);
        for _ in 0..target_slice.len() {
            let _ = dev.click(&bs);
        }
        let _ = dev.synchronize();

        for k in &self.switch_keys { let _ = dev.press(k); }
        let _ = dev.synchronize();
        thread::sleep(Duration::from_millis(100));
        for k in &self.switch_keys { let _ = dev.release(k); }
        let _ = dev.synchronize();

        thread::sleep(Duration::from_millis(250));

        let l_shift = UiKey::Key(Key::LeftShift);
        for (key, shifted) in target_slice.iter() {
            if let Some(ui_key) = evdev_to_ui_key(*key) {
                if *shifted {
                    let _ = dev.press(&l_shift);
                    let _ = dev.click(&ui_key);
                    let _ = dev.release(&l_shift);
                } else {
                    let _ = dev.click(&ui_key);
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        let _ = dev.synchronize();

        if self.all_selected {
            self.buffer.clear();
            self.all_selected = false;
        } else {
            self.buffer.truncate(start_idx);
        }
        Ok(())
    }
}

fn load_config() -> Config {
    let path = "config.toml";
    let content = fs::read_to_string(path).expect("config.toml not found!");
    toml::from_str(&content).expect("config.toml parse error")
}

fn find_keyboard(config: &Config) -> Result<PathBuf, Box<dyn Error>> {
    if let Some(ref path) = config.device_path {
        if !path.is_empty() {
            let p = PathBuf::from(path);
            if p.exists() { return Ok(p); }
        }
    }
    let mut candidates = Vec::new();
    for entry in glob::glob("/dev/input/event*")? {
        let path = entry?;
        if let Ok(dev) = Device::open(&path) {
            let name = dev.name().unwrap_or("").to_lowercase();
            if name.contains("rust switcher virtual") || config.ignored_keywords.iter().any(|k| name.contains(&k.to_lowercase())) {
                continue;
            }
            if dev.supported_keys().map_or(false, |k| k.contains(EvKey::KEY_A) && k.contains(EvKey::KEY_SPACE)) {
                let mut score = 0;
                if name.contains("keyboard") { score += 100; }
                candidates.push((score, path));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.first().map(|c| c.1.clone()).ok_or_else(|| "No keyboard found".into())
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    info!("Rust Switcher v1.2 запущен");
    let config = load_config();
    let mut state = Switcher::new(config.clone())?;

    loop {
        match find_keyboard(&state.config) {
            Ok(device_path) => {
                let _ = run_main_loop(&device_path, &mut state);
            },
            Err(e) => error!("{}", e),
        }
        thread::sleep(Duration::from_millis(state.config.retry_delay_ms));
    }
}

fn run_main_loop(device_path: &Path, state: &mut Switcher) -> Result<(), Box<dyn Error>> {
    let mut device = Device::open(device_path)?;
    device.grab()?;
    let mut v_dev = uinput::default()?
        .name("Rust Switcher Virtual Device")?
        .event(uinput::event::Keyboard::All)?
        .create()?;

    loop {
        for event in device.fetch_events()? {
            if let InputEventKind::Key(key) = event.kind() {
                let value = event.value();
                if key == EvKey::KEY_LEFTSHIFT || key == EvKey::KEY_RIGHTSHIFT { state.shift_pressed = value != 0; }
                if key == EvKey::KEY_LEFTCTRL || key == EvKey::KEY_RIGHTCTRL { state.ctrl_pressed = value != 0; }

                if value == 1 {
                    if key == state.trigger_keycode {
                        let _ = state.fix_text(&mut v_dev);
                        continue;
                    }
                    if state.ctrl_pressed && key == EvKey::KEY_A {
                        state.all_selected = true;
                    } else if is_printable(key) {
                        if !state.ctrl_pressed { state.all_selected = false; }
                        if state.buffer.len() >= state.config.buffer_size { state.buffer.remove(0); }
                        state.buffer.push((key, state.shift_pressed));
                    } else if is_reset_key(key) {
                        state.buffer.clear();
                        state.all_selected = false;
                    }
                }

                if let Some(ui_key) = evdev_to_ui_key(key) {
                    match value {
                        1 | 2 => { let _ = v_dev.press(&ui_key); },
                        0 => { let _ = v_dev.release(&ui_key); },
                        _ => {}
                    }
                    let _ = v_dev.synchronize();
                }
            }
        }
    }
}

fn is_printable(key: EvKey) -> bool {
    let c = key.code();
    (c >= EvKey::KEY_1.code() && c <= EvKey::KEY_SLASH.code()) ||
    (c >= EvKey::KEY_A.code() && c <= EvKey::KEY_Z.code()) || key == EvKey::KEY_SPACE
}

fn is_reset_key(key: EvKey) -> bool {
    key == EvKey::KEY_ENTER || key == EvKey::KEY_KPENTER || key == EvKey::KEY_ESC || key == EvKey::KEY_TAB
}

fn parse_evdev_key(s: &str) -> Option<EvKey> {
    match s {
        "KEY_PAUSE" => Some(EvKey::KEY_PAUSE),
        "KEY_F12" => Some(EvKey::KEY_F12),
        "KEY_CAPSLOCK" => Some(EvKey::KEY_CAPSLOCK),
        _ => Some(EvKey::KEY_PAUSE),
    }
}

fn parse_ui_key(s: &str) -> Option<UiKey> {
    match s {
        "KEY_LEFTMETA" => Some(UiKey::Key(Key::LeftMeta)),
        "KEY_SPACE" => Some(UiKey::Key(Key::Space)),
        "KEY_LEFTSHIFT" => Some(UiKey::Key(Key::LeftShift)),
        "KEY_LEFTALT" => Some(UiKey::Key(Key::LeftAlt)),
        "KEY_LEFTCTRL" => Some(UiKey::Key(Key::LeftControl)),
        _ => None,
    }
}

fn evdev_to_ui_key(key: EvKey) -> Option<UiKey> {
    match key {
        EvKey::KEY_A => Some(UiKey::Key(Key::A)), EvKey::KEY_B => Some(UiKey::Key(Key::B)), EvKey::KEY_C => Some(UiKey::Key(Key::C)),
        EvKey::KEY_D => Some(UiKey::Key(Key::D)), EvKey::KEY_E => Some(UiKey::Key(Key::E)), EvKey::KEY_F => Some(UiKey::Key(Key::F)),
        EvKey::KEY_G => Some(UiKey::Key(Key::G)), EvKey::KEY_H => Some(UiKey::Key(Key::H)), EvKey::KEY_I => Some(UiKey::Key(Key::I)),
        EvKey::KEY_J => Some(UiKey::Key(Key::J)), EvKey::KEY_K => Some(UiKey::Key(Key::K)), EvKey::KEY_L => Some(UiKey::Key(Key::L)),
        EvKey::KEY_M => Some(UiKey::Key(Key::M)), EvKey::KEY_N => Some(UiKey::Key(Key::N)), EvKey::KEY_O => Some(UiKey::Key(Key::O)),
        EvKey::KEY_P => Some(UiKey::Key(Key::P)), EvKey::KEY_Q => Some(UiKey::Key(Key::Q)), EvKey::KEY_R => Some(UiKey::Key(Key::R)),
        EvKey::KEY_S => Some(UiKey::Key(Key::S)), EvKey::KEY_T => Some(UiKey::Key(Key::T)), EvKey::KEY_U => Some(UiKey::Key(Key::U)),
        EvKey::KEY_V => Some(UiKey::Key(Key::V)), EvKey::KEY_W => Some(UiKey::Key(Key::W)), EvKey::KEY_X => Some(UiKey::Key(Key::X)),
        EvKey::KEY_Y => Some(UiKey::Key(Key::Y)), EvKey::KEY_Z => Some(UiKey::Key(Key::Z)),
        EvKey::KEY_0 => Some(UiKey::Key(Key::_0)), EvKey::KEY_1 => Some(UiKey::Key(Key::_1)), EvKey::KEY_2 => Some(UiKey::Key(Key::_2)),
        EvKey::KEY_3 => Some(UiKey::Key(Key::_3)), EvKey::KEY_4 => Some(UiKey::Key(Key::_4)), EvKey::KEY_5 => Some(UiKey::Key(Key::_5)),
        EvKey::KEY_6 => Some(UiKey::Key(Key::_6)), EvKey::KEY_7 => Some(UiKey::Key(Key::_7)), EvKey::KEY_8 => Some(UiKey::Key(Key::_8)),
        EvKey::KEY_9 => Some(UiKey::Key(Key::_9)),
        EvKey::KEY_ESC => Some(UiKey::Key(Key::Esc)), EvKey::KEY_ENTER => Some(UiKey::Key(Key::Enter)),
        EvKey::KEY_BACKSPACE => Some(UiKey::Key(Key::BackSpace)), EvKey::KEY_TAB => Some(UiKey::Key(Key::Tab)),
        EvKey::KEY_SPACE => Some(UiKey::Key(Key::Space)), EvKey::KEY_MINUS => Some(UiKey::Key(Key::Minus)),
        EvKey::KEY_EQUAL => Some(UiKey::Key(Key::Equal)), EvKey::KEY_LEFTBRACE => Some(UiKey::Key(Key::LeftBrace)),
        EvKey::KEY_RIGHTBRACE => Some(UiKey::Key(Key::RightBrace)), EvKey::KEY_BACKSLASH => Some(UiKey::Key(Key::BackSlash)),
        EvKey::KEY_SEMICOLON => Some(UiKey::Key(Key::SemiColon)), EvKey::KEY_APOSTROPHE => Some(UiKey::Key(Key::Apostrophe)),
        EvKey::KEY_GRAVE => Some(UiKey::Key(Key::Grave)), EvKey::KEY_COMMA => Some(UiKey::Key(Key::Comma)),
        EvKey::KEY_DOT => Some(UiKey::Key(Key::Dot)), EvKey::KEY_SLASH => Some(UiKey::Key(Key::Slash)),
        EvKey::KEY_LEFTSHIFT => Some(UiKey::Key(Key::LeftShift)), EvKey::KEY_RIGHTSHIFT => Some(UiKey::Key(Key::RightShift)),
        EvKey::KEY_LEFTCTRL => Some(UiKey::Key(Key::LeftControl)), EvKey::KEY_RIGHTCTRL => Some(UiKey::Key(Key::RightControl)),
        EvKey::KEY_LEFTALT => Some(UiKey::Key(Key::LeftAlt)), EvKey::KEY_RIGHTALT => Some(UiKey::Key(Key::RightAlt)),
        EvKey::KEY_LEFTMETA => Some(UiKey::Key(Key::LeftMeta)), EvKey::KEY_RIGHTMETA => Some(UiKey::Key(Key::RightMeta)),
        EvKey::KEY_CAPSLOCK => Some(UiKey::Key(Key::CapsLock)), EvKey::KEY_NUMLOCK => Some(UiKey::Key(Key::NumLock)),
        EvKey::KEY_SCROLLLOCK => Some(UiKey::Key(Key::ScrollLock)),
        EvKey::KEY_UP => Some(UiKey::Key(Key::Up)), EvKey::KEY_DOWN => Some(UiKey::Key(Key::Down)),
        EvKey::KEY_LEFT => Some(UiKey::Key(Key::Left)), EvKey::KEY_RIGHT => Some(UiKey::Key(Key::Right)),
        EvKey::KEY_HOME => Some(UiKey::Key(Key::Home)), EvKey::KEY_END => Some(UiKey::Key(Key::End)),
        EvKey::KEY_PAGEUP => Some(UiKey::Key(Key::PageUp)), EvKey::KEY_PAGEDOWN => Some(UiKey::Key(Key::PageDown)),
        EvKey::KEY_INSERT => Some(UiKey::Key(Key::Insert)), EvKey::KEY_DELETE => Some(UiKey::Key(Key::Delete)),
        EvKey::KEY_KP0 => Some(UiKey::KeyPad(KeyPad::_0)), EvKey::KEY_KP1 => Some(UiKey::KeyPad(KeyPad::_1)),
        EvKey::KEY_KP2 => Some(UiKey::KeyPad(KeyPad::_2)), EvKey::KEY_KP3 => Some(UiKey::KeyPad(KeyPad::_3)),
        EvKey::KEY_KP4 => Some(UiKey::KeyPad(KeyPad::_4)), EvKey::KEY_KP5 => Some(UiKey::KeyPad(KeyPad::_5)),
        EvKey::KEY_KP6 => Some(UiKey::KeyPad(KeyPad::_6)), EvKey::KEY_KP7 => Some(UiKey::KeyPad(KeyPad::_7)),
        EvKey::KEY_KP8 => Some(UiKey::KeyPad(KeyPad::_8)), EvKey::KEY_KP9 => Some(UiKey::KeyPad(KeyPad::_9)),
        EvKey::KEY_KPASTERISK => Some(UiKey::KeyPad(KeyPad::Asterisk)), EvKey::KEY_KPPLUS => Some(UiKey::KeyPad(KeyPad::Plus)),
        EvKey::KEY_KPMINUS => Some(UiKey::KeyPad(KeyPad::Minus)), EvKey::KEY_KPDOT => Some(UiKey::KeyPad(KeyPad::Dot)),
        EvKey::KEY_KPENTER => Some(UiKey::KeyPad(KeyPad::Enter)), EvKey::KEY_KPSLASH => Some(UiKey::KeyPad(KeyPad::Slash)),
        EvKey::KEY_KPEQUAL => Some(UiKey::KeyPad(KeyPad::Equal)), EvKey::KEY_KPCOMMA => Some(UiKey::KeyPad(KeyPad::Comma)),
        EvKey::KEY_PAUSE => Some(UiKey::Misc(Misc::Pause)), EvKey::KEY_SYSRQ => Some(UiKey::Key(Key::SysRq)),
        EvKey::KEY_MUTE => Some(UiKey::Misc(Misc::Mute)), EvKey::KEY_VOLUMEUP => Some(UiKey::Misc(Misc::VolumeUp)),
        EvKey::KEY_VOLUMEDOWN => Some(UiKey::Misc(Misc::VolumeDown)), EvKey::KEY_SLEEP => Some(UiKey::Misc(Misc::Sleep)),
        EvKey::KEY_WAKEUP => Some(UiKey::Misc(Misc::WakeUp)),
        EvKey::KEY_F1 => Some(UiKey::Key(Key::F1)), EvKey::KEY_F2 => Some(UiKey::Key(Key::F2)),
        EvKey::KEY_F3 => Some(UiKey::Key(Key::F3)), EvKey::KEY_F4 => Some(UiKey::Key(Key::F4)),
        EvKey::KEY_F5 => Some(UiKey::Key(Key::F5)), EvKey::KEY_F6 => Some(UiKey::Key(Key::F6)),
        EvKey::KEY_F7 => Some(UiKey::Key(Key::F7)), EvKey::KEY_F8 => Some(UiKey::Key(Key::F8)),
        EvKey::KEY_F9 => Some(UiKey::Key(Key::F9)), EvKey::KEY_F10 => Some(UiKey::Key(Key::F10)),
        EvKey::KEY_F11 => Some(UiKey::Key(Key::F11)), EvKey::KEY_F12 => Some(UiKey::Key(Key::F12)),
        _ => None,
    }
}
