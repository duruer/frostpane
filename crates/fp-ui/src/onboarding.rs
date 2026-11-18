//! Onboarding model (ui 3.6). Steps of the rewrite (no pricing step):
//! language, welcome, six effects, wallpapers, done.

use fp_core::effect::EffectKind;
use fp_text::Key;

pub const LANGUAGE: usize = 0;
pub const WELCOME: usize = 1;
/// First effect page; effects follow in enum order.
pub const FIRST_EFFECT: usize = 2;
pub const EFFECTS: usize = 6;
pub const WALLPAPERS: usize = FIRST_EFFECT + EFFECTS;
pub const DONE: usize = WALLPAPERS + 1;
/// Dots shown: every step except the done step.
pub const DOTS: usize = DONE;

/// Window size of onboarding (ui 1.1).
pub const WINDOW: (f32, f32) = (860.0, 620.0);

/// Effect shown by an effect step.
pub fn effect_of(step: usize) -> Option<EffectKind> {
    step.checked_sub(FIRST_EFFECT).and_then(|i| EffectKind::ALL.get(i).copied())
}

/// Live weather of a step: the effect step's own, snow on done, else none.
pub fn step_weather(step: usize) -> Option<EffectKind> {
    if step == DONE { Some(EffectKind::Snow) } else { effect_of(step) }
}

/// Back button: every step after the first except the last.
pub fn back_visible(step: usize) -> bool {
    step > LANGUAGE && step < DONE
}

/// Dots are hidden at the done step.
pub fn dots_visible(step: usize) -> bool {
    step < DONE
}

/// The snowman art shows on the welcome and done steps only.
pub fn snowman_visible(step: usize) -> bool {
    step == WELCOME || step == DONE
}

/// Label of the primary (forward) button.
pub fn primary_key(step: usize) -> Key {
    match step {
        WELCOME => Key::ObStart,
        DONE => Key::ObFinish,
        _ => Key::ObNext,
    }
}

/// Step after a click on the primary button; `None` = onboarding finished.
pub fn forward(step: usize) -> Option<usize> {
    (step < DONE).then_some(step + 1)
}

/// Step after Back.
pub fn back(step: usize) -> usize {
    step.saturating_sub(1)
}

/// 1-based effect number for the step counter ("effect i of 6").
pub fn effect_number(step: usize) -> Option<usize> {
    effect_of(step).map(|_| step - FIRST_EFFECT + 1)
}

/// Width of a progress dot: 18 for the current one, 6 otherwise.
pub fn dot_width(index: usize, step: usize) -> f32 {
    if index == step { 18.0 } else { 6.0 }
}

/// Rule chosen for leaving early: clear the live weather when the window is
/// minimised or closed before onboarding finished.
pub fn clears_on_leave(finished: bool) -> bool {
    !finished
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_indices() {
        assert_eq!((LANGUAGE, WELCOME, FIRST_EFFECT, WALLPAPERS, DONE), (0, 1, 2, 8, 9));
        assert_eq!(DOTS, 9);
    }

    #[test]
    fn live_weather_per_step() {
        assert_eq!(step_weather(LANGUAGE), None);
        assert_eq!(step_weather(WELCOME), None);
        for (i, e) in EffectKind::ALL.iter().enumerate() {
            assert_eq!(step_weather(FIRST_EFFECT + i), Some(*e));
            assert_eq!(effect_number(FIRST_EFFECT + i), Some(i + 1));
        }
        assert_eq!(step_weather(WALLPAPERS), None);
        assert_eq!(step_weather(DONE), Some(EffectKind::Snow));
        assert_eq!(effect_number(WELCOME), None);
        assert_eq!(effect_of(DONE), None);
    }

    #[test]
    fn navigation_and_labels() {
        assert!(!back_visible(LANGUAGE));
        assert!(back_visible(WELCOME) && back_visible(WALLPAPERS));
        assert!(!back_visible(DONE));
        assert!(dots_visible(WALLPAPERS) && !dots_visible(DONE));
        assert!(snowman_visible(WELCOME) && snowman_visible(DONE) && !snowman_visible(4));
        assert_eq!(primary_key(LANGUAGE), Key::ObNext);
        assert_eq!(primary_key(WELCOME), Key::ObStart);
        assert_eq!(primary_key(5), Key::ObNext);
        assert_eq!(primary_key(DONE), Key::ObFinish);
        assert_eq!(forward(LANGUAGE), Some(1));
        assert_eq!(forward(WALLPAPERS), Some(DONE));
        assert_eq!(forward(DONE), None);
        assert_eq!(back(3), 2);
        assert_eq!(back(0), 0);
    }

    #[test]
    fn dots_and_leave_rule() {
        assert_eq!(dot_width(3, 3), 18.0);
        assert_eq!(dot_width(2, 3), 6.0);
        assert!(clears_on_leave(false));
        assert!(!clears_on_leave(true));
    }
}
