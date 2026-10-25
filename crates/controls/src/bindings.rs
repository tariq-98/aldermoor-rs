//! Which keys and mouse buttons do what, as data the player can change. Inputs are named the
//! way winit names them (`KeyW`, `ShiftLeft`, `Digit1`) plus `Mouse3`..`Mouse5` for the middle
//! and thumb buttons. The left and right buttons stay fixed: they click, look and steer.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Something a key can be bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    Forward,
    Back,
    Left,
    Right,
    StrafeLeft,
    StrafeRight,
    Walk,
    Sprint,
    Jump,
    Roll,
    Guard,
    Autorun,
    FreePointer,
    Target,
    SwapWeapons,
    Revive,
    Respawn,
    Slot1,
    Slot2,
    Slot3,
    Slot4,
    Slot5,
    Slot6,
    Slot7,
    Slot8,
    Slot9,
    Slot10,
}

impl Control {
    /// Every control, in the order the menu lists them.
    pub const ALL: [Control; 27] = [
        Self::Forward,
        Self::Back,
        Self::Left,
        Self::Right,
        Self::StrafeLeft,
        Self::StrafeRight,
        Self::Walk,
        Self::Sprint,
        Self::Jump,
        Self::Roll,
        Self::Guard,
        Self::Autorun,
        Self::FreePointer,
        Self::Target,
        Self::SwapWeapons,
        Self::Revive,
        Self::Respawn,
        Self::Slot1,
        Self::Slot2,
        Self::Slot3,
        Self::Slot4,
        Self::Slot5,
        Self::Slot6,
        Self::Slot7,
        Self::Slot8,
        Self::Slot9,
        Self::Slot10,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Forward => "Run forward",
            Self::Back => "Back",
            Self::Left => "Left (strafe in Action, turn in Classic)",
            Self::Right => "Right (strafe in Action, turn in Classic)",
            Self::StrafeLeft => "Strafe left",
            Self::StrafeRight => "Strafe right",
            Self::Walk => "Walk or run (toggle)",
            Self::Sprint => "Sprint (hold, out of combat)",
            Self::Jump => "Jump",
            Self::Roll => "Roll",
            Self::Guard => "Guard (hold)",
            Self::Autorun => "Autorun",
            Self::FreePointer => "Show the pointer (hold, Action)",
            Self::Target => "Next target",
            Self::SwapWeapons => "Swap weapons",
            Self::Revive => "Revive",
            Self::Respawn => "Respawn",
            Self::Slot1 => "Skill 1",
            Self::Slot2 => "Skill 2",
            Self::Slot3 => "Skill 3",
            Self::Slot4 => "Skill 4",
            Self::Slot5 => "Skill 5",
            Self::Slot6 => "Skill 6",
            Self::Slot7 => "Skill 7",
            Self::Slot8 => "Skill 8",
            Self::Slot9 => "Skill 9",
            Self::Slot10 => "Skill 10",
        }
    }

    /// The ability bar slot, 0..10, for the skill controls.
    pub fn slot(self) -> Option<u8> {
        let first = Self::ALL.iter().position(|c| *c == Self::Slot1)?;
        let at = Self::ALL.iter().position(|c| *c == self)?;
        at.checked_sub(first).map(|i| i as u8)
    }

    fn defaults(self) -> &'static [&'static str] {
        match self {
            Self::Forward => &["KeyW"],
            Self::Back => &["KeyS"],
            Self::Left => &["KeyA"],
            Self::Right => &["KeyD"],
            Self::StrafeLeft => &["KeyQ"],
            Self::StrafeRight => &["KeyE"],
            Self::Walk => &["Slash"],
            Self::Sprint => &["ShiftLeft", "ShiftRight"],
            Self::Jump => &["Space"],
            Self::Roll => &["AltLeft", "AltRight", "KeyV", "Mouse4"],
            Self::Guard => &["KeyF", "Mouse5"],
            // T for MacBook keyboards, which have no Num Lock.
            Self::Autorun => &["KeyT", "NumLock"],
            Self::FreePointer => &["ControlLeft", "ControlRight"],
            Self::Target => &["Tab"],
            Self::SwapWeapons => &["Backquote"],
            Self::Revive => &["KeyG"],
            Self::Respawn => &["KeyR"],
            Self::Slot1 => &["Digit1"],
            Self::Slot2 => &["Digit2"],
            Self::Slot3 => &["Digit3"],
            Self::Slot4 => &["Digit4"],
            Self::Slot5 => &["Digit5"],
            Self::Slot6 => &["Digit6"],
            Self::Slot7 => &["Digit7"],
            Self::Slot8 => &["Digit8"],
            Self::Slot9 => &["Digit9"],
            Self::Slot10 => &["Digit0"],
        }
    }
}

