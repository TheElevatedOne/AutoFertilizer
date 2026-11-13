//! Botanical Research rules, on plain copies of the save data so they can be tested off the game.
//!
//! The costs and amounts are the ones the original plugin used.

/// One fertilizer slot: which fertilizer, and how many quests it has left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fertilizer {
    pub kind: u32,
    pub remain: u32,
}

/// One item stack, in the harvest box or the item box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Item {
    pub id: u32,
    pub count: u32,
}

/// The four fertilizers the plugin keeps going.
pub const KINDS: [u32; 4] = [2, 4, 6, 8];
/// Quests a fertilizer lasts at most.
pub const MAX_REMAIN: u32 = 9;
/// Quests one refill adds.
pub const REFILL_ADD: u32 = 5;
/// Quests a fertilizer starts with when the plugin adds a missing one.
pub const NEW_REMAIN: u32 = 4;
/// Refill once any fertilizer has fewer quests left than this.
pub const REFILL_BELOW: u32 = 5;
/// Most items one item box stack holds.
pub const STACK_MAX: u32 = 9999;

/// Research points to refill all fertilizers.
pub fn refill_cost(event: bool) -> u32 {
    if event { 300 } else { 600 }
}

/// Research points to add a missing fertilizer.
pub fn add_cost(kind: u32, event: bool) -> u32 {
    let cost = if kind == 8 { 500 } else { 250 };
    if event { cost / 2 } else { cost }
}

/// What `plan_fertilize` decided.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub slots: [Fertilizer; 4],
    pub cost: u32,
    pub added: Vec<u32>,
    pub refilled: bool,
    /// Costs that were skipped because there weren't enough research points.
    pub short_of: Option<u32>,
}

/// Adds missing fertilizers (with `add_missing`) and refills them once one runs low (or always,
/// with `force`), as far as `points` allow. Returns `None` when nothing needs changing.
pub fn plan_fertilize(
    slots: [Fertilizer; 4],
    points: u32,
    event: bool,
    add_missing: bool,
    force: bool,
) -> Option<Plan> {
    let mut remain = [0u32; 4];
    for slot in slots.iter().filter(|s| s.remain > 0) {
        if let Some(i) = KINDS.iter().position(|&k| k == slot.kind) {
            remain[i] = remain[i].max(slot.remain);
        }
    }

    let mut left = points;
    let mut added = Vec::new();
    let mut short_of = None;
    for (i, &kind) in KINDS.iter().enumerate() {
        if add_missing && remain[i] == 0 {
            let cost = add_cost(kind, event);
            if left >= cost {
                left -= cost;
                remain[i] = NEW_REMAIN;
                added.push(kind);
            } else {
                short_of.get_or_insert(cost);
            }
        }
    }

    let low = remain.iter().any(|&r| r > 0 && r < REFILL_BELOW);
    let mut refilled = false;
    if force || low {
        let cost = refill_cost(event);
        if left >= cost && remain.iter().any(|&r| r > 0 && r < MAX_REMAIN) {
            left -= cost;
            for r in remain.iter_mut().filter(|r| **r > 0) {
                *r = if *r < REFILL_BELOW { *r + REFILL_ADD } else { MAX_REMAIN };
            }
            refilled = true;
        } else if left < cost {
            short_of.get_or_insert(cost);
        }
    }

    if added.is_empty() && !refilled {
        return short_of.map(|_| Plan { slots, cost: 0, added, refilled, short_of });
    }

    // Same order the original wrote them in: the four kinds first, then anything else the save had,
    // as long as there is room.
    let mut out = [Fertilizer::default(); 4];
    let mut n = 0;
    for (i, &kind) in KINDS.iter().enumerate() {
        if remain[i] > 0 {
            out[n] = Fertilizer { kind, remain: remain[i] };
            n += 1;
        }
    }
    for slot in slots.iter().filter(|s| s.remain > 0 && !KINDS.contains(&s.kind)) {
        if n < out.len() {
            out[n] = *slot;
            n += 1;
        }
    }
    Some(Plan { slots: out, cost: points - left, added, refilled, short_of })
}

/// What `collect` moved.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Collected {
    pub moved: u32,
    /// Items left in the harvest box because their item box stack is full or there is no free slot.
    pub left: u32,
}

