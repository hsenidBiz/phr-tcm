//! The Auto Test browser's numbered slots.
//!
//! There are at most eight browsers in all: slot 0, the main one, and
//! `EXTRA_CAP` more beside it. How many may be open at once is the person's
//! setting (`app_settings::autorun_browser_limit`, 1 to 8, default 5); the
//! cap here is only the ceiling the fixed arrays are sized for. The main slot is the browser the person and every path
//! that predates several assistants use (`commands::autorun::SESSION`);
//! the others hold what an extra assistant opens. The cap is fixed, so the
//! per-browser state is a plain array indexed by `SlotNo`, not a map.

/// The ceiling on how many browsers there can be beside the main one: what
/// the per-slot arrays are sized for, not how many are allowed (that is the
/// `autorun_browsers` setting, which never exceeds `SLOT_COUNT`).
pub const EXTRA_CAP: usize = 7;

/// Every browser there can be: the main one and the extras.
pub const SLOT_COUNT: usize = 1 + EXTRA_CAP;

/// Which of the Auto Test browsers: 0 is the main one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotNo(pub u8);

impl SlotNo {
    /// The person's browser, and the one every default caller uses.
    pub const MAIN: SlotNo = SlotNo(0);

    /// Where this slot's state sits in a per-slot array.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    pub fn is_main(self) -> bool {
        self == SlotNo::MAIN
    }

    /// Every slot, main first.
    pub fn all() -> [SlotNo; SLOT_COUNT] {
        std::array::from_fn(|i| SlotNo(i as u8))
    }

    /// Every slot but main.
    pub fn extras() -> [SlotNo; EXTRA_CAP] {
        std::array::from_fn(|i| SlotNo(i as u8 + 1))
    }

    /// The slot numbered `n`; `None` from `SLOT_COUNT` up.
    pub fn from_number(n: u8) -> Option<SlotNo> {
        (usize::from(n) < SLOT_COUNT).then_some(SlotNo(n))
    }
}