/// A key or mouse button by name, such as `KeyW` or `Mouse4`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Input(pub String);

impl Input {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// How the menu and HUD show it: `W`, `1`, `Left Shift`, `Mouse 4`.
    pub fn label(&self) -> String {
        let n = self.0.as_str();
        if let Some(rest) = n.strip_prefix("Key").or_else(|| n.strip_prefix("Digit")) {
            return rest.to_string();
        }
        if let Some(rest) = n.strip_prefix("Mouse") {
            return format!("Mouse {rest}");
        }
        if let Some(rest) = n.strip_prefix("Arrow") {
            return format!("{rest} arrow");
        }
        for side in ["Left", "Right"] {
            if let Some(key) = n.strip_suffix(side) {
                let key = match key {
                    "Control" => "Ctrl",
                    "Super" => "Cmd",
                    key => key,
                };
                return format!("{side} {key}");
            }
        }
        match n {
            "Backquote" => "`".into(),
            "Slash" => "/".into(),
            "NumLock" => "Num Lock".into(),
            n => n.into(),
        }
    }
}

/// Every control's inputs. A control left out of a settings file keeps its default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    from = "BTreeMap<Control, Vec<Input>>",
    into = "BTreeMap<Control, Vec<Input>>"
)]
pub struct Bindings {
    map: BTreeMap<Control, Vec<Input>>,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            map: Control::ALL
                .iter()
                .map(|c| (*c, c.defaults().iter().map(|n| Input::new(*n)).collect()))
                .collect(),
        }
    }
}

impl From<BTreeMap<Control, Vec<Input>>> for Bindings {
    fn from(map: BTreeMap<Control, Vec<Input>>) -> Self {
        let mut b = Self::default();
        b.map.extend(map);
        b
    }
}

impl From<Bindings> for BTreeMap<Control, Vec<Input>> {
    fn from(b: Bindings) -> Self {
        b.map
    }
}

impl Bindings {
    pub fn inputs(&self, control: Control) -> &[Input] {
        self.map.get(&control).map_or(&[], Vec::as_slice)
    }

    /// The controls `input` triggers; more than one only if a file bound it twice.
    pub fn controls(&self, input: &Input) -> Vec<Control> {
        Control::ALL
            .into_iter()
            .filter(|c| self.inputs(*c).contains(input))
            .collect()
    }

    /// Whether any of the control's inputs is down.
    pub fn held(&self, control: Control, down: impl Fn(&Input) -> bool) -> bool {
        self.inputs(control).iter().any(down)
    }

    /// The first input's label, or "unbound".
    pub fn label(&self, control: Control) -> String {
        self.inputs(control)
            .first()
            .map_or_else(|| "unbound".into(), Input::label)
    }

    /// Adds `input` to `control`, taking it off any other control that had it. Returns the
    /// controls it was taken from, so the menu can say so.
    pub fn bind(&mut self, control: Control, input: Input) -> Vec<Control> {
        let mut taken = Vec::new();
        for (c, inputs) in &mut self.map {
            if *c != control && inputs.contains(&input) {
                inputs.retain(|i| *i != input);
                taken.push(*c);
            }
        }
        let inputs = self.map.entry(control).or_default();
        if !inputs.contains(&input) {
            inputs.push(input);
        }
        taken
    }