/// Moves the harvest box into the item box: onto the item's stack if it has one, else into a free
/// slot. Whatever doesn't fit stays in the harvest box.
pub fn collect(harvest: &mut [Item], items: &mut [Item]) -> Collected {
    let mut result = Collected::default();
    for h in harvest.iter_mut().filter(|h| h.id != 0 && h.count > 0) {
        let mut left = h.count;
        for e in items.iter_mut().filter(|e| e.id == h.id && e.count < STACK_MAX) {
            let take = left.min(STACK_MAX - e.count);
            e.count += take;
            left -= take;
            if left == 0 {
                break;
            }
        }
        while left > 0 {
            let Some(e) = items.iter_mut().find(|e| e.count == 0) else { break };
            let take = left.min(STACK_MAX);
            *e = Item { id: h.id, count: take };
            left -= take;
        }
        result.moved += h.count - left;
        result.left += left;
        *h = if left == 0 { Item::default() } else { Item { id: h.id, count: left } };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ferts(r: [u32; 4]) -> [Fertilizer; 4] {
        std::array::from_fn(|i| Fertilizer { kind: KINDS[i], remain: r[i] })
    }

    #[test]
    fn nothing_to_do_above_threshold() {
        assert_eq!(plan_fertilize(ferts([9, 8, 7, 5]), 10_000, false, true, false), None);
    }

    #[test]
    fn refills_when_one_runs_low() {
        let plan = plan_fertilize(ferts([5, 5, 5, 4]), 10_000, false, true, false).unwrap();
        assert_eq!(plan.slots, ferts([9, 9, 9, 9]));
        assert_eq!(plan.cost, 600);
        let plan = plan_fertilize(ferts([4, 4, 4, 4]), 10_000, true, true, false).unwrap();
        assert_eq!(plan.slots, ferts([9, 9, 9, 9]));
        assert_eq!(plan.cost, 300);
    }

    #[test]
    fn adds_missing_and_refills() {
        let slots = [
            Fertilizer { kind: 2, remain: 7 },
            Fertilizer { kind: 6, remain: 7 },
            Fertilizer::default(),
            Fertilizer::default(),
        ];
        let plan = plan_fertilize(slots, 10_000, false, true, false).unwrap();
        assert_eq!(plan.added, vec![4, 8]);
        assert_eq!(plan.slots, ferts([9, 9, 9, 9]));
        assert_eq!(plan.cost, 250 + 500 + 600);
    }

    #[test]
    fn never_spends_points_it_does_not_have() {
        let plan = plan_fertilize(ferts([4, 4, 4, 4]), 599, false, true, false).unwrap();
        assert_eq!(plan.cost, 0);
        assert_eq!(plan.slots, ferts([4, 4, 4, 4]));
        assert_eq!(plan.short_of, Some(600));
    }

    #[test]
    fn leaves_missing_alone_when_told() {
        let slots = [
            Fertilizer { kind: 2, remain: 3 },
            Fertilizer::default(),
            Fertilizer::default(),
            Fertilizer::default(),
        ];
        let plan = plan_fertilize(slots, 10_000, false, false, false).unwrap();
        assert!(plan.added.is_empty());
        assert_eq!(plan.slots[0], Fertilizer { kind: 2, remain: 8 });
        assert_eq!(plan.slots[1], Fertilizer::default());
        assert_eq!(plan.cost, 600);
        assert_eq!(plan_fertilize([Fertilizer::default(); 4], 10_000, false, false, false), None);
    }

    #[test]
    fn force_tops_up_to_max() {
        let plan = plan_fertilize(ferts([9, 8, 6, 2]), 10_000, false, true, true).unwrap();
        assert_eq!(plan.slots, ferts([9, 9, 9, 7]));
        assert_eq!(plan_fertilize(ferts([9, 9, 9, 9]), 10_000, false, true, true), None);
    }

    #[test]
    fn collect_stacks_fills_and_keeps_overflow() {
        let mut harvest = vec![
            Item { id: 1, count: 30 },
            Item { id: 2, count: 10 },
            Item { id: 3, count: 5 },
            Item { id: 4, count: 7 },
            Item::default(),
        ];
        let mut items = vec![
            Item { id: 1, count: 9990 },
            Item { id: 2, count: 1 },
            Item { id: 9, count: 3 },
            Item::default(),
        ];
        let got = collect(&mut harvest, &mut items);
        // 1: 9 onto its stack, 21 into the free slot. 2: onto its stack. 3 and 4: no room left.
        assert_eq!(items[0], Item { id: 1, count: 9999 });
        assert_eq!(items[3], Item { id: 1, count: 21 });
        assert_eq!(items[1], Item { id: 2, count: 11 });
        assert_eq!(harvest[0], Item::default());
        assert_eq!(harvest[1], Item::default());
        assert_eq!(harvest[2], Item { id: 3, count: 5 });
        assert_eq!(harvest[3], Item { id: 4, count: 7 });
        assert_eq!(got, Collected { moved: 40, left: 12 });
    }
}
