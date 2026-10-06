//! What `press_key` presses: one key, with any of Ctrl, Alt, Shift and
//! Meta held down for it, as a keyboard sends it.
//!
//! A script writes the combination as it reads - `Ctrl+ArrowUp`,
//! `Shift+Tab`, `Ctrl+Shift+End`. The modifiers ignore case; the key is one
//! of `actions::PRESS_KEYS`, spelt exactly as it always was, so a script
//! saved before combinations existed means what it meant.
//!
//! Every key event carries the protocol's modifier bitmask (Alt 1, Ctrl 2,
//! Meta 4, Shift 8): the modifiers go down in the order Ctrl, Alt, Shift,
//! Meta, each with the bits of those already down plus its own; the key
//! goes down and up with all of them; then the modifiers come up in reverse,
//! each without its own bit, as a real keyboard's do. A page that reads
//! `event.ctrlKey` on the key's keydown sees it.

use super::actions::PRESS_KEYS;
use super::cdp::{CdpError, Driver};
use serde_json::{json, Value};

/// A key held down for another: Ctrl, Alt, Shift or Meta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Meta,
}

impl Modifier {
    /// The order the modifiers go down in, and the reverse of the order
    /// they come up in.
    pub const HOLD_ORDER: [Modifier; 4] = [Modifier::Ctrl, Modifier::Alt, Modifier::Shift, Modifier::Meta];

    /// As a script and a sentence write it.
    pub fn name(self) -> &'static str {
        match self {
            Modifier::Ctrl => "Ctrl",
            Modifier::Alt => "Alt",
            Modifier::Shift => "Shift",
            Modifier::Meta => "Meta",
        }
    }

    /// Its bit in the protocol's `modifiers`.
    pub fn bit(self) -> i64 {
        match self {
            Modifier::Alt => 1,
            Modifier::Ctrl => 2,
            Modifier::Meta => 4,
            Modifier::Shift => 8,
        }
    }

    /// Its own key: the DOM `key`, the DOM `code` and the Windows virtual
    /// key.
    fn key(self) -> (&'static str, &'static str, i64) {
        match self {
            Modifier::Ctrl => ("Control", "ControlLeft", 17),
            Modifier::Alt => ("Alt", "AltLeft", 18),
            Modifier::Shift => ("Shift", "ShiftLeft", 16),
            Modifier::Meta => ("Meta", "MetaLeft", 91),
        }
    }

    fn parse(part: &str) -> Option<Modifier> {
        Modifier::HOLD_ORDER.into_iter().find(|m| m.name().eq_ignore_ascii_case(part))
    }
}

/// A combination `press_key` can press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCombo {
    /// In the order they go down (`Modifier::HOLD_ORDER`), whatever order
    /// the script wrote them in.
    pub modifiers: Vec<Modifier>,
    /// An index into `PRESS_KEYS`.
    key: usize,
}

impl KeyCombo {
    /// The key's name, as `PRESS_KEYS` spells it.
    pub fn key_name(&self) -> &'static str {
        PRESS_KEYS[self.key].0
    }

    /// Every modifier's bit, together.
    pub fn mask(&self) -> i64 {
        self.modifiers.iter().fold(0, |m, k| m | k.bit())
    }

    /// The combination as a sentence says it: `Ctrl+Shift+End`. A key
    /// pressed alone is just its name, as before.
    pub fn name(&self) -> String {
        let mut parts: Vec<&str> = self.modifiers.iter().map(|m| m.name()).collect();
        parts.push(self.key_name());
        parts.join("+")
    }

    /// The key events one press sends, in order.
    pub fn events(&self) -> Vec<Value> {
        let (_, key, code, vk, text) = PRESS_KEYS[self.key];
        let mut out = Vec::with_capacity(2 + 2 * self.modifiers.len());
        let mut mask = 0;
        for m in &self.modifiers {
            mask |= m.bit();
            let (k, c, v) = m.key();
            out.push(json!({
                "type": "rawKeyDown", "key": k, "code": c,
                "windowsVirtualKeyCode": v, "nativeVirtualKeyCode": v,
                "modifiers": mask,
            }));
        }
        let mut down = json!({
            "type": if text.is_some() { "keyDown" } else { "rawKeyDown" },
            "key": key, "code": code,
            "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
            "modifiers": mask,
        });
        if let Some(t) = text {
            down["text"] = json!(t);
            down["unmodifiedText"] = json!(t);
        }
        out.push(down);
        out.push(json!({
            "type": "keyUp", "key": key, "code": code,
            "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
            "modifiers": mask,
        }));
        for m in self.modifiers.iter().rev() {
            mask &= !m.bit();
            let (k, c, v) = m.key();
            out.push(json!({
                "type": "keyUp", "key": k, "code": c,
                "windowsVirtualKeyCode": v, "nativeVirtualKeyCode": v,
                "modifiers": mask,
            }));
        }
        out
    }

    /// One press of the whole combination.
    pub async fn press<D: Driver>(&self, d: &mut D) -> Result<(), CdpError> {
        for e in self.events() {
            d.call("Input.dispatchKeyEvent", e).await?;
        }
        Ok(())
    }
}

/// The names `press_key` accepts as its key, for a refusal to list.
pub fn key_names() -> String {
    PRESS_KEYS.iter().map(|k| k.0).collect::<Vec<_>>().join(", ")
}

/// The fewest and most times `press_key` presses its combination.
pub const TIMES_MIN: u8 = 1;
pub const TIMES_MAX: u8 = 50;
/// `press_key` told to press too few or too many times.
pub const TIMES_RULE: &str = "press_key: times must be 1 to 50";

/// A `press_key` value read as a combination, or the refusal that says
/// what is wrong with it.
pub fn parse(value: &str) -> Result<KeyCombo, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("press_key \"\" is not a key it presses - use one of {}", key_names()));
    }
    let parts: Vec<&str> = value.split('+').map(str::trim).collect();
    let (last, before) = parts.split_last().expect("split always yields one part");
    let mut modifiers: Vec<Modifier> = Vec::new();
    for part in before {
        let Some(m) = Modifier::parse(part) else {
            return Err(format!("press_key: \"{part}\" is not a modifier - use Ctrl, Shift, Alt or Meta"));
        };
        if modifiers.contains(&m) {
            return Err(format!("press_key: \"{}\" is given twice", m.name()));
        }
        modifiers.push(m);
    }
    if last.is_empty() || Modifier::parse(last).is_some() {
        // A modifier written last, given twice, is still given twice.
        if let Some(m) = Modifier::parse(last).filter(|m| modifiers.contains(m)) {
            return Err(format!("press_key: \"{}\" is given twice", m.name()));
        }
        return Err(format!("press_key: \"{value}\" has no key after its modifiers"));
    }
    let Some(key) = PRESS_KEYS.iter().position(|k| k.0 == *last) else {
        return Err(format!(
            "press_key \"{last}\" is not a key it presses - use one of {}, with Ctrl, Shift, Alt or Meta before it joined by +",
            key_names()
        ));
    };
    modifiers.sort_by_key(|m| Modifier::HOLD_ORDER.iter().position(|h| h == m));
    Ok(KeyCombo { modifiers, key })
}

/// What `validate` says about a `press_key`.
pub fn check(key: &str, times: Option<u8>) -> Result<(), String> {
    parse(key)?;
    match times {
        Some(t) if !(TIMES_MIN..=TIMES_MAX).contains(&t) => Err(TIMES_RULE.to_string()),
        _ => Ok(()),
    }
}
