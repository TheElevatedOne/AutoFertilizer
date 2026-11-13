# AutoFertilizerPlugin

![Static Badge](https://img.shields.io/badge/CLAUDE-VIBECODED?style=for-the-badge&logo=claude&label=VIBECODED&labelColor=%23333333&color=%23de7356)

A Monster Hunter: World plugin for Stracker's Loader that, after every quest, moves the Botanical Research harvest
box into your item box and keeps the fertilizers topped up with research points. It's a rewrite in Rust of
[Auto Fertilizer Plugin][original] by its original author on Nexus Mods, which sometimes stopped working; this one
replaces it and reads the same config file.

> [!IMPORTANT]
> Version 1.1.0 hasn't been run in the game. 1.0.0 was started in the game once, without a quest being played.
> Harvesting and fertilizing are covered by unit tests against a fake save. Check `AutoFertilizer.log` after your
> first quests.

> [!WARNING]
> The plugin changes your save in memory: research points, the fertilizer slots, the harvest box and the item box.
> Back up your save first, like with any plugin that touches it. Remove the original
> `MHW Auto Fertilizer Plugin.dll` before using this one; the two must not run together.

## What it does

- **Collects after every quest.** The harvest box goes into the item box: onto the item's stack if you have one,
  else into a free slot. A stack holds 9999.
- **Never throws items away.** What doesn't fit in the item box stays in the harvest box, and the log says how
  much.
- **Refills fertilizer before it runs out.** Once any fertilizer has fewer than 5 quests left, all of them are
  refilled for 600 research points: +5 quests each, up to 9.
- **Buys the fertilizers you don't have** (types 2, 4, 6 and 8 in the save, the four the original plugin used),
  for 250 research points each, 500 for type 8, with 4 quests. Turn this off with `AddMissing`.
- **Only spends points you have.** When you're short, nothing is changed and the log says what it needed.
- **Follows the hunter you play.** Switching characters or going back to the title screen is picked up on the
  next check.
- **Writes a log**, `AutoFertilizer.log` next to the DLL, with every refill and collection.

## What's fixed over the original

Found by disassembling the original DLL:

| Original | This plugin |
|---|---|
| Collected only when fertilizer was refilled, about one quest in five once all four ran level | Collects after every quest |
| Worked out the save slot once, 30 s after launch | Checks the slot on every pass |
| Cleared the harvest box even when the item box was full | Leaves what doesn't fit |
| Could spend research points you don't have | Skips what you can't pay for |
| Read save data without checking it was loaded, which could crash the game | Checks every read and write |
| Changed the save without the game's lock | Holds the save slot's lock while it works |

## Requirements

- `MonsterHunterWorld.exe` version 15.23.00 (build 421810, PE timestamp `0x66D13CB6`). On any other version the
  plugin logs the mismatch and changes nothing.
- [Stracker's Loader][loader].

The plugin doesn't change the game's code, so it needs no CRC bypass.

## Install

1. Delete `MHW Auto Fertilizer Plugin.dll`, the original plugin, from `Monster Hunter World/nativePC/plugins/` if
   it's there. Keep `AutoFertilizer-config.json` if you like your settings.
2. Download `AutoFertilizerPlugin-<version>.zip` from [Releases][releases] and unpack it into the
   `Monster Hunter World` folder, or copy `AutoFertilizer.dll` and `AutoFertilizer-config.json` to
   `Monster Hunter World/nativePC/plugins/` yourself, or run `make install`.
3. Start the game. The plugin writes `AutoFertilizer.log` next to the DLL.

A working session looks like this:

```text
AutoFertilizer 1.1.0
checking every 5000 ms, event prices off, missing fertilizers are bought
[00:01:10] playing save slot 1 (Hunter)
[00:14:35] collected 37 items
[00:52:05] fertilizer refilled for 600 research points (41250 left): type 2 x9, type 4 x9, type 6 x9, type 8 x9
[00:52:05] collected 41 items
```

## Configuration

The file is `AutoFertilizer-config.json`, next to the DLL. The plugin creates it with the defaults if it's
missing, and reads it at game start.

| Key | Default | Description |
|---|---|---|
| `CheckRate` | `5000` | Milliseconds between checks of the save, 500 at least |
| `Event` | `false` | Half prices, for when the game's event discounts fertilizer. The original's file had `true` |
| `AddMissing` | `true` | Buy the fertilizers the hunter doesn't have. With `false` only the ones in use are refilled |

`Toggle` from the original's file is ignored, as the original ignored it.

> [!NOTE]
> With `AddMissing` on, a hunter gets all four fertilizers as soon as they have the research points, whether or not
> the Botanical Research Center has offered them yet. Set it to `false` on a new character if you don't want that.

## Console commands

Like the original, the plugin connects to a console listening on `127.0.0.1:50382` when the game starts, if there
is one. It takes these commands and answers with `console <text>`:

| Command | Does |
|---|---|
| `autofarm harvest` | Collects the harvest box now |
| `autofarm fert` | Refills fertilizer now, up to 9 quests, and collects |
| `autofarm remain` | Lists the fertilizers and their quests left |
| `autofarm reload` | Reads the config file again |

## How it works

The plugin runs on a thread of its own and hooks nothing. Every `CheckRate` milliseconds it reads the save data
the game keeps in memory: a pointer at RVA `0x5013950` leads to an object with the three save slots at `+0xA8`
(0x26CC00 bytes each) and the index of the slot being played at `+0xA0`.

| In a slot | Offset |
|---|---|
| Critical section | `0x08` |
| Hunter name | `0x50` |
| Research points | `0x98` |
| Playtime, seconds | `0xA0` |
| Item box, 200 entries | `0x38A10` |
| Fertilizer, 4 entries | `0x103070` |
| Harvest box, 50 entries | `0x1030C0` |

Entries are 16 bytes: an id, a count, and 8 bytes the plugin leaves alone.

A return from a quest shows as the fertilizers' quests going down, or as more items in the harvest box. The plugin
then fertilizes and collects while holding the slot's critical section, the one the game's menus take when they
change the slot. It also does so once when it first sees a hunter, one check after, so a slot that's still being
loaded is left alone.

The slot is the one the game's index names. If another slot's playtime is the one counting up, a few seconds per
check, that slot is used instead and the log says so. A save being loaded makes playtime jump by hours, which
doesn't count.

## Known issues

- Not run in the game yet, see the note at the top.
- The item box and harvest box are changed in memory. If the item box is open while a check runs, it may show the
  old amounts until you reopen it.
- Items go to the first 200 item box entries, the Items tab, as in the original.

## Building

The Makefile uses the Rust and MinGW toolchain in `~/workdir/elden/ERBetterLod/.toolchain`. Run `make toolchain`
there first if it's missing, or point `TOOLCHAIN` at another copy.

```sh
make                # builds target/x86_64-pc-windows-gnu/release/AutoFertilizer.dll
make test           # unit tests, on Linux, against a fake save
make install        # copies the DLL and config to the game's nativePC/plugins
make install PLUGINS_DIR=/path/to/nativePC/plugins
make uninstall
```

`make install` keeps a config you already have, and renames the original plugin to `.bak` if it finds it.

## Credits

The idea, the save offsets, the prices and the config file come from [Auto Fertilizer Plugin][original] on Nexus
Mods. No code or files of the original are in this repository.

[original]: https://www.nexusmods.com/monsterhunterworld/mods/3626?tab=description
[loader]: https://www.nexusmods.com/monsterhunterworld/mods/1982
[releases]: https://github.com/TheElevatedOne/AutoFertilizerPlugin/releases