    pub fn unbind(&mut self, control: Control, input: &Input) {
        if let Some(inputs) = self.map.get_mut(&control) {
            inputs.retain(|i| i != input);
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Inputs bound to more than one control, with the controls they trigger.
    pub fn conflicts(&self) -> Vec<(Input, Vec<Control>)> {
        let mut by_input: BTreeMap<&Input, Vec<Control>> = BTreeMap::new();
        for (c, inputs) in &self.map {
            for i in inputs {
                by_input.entry(i).or_default().push(*c);
            }
        }
        by_input
            .into_iter()
            .filter(|(_, cs)| cs.len() > 1)
            .map(|(i, cs)| (i.clone(), cs))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: &str) -> Input {
        Input::new(n)
    }

    #[test]
    fn defaults_have_no_conflicts_and_bind_everything() {
        let b = Bindings::default();
        assert!(b.conflicts().is_empty());
        for c in Control::ALL {
            assert!(!b.inputs(c).is_empty(), "{c:?}");
        }
        assert_eq!(b.controls(&key("KeyW")), [Control::Forward]);
    }

    #[test]
    fn slots_count_from_zero() {
        assert_eq!(Control::Slot1.slot(), Some(0));
        assert_eq!(Control::Slot10.slot(), Some(9));
        assert_eq!(Control::Jump.slot(), None);
    }

    #[test]
    fn binding_takes_the_key_from_its_old_control() {
        let mut b = Bindings::default();
        let taken = b.bind(Control::Jump, key("KeyE"));
        assert_eq!(taken, [Control::StrafeRight]);
        assert_eq!(b.controls(&key("KeyE")), [Control::Jump]);
        assert!(b.inputs(Control::Jump).contains(&key("Space")));
        assert!(b.inputs(Control::StrafeRight).is_empty());
        assert_eq!(b.label(Control::StrafeRight), "unbound");
        // Binding again is a no-op.
        assert!(b.bind(Control::Jump, key("KeyE")).is_empty());
        assert_eq!(b.inputs(Control::Jump).len(), 2);
    }

    #[test]
    fn held_checks_every_input() {
        let b = Bindings::default();
        assert!(b.held(Control::Guard, |i| i.0 == "Mouse5"));
        assert!(!b.held(Control::Guard, |i| i.0 == "KeyW"));
    }

    #[test]
    fn unbind_and_reset() {
        let mut b = Bindings::default();
        b.unbind(Control::Roll, &key("KeyV"));
        assert!(!b.inputs(Control::Roll).contains(&key("KeyV")));
        b.reset();
        assert_eq!(b, Bindings::default());
    }

    #[test]
    fn a_file_lists_only_what_changed_and_may_conflict() {
        let b: Bindings = serde_json::from_str(r#"{ "jump": ["KeyW"] }"#).unwrap();
        assert_eq!(b.inputs(Control::Jump), [key("KeyW")]);
        assert_eq!(b.inputs(Control::Back), [key("KeyS")]);
        assert_eq!(
            b.conflicts(),
            [(key("KeyW"), vec![Control::Forward, Control::Jump])]
        );
        assert!(serde_json::from_str::<Bindings>(r#"{ "jupm": ["KeyW"] }"#).is_err());
        let round: Bindings = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        assert_eq!(round, b);
    }

    #[test]
    fn labels() {
        for (name, label) in [
            ("KeyW", "W"),
            ("Digit0", "0"),
            ("ShiftLeft", "Left Shift"),
            ("ControlRight", "Right Ctrl"),
            ("Mouse4", "Mouse 4"),
            ("Backquote", "`"),
            ("ArrowUp", "Up arrow"),
            ("Space", "Space"),
        ] {
            assert_eq!(key(name).label(), label);
        }
    }
}
