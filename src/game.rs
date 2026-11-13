//! Where the save data lives in game version 421810, and the loop that watches it.
//!
//! The game keeps all three save slots in memory, and the index of the one being played next to
//! the pointer to them. Both are read on every tick, so switching characters is picked up. If
//! another slot's playtime is the one counting up, that slot is believed over the index. A return
//! from a quest shows up as fertilizer quests going down or new items in the harvest box, and
//! that's when the plugin fertilizes and collects.
//!
//! The slot's critical section, which the game's menus take while they change the slot, is held
//! for the whole check, so the game never sees a half-moved harvest.

use crate::farm::{self, Fertilizer, Item};

/// Static pointer in `MonsterHunterWorld.exe` to the object that holds the save data pointer.
pub const SAVE_ROOT_RVA: usize = 0x501_3950;
const SAVE_PTR_OFFSET: usize = 0xA8;
/// Index of the slot being played, in the same object.
const SLOT_INDEX_OFFSET: usize = 0xA0;
const SLOT_SIZE: usize = 0x26_CC00;
const SLOTS: usize = 3;
// Offsets inside a slot.
const SLOT_LOCK: usize = 0x08;
const HUNTER_NAME: usize = 0x50;
const NAME_LEN: usize = 0x40;
const RESEARCH_POINTS: usize = 0x98;
const PLAYTIME: usize = 0xA0;
const ITEM_BOX: usize = 0x3_8A10;
const FERTILIZER: usize = 0x10_3070;
const HARVEST_BOX: usize = FERTILIZER + 0x50;
const ENTRY_SIZE: usize = 0x10;
const FERTILIZER_SLOTS: usize = 4;
const HARVEST_SLOTS: usize = 50;
const ITEM_BOX_SLOTS: usize = 200;

/// Game memory, through a trait so the logic can run against a fake in tests.
pub trait Memory {
    fn read(&self, addr: usize, buf: &mut [u8]) -> bool;
    fn write(&mut self, addr: usize, data: &[u8]) -> bool;

    fn read_u32(&self, addr: usize) -> Option<u32> {
        let mut b = [0u8; 4];
        self.read(addr, &mut b).then(|| u32::from_le_bytes(b))
    }
    fn read_u64(&self, addr: usize) -> Option<u64> {
        let mut b = [0u8; 8];
        self.read(addr, &mut b).then(|| u64::from_le_bytes(b))
    }
    fn write_u32(&mut self, addr: usize, value: u32) -> bool {
        self.write(addr, &value.to_le_bytes())
    }

    /// Enters the critical section at `addr`. Returns false, without entering, if it can't.
    fn enter(&mut self, _addr: usize) -> bool {
        true
    }
    /// Leaves the critical section `enter` entered.
    fn leave(&mut self, _addr: usize) {}
}

/// Reads `n` entries of `{ u32 id, u32 count, 8 more bytes }`.
fn read_entries(mem: &impl Memory, addr: usize, n: usize) -> Option<Vec<Item>> {
    let mut buf = vec![0u8; n * ENTRY_SIZE];
    if !mem.read(addr, &mut buf) {
        return None;
    }
    Some(
        buf.chunks_exact(ENTRY_SIZE)
            .map(|e| Item {
                id: u32::from_le_bytes(e[0..4].try_into().unwrap()),
                count: u32::from_le_bytes(e[4..8].try_into().unwrap()),
            })
            .collect(),
    )
}

/// Writes back the entries that differ from `old`, the first 8 bytes of each only.
fn write_entries(mem: &mut impl Memory, addr: usize, old: &[Item], new: &[Item]) -> bool {
    let mut ok = true;
    for (i, (_, n)) in old.iter().zip(new).enumerate().filter(|(_, (o, n))| o != n) {
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&n.id.to_le_bytes());
        b[4..].copy_from_slice(&n.count.to_le_bytes());
        ok &= mem.write(addr + i * ENTRY_SIZE, &b);
    }
    ok
}

