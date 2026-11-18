//! Session, power and display events and the combined "asleep" state
//! (desktop-native 3.7), shared by every backend's system watcher.

/// Session, power and display events that decide whether we are asleep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemEvent {
    Locked,
    Unlocked,
    Suspend,
    Resume,
    DisplayOff,
    DisplayOn,
    /// WM_DISPLAYCHANGE: rebuild overlays.
    DisplaysChanged,
    /// Theme or accent setting changed: repaint the tray icon.
    ThemeChanged,
    /// AC/battery status changed (PBT_APMPOWERSTATUSCHANGE): re-read
    /// [`crate::Platform::power`] (weather-sources 2.6). Does not affect sleep.
    PowerChanged,
}

/// desktop-native 3.7: `asleep = locked || suspended || displayOff`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SleepState {
    pub locked: bool,
    pub suspended: bool,
    pub display_off: bool,
}

impl SleepState {
    pub fn asleep(&self) -> bool {
        self.locked || self.suspended || self.display_off
    }

    /// Applies an event; returns `Some(asleep)` when the combined value changed.
    pub fn apply(&mut self, ev: SystemEvent) -> Option<bool> {
        let before = self.asleep();
        match ev {
            SystemEvent::Locked => self.locked = true,
            SystemEvent::Unlocked => self.locked = false,
            SystemEvent::Suspend => self.suspended = true,
            SystemEvent::Resume => self.suspended = false,
            SystemEvent::DisplayOff => self.display_off = true,
            SystemEvent::DisplayOn => self.display_off = false,
            SystemEvent::DisplaysChanged | SystemEvent::ThemeChanged | SystemEvent::PowerChanged => {}
        }
        let after = self.asleep();
        (before != after).then_some(after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_state() {
        let mut s = SleepState::default();
        assert_eq!(s.apply(SystemEvent::Locked), Some(true));
        assert_eq!(s.apply(SystemEvent::Suspend), None);
        assert_eq!(s.apply(SystemEvent::Unlocked), None);
        assert_eq!(s.apply(SystemEvent::DisplayOff), None);
        assert_eq!(s.apply(SystemEvent::Resume), None);
        assert!(s.asleep());
        assert_eq!(s.apply(SystemEvent::DisplayOn), Some(false));
        assert_eq!(s.apply(SystemEvent::DisplaysChanged), None);
        assert_eq!(s.apply(SystemEvent::ThemeChanged), None);
        assert_eq!(s.apply(SystemEvent::PowerChanged), None);
        assert!(!s.asleep());
        // Display off while locked: only the first change flips the combined value.
        assert_eq!(s.apply(SystemEvent::DisplayOff), Some(true));
        assert_eq!(s.apply(SystemEvent::Locked), None);
        assert_eq!(s.apply(SystemEvent::DisplayOn), None);
        assert_eq!(s.apply(SystemEvent::Unlocked), Some(false));
    }
}
