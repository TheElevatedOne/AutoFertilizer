# Changelog

## 1.2.0

- Added: `HubOnly` (`true` by default). While a quest is set, the plugin reads only the game's quest state, one
  number per check, and leaves the save alone until the quest is over. Each change of the quest state still gets
  one check. With `false` it checks during quests too, as before.

## 1.1.0

- Fixed: when the game read the save at startup, the playtime of every slot jumped up, and the plugin took the
  first slot for the one being played. It fertilized and collected there even if you then played another hunter.
  The slot is now the one the game's own index names, with playtime as a check on it.
- Fixed: the save was changed without the game's lock on the slot, so the game could read a harvest that was half
  moved. The lock is now held for each check.
- Fixed: a hunter loaded into a slot the plugin was already watching wasn't fertilized or collected until its next
  quest.
- Changed: nothing is done on the check that first sees a hunter, only on the next, so a slot still being loaded
  is left alone. Slots without a hunter are skipped.
- Added: `AddMissing` (`true` by default, as the original did). With `false`, fertilizers the hunter doesn't have
  aren't bought.
- Changed: the default config has `Event` off. The original's file had it on, which halves prices all the time.
- Changed: the DLL is `AutoFertilizer.dll`. Delete `MHW Auto Fertilizer Plugin.dll` if you have it.

## 1.0.0

- Added: a rewrite of the original Auto Fertilizer Plugin. Collects the harvest box after every quest, refills
  fertilizer when one has fewer than 5 quests left, and follows character switches.
- Fixed over the original: items that don't fit in the item box stay in the harvest box, research points never go
  below zero, and every memory read is checked.