fn to_fertilizers(entries: &[Item]) -> [Fertilizer; 4] {
    std::array::from_fn(|i| Fertilizer { kind: entries[i].id, remain: entries[i].count })
}

fn fertilizer_list(slots: &[Fertilizer; 4]) -> String {
    let list: Vec<String> =
        slots.iter().filter(|s| s.remain > 0).map(|s| format!("type {} x{}", s.kind, s.remain)).collect();
    if list.is_empty() { "none".into() } else { list.join(", ") }
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub event: bool,
    /// Whether to buy the fertilizers a hunter doesn't have.
    pub add_missing: bool,
    /// The most seconds a slot's playtime can go up between two checks and count as being played.
    /// A save being loaded makes it jump by far more.
    pub max_step: u32,
}

/// What to do on a tick besides watching.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Harvest,
    Fertilize,
    Remain,
}

#[derive(Default)]
pub struct Tracker {
    save: usize,
    slot: Option<usize>,
    /// The hunter's name in that slot. A change means another hunter was loaded into it.
    name: Vec<u8>,
    playtime: [Option<u32>; SLOTS],
    /// Set once another slot's playtime counted up while the game's index pointed elsewhere.
    index_untrusted: bool,
    fertilizer_total: Option<u32>,
    harvest_total: Option<u32>,
}

impl Tracker {
    pub fn slot(&self) -> Option<usize> {
        self.slot
    }

