use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;
use tokio::sync::mpsc;

/// Terminal events that the application can handle.
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// A key was pressed.
/// [`Key`].
    Key(KeyEvent),
    /// Terminal was resized.
/// [`Resize`].
    Resize(u16, u16),
    /// Tick event for periodic updates.
/// [`Tick`].
    Tick,
}

/// Event handler that reads terminal events and sends them through a channel.
pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
    _tx: mpsc::UnboundedSender<AppEvent>,
}

impl EventHandler {
    /// Create a new event handler with a background thread for reading events.
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let event_tx = tx.clone();

        std::thread::spawn(move || {
            loop {
                // Poll for events with the tick rate as timeout
                if event::poll(tick_rate).unwrap_or(false) {
                    if let Ok(evt) = event::read() {
                        match evt {
                            Event::Key(key) => {
                                let _ = event_tx.send(AppEvent::Key(key));
                            }
                            Event::Resize(w, h) => {
                                let _ = event_tx.send(AppEvent::Resize(w, h));
                            }
                            _ => {}
                        }
                    }
                } else {
                    // Only send tick on timeout (no real event received)
                    let _ = event_tx.send(AppEvent::Tick);
                }
            }
        });

        Self { rx, _tx: tx }
    }

    /// Receive the next event (async).
    pub async fn next(&mut self) -> AppEvent {
        self.rx.recv().await.unwrap_or(AppEvent::Tick)
    }
}

/// Check if a key event matches a specific key with Ctrl modifier.
pub fn is_ctrl_key(key: &KeyEvent, code: KeyCode) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn test_is_ctrl_key() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(is_ctrl_key(&key, KeyCode::Char('c')));
        assert!(!is_ctrl_key(&key, KeyCode::Char('d')));
    }

    #[test]
    fn test_is_ctrl_key_no_modifier() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        assert!(!is_ctrl_key(&key, KeyCode::Char('c')));
    }
}
