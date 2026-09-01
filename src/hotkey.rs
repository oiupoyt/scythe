use global_hotkey::{GlobalHotKeyManager, hotkey::{HotKey, Modifiers, Code}, GlobalHotKeyEvent};
use crossbeam_channel::Sender;

pub fn run_hotkey_listener(trigger_tx: Sender<()>) -> Result<(), Box<dyn std::error::Error>> {
    let manager = GlobalHotKeyManager::new()?;
    let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR);
    manager.register(hotkey)?;

    println!("Registered global hotkey Ctrl+Shift+R to trigger replay save.");

    let receiver = GlobalHotKeyEvent::receiver();
    loop {
        if let Ok(event) = receiver.recv() {
            if event.id == hotkey.id() {
                if event.state == global_hotkey::HotKeyState::Pressed {
                    println!("Hotkey triggered! Saving replay...");
                    let _ = trigger_tx.send(());
                }
            }
        }
    }
}