    fn slot_base(&self) -> Option<usize> {
        self.slot.map(|s| self.save + s * SLOT_SIZE)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Finds the save data and the slot being played. Returns false while there is none, and on
    /// the check that first sees a hunter, so nothing is done to a slot that's still being loaded.
    fn locate(&mut self, mem: &impl Memory, root: usize, settings: Settings, log: &mut impl FnMut(String)) -> bool {
        let holder = mem.read_u64(root).filter(|&p| p != 0).map_or(0, |p| p as usize);
        let save = if holder == 0 {
            0
        } else {
            mem.read_u64(holder + SAVE_PTR_OFFSET).map_or(0, |p| p as usize)
        };
        if save == 0 {
            if self.save != 0 {
                log("save data unloaded".into());
            }
            self.reset();
            return false;
        }
        if save != self.save {
            if self.save != 0 {
                log("save data moved, looking for the active slot again".into());
            }
            self.reset();
            self.save = save;
        }

        let mut counting = Vec::new();
        for i in 0..SLOTS {
            let now = mem.read_u32(save + i * SLOT_SIZE + PLAYTIME);
            if let (Some(before), Some(now)) = (self.playtime[i], now)
                && now > before
                && now - before <= settings.max_step
            {
                counting.push(i);
            }
            self.playtime[i] = now;
        }
        let index = mem
            .read_u32(holder + SLOT_INDEX_OFFSET)
            .map(|i| i as usize)
            .filter(|&i| i < SLOTS && !self.index_untrusted);
        let slot = if !counting.is_empty() && !index.is_some_and(|i| counting.contains(&i)) {
            if index.is_some() {
                self.index_untrusted = true;
                log("the game's slot index doesn't follow the slot being played, going by playtime".into());
            }
            self.slot.filter(|s| counting.contains(s)).or(counting.first().copied())
        } else {
            index.or(self.slot)
        };
        let Some(slot) = slot else { return false };

        let mut name = vec![0u8; NAME_LEN];
        if !mem.read(save + slot * SLOT_SIZE + HUNTER_NAME, &mut name) {
            name.clear();
        }
        name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
        if name.is_empty() {
            // No hunter in the slot.
            self.slot = None;
            self.name.clear();
            return false;
        }
        if self.slot != Some(slot) || self.name != name {
            log(format!("playing save slot {} ({})", slot + 1, String::from_utf8_lossy(&name)));
            self.slot = Some(slot);
            self.name = name;
            self.fertilizer_total = None;
            self.harvest_total = None;
            return false;
        }
        true
    }

    /// One check. Fertilizes and collects after a quest, or when `command` asks for it.
    pub fn tick(
        &mut self,
        mem: &mut impl Memory,
        root: usize,
        settings: Settings,
        command: Option<Command>,
        log: &mut impl FnMut(String),
    ) {
        if !self.locate(mem, root, settings, log) {
            if command.is_some() {
                log("no save slot is being played yet, try again in a moment".into());
            }
            return;
        }
        let base = self.slot_base().unwrap();
        if !mem.enter(base + SLOT_LOCK) {
            log("error: can't lock the save slot".into());
            return;
        }
        // The lock must be let go even if something below panics, or the game would hang on it.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.check(mem, base, settings, command, log);
        }));
        mem.leave(base + SLOT_LOCK);
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    /// The check itself, with the slot locked.
    fn check(
        &mut self,
        mem: &mut impl Memory,
        base: usize,
        settings: Settings,
        command: Option<Command>,
        log: &mut impl FnMut(String),
    ) {
        let Some(ferts) = read_entries(mem, base + FERTILIZER, FERTILIZER_SLOTS) else {
            log("error: can't read the fertilizer slots".into());
            return;
        };
        let Some(harvest) = read_entries(mem, base + HARVEST_BOX, HARVEST_SLOTS) else {
            log("error: can't read the harvest box".into());
            return;
        };
        let fert_total: u32 = ferts.iter().map(|f| f.count).sum();
        let harvest_total: u32 = harvest.iter().filter(|h| h.id != 0).map(|h| h.count).sum();

        let first = self.fertilizer_total.is_none();
        let quest = self.fertilizer_total.is_some_and(|t| fert_total < t);
        let new_items = self.harvest_total.is_some_and(|t| harvest_total > t);

        if command == Some(Command::Remain) {
            log(format!("fertilizer: {}", fertilizer_list(&to_fertilizers(&ferts))));
        }
        if quest || first || command == Some(Command::Fertilize) {
            self.fertilize(mem, base, &ferts, settings, command == Some(Command::Fertilize), log);
        }
        if quest || first || new_items || command == Some(Command::Harvest) || command == Some(Command::Fertilize) {
            self.collect(mem, base, harvest_total, log);
        }

        // What it is now, after any change above, so the plugin's own writes don't count as a quest.
        self.fertilizer_total = read_entries(mem, base + FERTILIZER, FERTILIZER_SLOTS)
            .map(|f| f.iter().map(|f| f.count).sum());
        self.harvest_total = read_entries(mem, base + HARVEST_BOX, HARVEST_SLOTS)
            .map(|h| h.iter().filter(|h| h.id != 0).map(|h| h.count).sum());
    }

    fn fertilize(
        &self,
        mem: &mut impl Memory,
        base: usize,
        entries: &[Item],
        settings: Settings,
        force: bool,
        log: &mut impl FnMut(String),
    ) {
        let Some(points) = mem.read_u32(base + RESEARCH_POINTS) else {
            log("error: can't read the research points".into());
            return;
        };
        let current = to_fertilizers(entries);
        let Some(plan) = farm::plan_fertilize(current, points, settings.event, settings.add_missing, force) else {
            if force {
                log(format!("fertilizer already full: {}", fertilizer_list(&current)));
            }
            return;
        };
        if plan.cost > 0 {
            let new: Vec<Item> = plan.slots.iter().map(|f| Item { id: f.kind, count: f.remain }).collect();
            if !mem.write_u32(base + RESEARCH_POINTS, points - plan.cost)
                || !write_entries(mem, base + FERTILIZER, entries, &new)
            {
                log("error: can't write the fertilizer slots".into());
                return;
            }
            let mut what = Vec::new();
            if !plan.added.is_empty() {
                what.push(format!("added type {:?}", plan.added));
            }
            if plan.refilled {
                what.push("refilled".into());
            }
            log(format!(
                "fertilizer {} for {} research points ({} left): {}",
                what.join(" and "),
                plan.cost,
                points - plan.cost,
                fertilizer_list(&plan.slots)
            ));
        }
        if let Some(need) = plan.short_of {
            log(format!("not enough research points to fertilize: have {}, need {need}", points - plan.cost));
        }
    }

    fn collect(&self, mem: &mut impl Memory, base: usize, harvest_total: u32, log: &mut impl FnMut(String)) {
        if harvest_total == 0 {
            return;
        }
        let (Some(old_harvest), Some(old_items)) = (
            read_entries(mem, base + HARVEST_BOX, HARVEST_SLOTS),
            read_entries(mem, base + ITEM_BOX, ITEM_BOX_SLOTS),
        ) else {
            log("error: can't read the harvest box or item box".into());
            return;
        };
        let (mut harvest, mut items) = (old_harvest.clone(), old_items.clone());
        let got = farm::collect(&mut harvest, &mut items);
        if got.moved == 0 {
            log(format!("harvest box: {} items don't fit in the item box", got.left));
            return;
        }
        // Item box first: if writing the harvest box fails, items are doubled rather than lost.
        if !write_entries(mem, base + ITEM_BOX, &old_items, &items)
            || !write_entries(mem, base + HARVEST_BOX, &old_harvest, &harvest)
        {
            log("error: can't write the item box or harvest box".into());
            return;
        }
        if got.left > 0 {
            log(format!("collected {} items, {} don't fit in the item box and stay in the harvest box", got.moved, got.left));
        } else {
            log(format!("collected {} items", got.moved));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const ROOT: usize = 0x1000;
    const HOLDER: usize = 0x2000;
    const SAVE: usize = 0x10_0000;

    /// Sparse little-endian memory. Unwritten bytes read as zero inside the save, and fail outside.
    #[derive(Default)]
    struct Fake(HashMap<usize, u8>);

    impl Memory for Fake {
        fn read(&self, addr: usize, buf: &mut [u8]) -> bool {
            for (i, b) in buf.iter_mut().enumerate() {
                *b = *self.0.get(&(addr + i)).unwrap_or(&0);
            }
            addr >= ROOT
        }
        fn write(&mut self, addr: usize, data: &[u8]) -> bool {
            for (i, b) in data.iter().enumerate() {
                self.0.insert(addr + i, *b);
            }
            true
        }
    }

    impl Fake {
        fn new() -> Self {
            let mut m = Self::default();
            m.write(ROOT, &(HOLDER as u64).to_le_bytes());
            m.write(HOLDER + SAVE_PTR_OFFSET, &(SAVE as u64).to_le_bytes());
            for s in 0..SLOTS {
                m.write(Self::slot(s) + HUNTER_NAME, format!("Hunter{s}").as_bytes());
            }
            m
        }
        /// The game's index of the slot being played.
        fn set_index(&mut self, s: usize) {
            self.write_u32(HOLDER + SLOT_INDEX_OFFSET, s as u32);
        }
        fn slot(s: usize) -> usize {
            SAVE + s * SLOT_SIZE
        }
        fn set(&mut self, addr: usize, id: u32, count: u32) {
            self.write_u32(addr, id);
            self.write_u32(addr + 4, count);
        }
        fn get(&self, addr: usize) -> (u32, u32) {
            (self.read_u32(addr).unwrap(), self.read_u32(addr + 4).unwrap())
        }
        fn tick_playtime(&mut self, s: usize) {
            let a = Self::slot(s) + PLAYTIME;
            let t = self.read_u32(a).unwrap();
            self.write_u32(a, t + 5);
        }
        fn set_ferts(&mut self, s: usize, r: [u32; 4]) {
            for i in 0..4 {
                self.set(Self::slot(s) + FERTILIZER + i * ENTRY_SIZE, farm::KINDS[i], r[i]);
            }
        }
        fn ferts(&self, s: usize) -> [u32; 4] {
            std::array::from_fn(|i| self.get(Self::slot(s) + FERTILIZER + i * ENTRY_SIZE).1)
        }
        /// What the game does on a return from a quest.
        fn quest(&mut self, s: usize, harvest: &[(u32, u32)]) {
            let r = self.ferts(s);
            self.set_ferts(s, r.map(|x| x.saturating_sub(1)));
            for (i, &(id, n)) in harvest.iter().enumerate() {
                let a = Self::slot(s) + HARVEST_BOX + i * ENTRY_SIZE;
                let (_, have) = self.get(a);
                self.set(a, id, have + n);
            }
        }
    }

    fn tick(t: &mut Tracker, m: &mut Fake, cmd: Option<Command>) -> Vec<String> {
        let mut lines = Vec::new();
        let settings = Settings { event: false, add_missing: true, max_step: 20 };
        t.tick(m, ROOT, settings, cmd, &mut |l| lines.push(l));
        lines
    }

    #[test]
    fn collects_after_every_quest_and_refills_when_low() {
        let mut m = Fake::new();
        m.set_ferts(1, [9, 9, 9, 9]);
        m.write_u32(Fake::slot(1) + RESEARCH_POINTS, 5000);
        m.set(Fake::slot(1) + ITEM_BOX, 100, 10);
        m.set_index(1);
        let mut t = Tracker::default();

        tick(&mut t, &mut m, None);
        assert_eq!(t.slot(), Some(1));
        assert_eq!(m.get(Fake::slot(1) + ITEM_BOX), (100, 10), "nothing is done on the check that finds the slot");

        for quest in 1..=10 {
            m.tick_playtime(1);
            m.quest(1, &[(100, 2), (200, 3)]);
            tick(&mut t, &mut m, None);
            m.tick_playtime(1);
            tick(&mut t, &mut m, None);
            assert_eq!(m.get(Fake::slot(1) + HARVEST_BOX), (0, 0), "quest {quest}: harvest box emptied");
            assert_eq!(m.get(Fake::slot(1) + ITEM_BOX), (100, 10 + 2 * quest));
            assert_eq!(m.get(Fake::slot(1) + ITEM_BOX + ENTRY_SIZE), (200, 3 * quest));
            assert!(m.ferts(1).iter().all(|&r| r >= farm::REFILL_BELOW), "quest {quest}: {:?}", m.ferts(1));
        }
        // 9 -> 4 on quest 5 and again on quest 10.
        assert_eq!(m.read_u32(Fake::slot(1) + RESEARCH_POINTS), Some(5000 - 2 * 600));
    }

    #[test]
    fn follows_a_character_switch() {
        let mut m = Fake::new();
        m.set_ferts(0, [9, 9, 9, 9]);
        m.set_ferts(2, [9, 9, 9, 9]);
        let mut t = Tracker::default();
        tick(&mut t, &mut m, None);
        m.tick_playtime(0);
        tick(&mut t, &mut m, None);
        assert_eq!(t.slot(), Some(0));

        // Back to the title, slot 3 loaded: only its playtime moves now.
        m.set_index(2);
        m.tick_playtime(2);
        tick(&mut t, &mut m, None);
        assert_eq!(t.slot(), Some(2));
        m.tick_playtime(2);
        m.quest(2, &[(300, 1)]);
        tick(&mut t, &mut m, None);
        assert_eq!(m.get(Fake::slot(2) + ITEM_BOX), (300, 1));
    }

    #[test]
    fn waits_for_save_data() {
        let mut m = Fake::new();
        m.write(HOLDER + SAVE_PTR_OFFSET, &0u64.to_le_bytes());
        let mut t = Tracker::default();
        tick(&mut t, &mut m, None);
        m.tick_playtime(0);
        let lines = tick(&mut t, &mut m, Some(Command::Harvest));
        assert_eq!(t.slot(), None);
        assert_eq!(lines, vec!["no save slot is being played yet, try again in a moment".to_string()]);
    }

    #[test]
    fn a_save_being_loaded_is_not_a_slot_being_played() {
        let mut m = Fake::new();
        m.set_ferts(0, [9, 9, 9, 9]);
        let mut t = Tracker::default();
        tick(&mut t, &mut m, None);
        // Slot 2's playtime jumps from nothing to hours when the save is read.
        m.write_u32(Fake::slot(1) + PLAYTIME, 400_000);
        m.tick_playtime(0);
        tick(&mut t, &mut m, None);
        assert_eq!(t.slot(), Some(0));
    }

    #[test]
    fn goes_by_playtime_when_the_index_does_not_follow() {
        let mut m = Fake::new();
        m.set_ferts(2, [9, 9, 9, 9]);
        let mut t = Tracker::default();
        tick(&mut t, &mut m, None);
        assert_eq!(t.slot(), Some(0));
        m.tick_playtime(2);
        tick(&mut t, &mut m, None);
        assert_eq!(t.slot(), Some(2));
        // Paused: no playtime moves, and the index still says slot 1.
        tick(&mut t, &mut m, None);
        assert_eq!(t.slot(), Some(2));
        m.tick_playtime(2);
        m.quest(2, &[(300, 4)]);
        tick(&mut t, &mut m, None);
        assert_eq!(m.get(Fake::slot(2) + ITEM_BOX), (300, 4));
    }

    #[test]
    fn another_hunter_in_the_slot_starts_over() {
        let mut m = Fake::new();
        m.set_ferts(0, [9, 9, 9, 9]);
        let mut t = Tracker::default();
        tick(&mut t, &mut m, None);
        tick(&mut t, &mut m, None);
        // The save is loaded over the placeholder: lower fertilizer, but no quest was done.
        m.write(Fake::slot(0) + HUNTER_NAME, b"Loaded\0");
        m.set_ferts(0, [6, 6, 6, 6]);
        m.write_u32(Fake::slot(0) + RESEARCH_POINTS, 5000);
        let lines = tick(&mut t, &mut m, None);
        assert_eq!(lines, vec!["playing save slot 1 (Loaded)".to_string()]);
        tick(&mut t, &mut m, None);
        assert_eq!(m.ferts(0), [6, 6, 6, 6]);
        assert_eq!(m.read_u32(Fake::slot(0) + RESEARCH_POINTS), Some(5000));
    }

    #[test]
    fn an_empty_slot_is_left_alone() {
        let mut m = Fake::new();
        m.write(Fake::slot(0) + HUNTER_NAME, &[0; 8]);
        m.write_u32(Fake::slot(0) + RESEARCH_POINTS, 5000);
        let mut t = Tracker::default();
        tick(&mut t, &mut m, None);
        tick(&mut t, &mut m, None);
        assert_eq!(t.slot(), None);
        assert_eq!(m.read_u32(Fake::slot(0) + RESEARCH_POINTS), Some(5000));
    }

    #[test]
    fn no_points_no_change() {
        let mut m = Fake::new();
        m.set_ferts(0, [4, 4, 4, 4]);
        m.write_u32(Fake::slot(0) + RESEARCH_POINTS, 100);
        let mut t = Tracker::default();
        tick(&mut t, &mut m, None);
        m.tick_playtime(0);
        let lines = tick(&mut t, &mut m, None);
        assert_eq!(m.ferts(0), [4, 4, 4, 4]);
        assert_eq!(m.read_u32(Fake::slot(0) + RESEARCH_POINTS), Some(100));
        assert!(lines.iter().any(|l| l.contains("not enough research points")), "{lines:?}");
    }
}
