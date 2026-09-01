use global_hotkey::{GlobalHotKeyManager, hotkey::{HotKey, Modifiers, Code}, GlobalHotKeyEvent};
use crossbeam_channel::Sender;

pub fn run_hotkey_listener(trigger_tx: Sender<()>) -> Result<(), Box<dyn std::error::Error>> {
    let manager = GlobalHotKeyManager::new()?;
    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::KeyZ);
    manager.register(hotkey)?;

    println!("Registered global hotkey Ctrl+Shift+R to trigger replay save.");

    let receiver = GlobalHotKeyEvent::receiver();
    loop {
        if let Ok(event) = receiver.recv()
            && event.id == hotkey.id() && event.state == global_hotkey::HotKeyState::Pressed {
                println!("Hotkey triggered! Saving replay...");
                let _ = trigger_tx.send(());
            }
    }
}
