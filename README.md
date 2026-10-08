# Guildmaster's Seat — a Rust slice

A playable slice of **Guildmaster's Seat** written in Rust. It is a 3D guild-management sim of grey-box capsules: hire adventurers, send them to their fortunes or their deaths, staff the desk, build rooms, and watch the guild grow.

It follows the road GuildGameV2 lays out to its **M8 Playable Slice / Demo** (`docs/milestones.md` in that repo). That means V2's M0–M3 systems ported, plus the M4–M7 systems V2 has only planned: persistence, staff, progression and building. The demo curates all of it into a content cut.

GuildGame (V1) supplied the class tree, skills, services and quest shapes. GuildGameV2 supplied the architecture, the rules, the rank curve, the content and the look.

```
cargo run --release -p ggr_game        # play
cargo run --release -p ggr_smoke       # headless 7-day smoke run, PASS/FAIL lines
cargo test --workspace                 # everything
```

## Building

**Requirements:** Rust 1.95 or newer.

**Linux:** Bevy needs a few development packages.

```sh
sudo apt-get install libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
```

**Windows:** nothing beyond the Rust toolchain (MSVC).

The first build compiles Bevy and takes several minutes. Debug builds optimise dependencies, so `cargo run -p ggr_game` stays playable without `--release`.

## Playing

You are not the hero. Adventurers read the **Quest Board** and pick their own work, walk out of the door, and come back days later with gold and experience. Or they come back injured, or they don't come back.

**Your levers:**

- **Hire** at the Guild Desk. Candidates wait a day. Hiring needs a clerk on shift.
- **Staff** shifts: pick a length and a start hour. Long shifts and night hours cost happiness, and happiness sets work speed. Staff who stay miserable quit.
- **Build** (B): canteen, infirmary, training dummies, benches, decor, and later the East Wing.
- **Grow** adventurers:
  - Give them an aspiration and they train toward it.
  - Accept their promotion requests.
  - Equip loot and outfitter purchases from the stash.
- Earn **renown** with every success and lose it with every death. Each tier unlocks buildings, better applicants and more work. Reaching **Renowned** completes the demo; you can keep playing after that.

The *Guild aims* panel (O) gives light nudges, never gates. The *How the guild works* page (`?`) is the reference.

| Action | Default |
|---|---|
| Select / inspect | left-click; click again or Enter |
| Next / previous character | E / Q |
| Pan · turn · zoom | WASD · Z/X or right/middle-drag · R/F or wheel |
| Pause · speed 1×/2×/4×/8×/16× | Space · 1–5 |
| Build mode · Guild aims · debug panel | B · O · F3 |
| Back, then close, then pause menu | Esc |

Every gameplay key can be rebound under *Options* (open it from the pause menu during play). Settings and saves live in the platform's user-data folder (via `directories`). There are three save slots, plus an autosave at each day boundary.

## Layout

The workspace mirrors V2's layering. Dependencies point down, and the simulation has no engine in it.

```
ggr_game     Bevy composition root: 3D view, egui HUD, input, audio, menus, settings
  ├── ggr_save     versioned envelope, per-section versions, migrations, atomic writes + backups
  └── ggr_sim      the game: commands in, events out — engine-free
        └── ggr_content   YAML content embedded at compile time, validated, sealed; localisation
              └── ggr_core      xoshiro256** RNG streams (bit-identical to V2), hashing, errors
ggr_smoke    headless smoke run + balance harness
content/     every number, name and string; tuning is a content edit
```

**V2 rules carried over:**

- **Commands in, events out.** Every player action is a validated `Command` that either commits whole or is refused with a reason the notice line shows. The view drains `SimEvent`s and decides nothing.
- **Fast-forward is more ticks.** The sim advances one game-minute at a time. A test proves 16× lands on the same state hash as 1×.
- **Timed walks.** The arrival minute is fixed when a walk is issued, and the 3D view only draws it. A walk is the same code path at 1×, at 16×, or with no view at all.
- **One seeded root RNG, split into named streams** (`dice`, `questgen`, `names`, `economy`, `behaviour`, `chargen`). It is our own xoshiro256**, and V2's golden vectors are its tests.
- **Every user-facing string is a localisation key** (`content/lang/en.yaml`). A test scans the game's source for every key it uses. The content validator checks every key the content names.
- **No singletons or service locators.** Paths are built with `Path::join`, and saves never spell a separator.

## What is in the slice, by V2 milestone

| Milestone | In this slice |
|---|---|
| **M0 Skeleton** | Workspace with an engine-free sim. CI workflow running fmt, clippy `-D warnings`, tests and smoke on Linux and Windows. |
| **M1 Clock & content** | <ul><li>Minute clock and bucket scheduler with serial guards and a stale-event counter.</li><li>Named RNG streams matching V2's goldens.</li><li>YAML content with a validator: a typo'd id fails naming the valid set, and duplicates, missing localisation keys and malformed itineraries also fail.</li></ul> |
| **M2 Actors in space** | <ul><li>Logical grid with 8-connected, no-corner-cutting BFS.</li><li>Slots go reserve → arrive → leave, with an audit and a consistency check.</li><li>V2's travel-time formula. Capsules walk paths the sim timed. Camera rig, click-to-inspect, debug panel.</li></ul> |
| **M3 Core loop** | <ul><li>Quest templates (V2's eight plus seven curated ones) and procedural quests (V1's "{objective} {target}" generator on V2's itinerary).</li><li>A board with capacity and expiry. Adventurers self-select.</li><li>Pre-rolled itineraries using V2's d20 check and consequence table. Gold, loot, XP, injury, permadeath.</li><li>Manned desk, candidate windows, a treasury that refuses debits it can't cover. The HUD after V2's `DESIGN.md`.</li></ul> |
| **M4 Persistence** | <ul><li>Versioned envelope with per-section versions and a one-step migration chain.</li><li>Unknown sections carried through. The camera rides as a view-owned section.</li><li>Atomic temp-then-rename writes with 3 rolling backups, and fallback to a backup if a slot is corrupt.</li><li>Committed fixture `tests/fixtures/demo_v1.json`. Save → load → resimulate equals never saving.</li></ul> |
| **M5 Staff & services** | <ul><li>Clerk, cook and medic roles. Shift template plus start hour.</li><li>Happiness drifts toward an itemised target (long shifts, night hours, unpaid wages, decor, a staffed canteen). Work speed follows happiness; quitting follows misery.</li><li>Daily wages.</li><li>Canteen: hunger, meals bought, a well-fed bonus and a starving penalty on the road. Infirmary: faster recovery with a medic.</li><li>Queues at unstaffed counters, with patience.</li></ul> |
| **M6 Progression** | <ul><li>Four branches (Fighter, Rogue, Mage, Holy), each one tier deep: a base class plus two advanced classes, from V1's tree.</li><li>Per-branch aptitude. Eight skills raised by quest legs and training.</li><li>An aspired class with a met/close/failing "what's missing" list.</li><li>Promotion F→E→D offered at the desk with a window and a fee.</li><li>Three armour slots and two hands; two-handed items take both. Bonuses are capped per rank.</li><li>The quest report prints every roll.</li></ul> |
| **M7 Building** | <ul><li>Prefab placement with footprints and use-slots. Rules are validated in the sim command: bounds, walls, overlap, the door, and reachability from the door. The ghost uses the same function.</li><li>Timed construction paid up front. Demolish refunds half. Users of a demolished building resolve through the normal rules.</li><li>The East Wing expansion.</li></ul> |
| **M8 Demo** | <ul><li>Ranks capped at D, a curated plus procedural quest pool, three services, a bounded build palette.</li><li>Renown tiers and a goal. Nudge objectives, a first-run welcome, tooltips, and the "how the guild works" reference.</li><li>Options: volume, effects, fullscreen, window size, interface scale, a language dropdown, and gameplay rebinding.</li><li>Generated audio. A difficulty floor: a volunteer for an empty, broke guild, and the guildmaster signs when no clerk is employed.</li><li>A balance pass measured by the harness and a scripted player.</li></ul> |

## The cut line: what this demo does not show

These were left out on purpose. They are content and polish, not missing systems.

- Ranks above D. They exist on the ladder but are never reached.
- Class tiers beyond the first. Party quests.
- Needs beyond hunger. Research, reputation effects beyond renown, and mods.
- Character art and animation: capsules, as V2's policy has it until its M11.
- Gamepad support (keyboard and mouse only).
- Free-form walls and room drawing (prefabs only).
- A* navigation meshes. The sim's BFS grid is the navigation.
- More languages: the registry is complete, but only English is authored.

## Where it departs from V2, and why

- **Modifier cap is `2 + rank`** instead of V2's `0, 1, 2…`. At F, V2's 0 left progression nothing to give a new adventurer. The balance harness bands were re-measured at this value.
- **The guild banks 70% of quest gold, and adventurers cost daily upkeep.** Measured play without these left 10,000+ gold idle by day 30.
- **Adventurers take the highest-rank work they qualify for**, uniformly among those quests. V2 drew uniformly over all eligible quests. Promotion needs quests at rank, so taking easy work stalled careers.
- **A save records a fingerprint of the content's ids** and is refused against different content, rather than remapping ids. The in-game note says demo saves may not carry into the full game, which is the M8 decision V2 asked to be made explicitly.
- **The hall is a new 40×28 layout** with an East Wing, not V2's 32×24 test hall. So V2's smoke-run state hash is not reproduced. The RNG is: the smoke run checks V2's first `dice` value for seed 42.

## Verifying

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run --release -p ggr_smoke -- --days 7 --seed 42 --balance 10000 --play 14
```

**Tests cover:**

- RNG goldens against V2.
- 16× vs 1× state hashes.
- The slot audit with 20 wanderers for a week.
- Invariants over 10 seeds × 7 days: gold never negative, no orphaned quests, a per-state stuck rule.
- Placement rules, including sealing the door off.
- Demolishing an occupied building. The last desk can't go.
- The desk gating hires. Queueing at an unstaffed canteen.
- Night shifts costing happiness. A 30-day happiness equilibrium.
- Gear moving outcomes. Two-handed items. The modifier cap.
- Aligned aspirations outpacing misaligned ones. Class attainment. Promotion.
- The East Wing. The volunteer floor.
- Save round trips at random ticks across a week. A crash mid-write. Corrupt-slot fallback. Bounded backups. Migration chains. Unknown sections. The committed fixture.
- Balance bands at 2,000 runs per rank.
- A reasonably played guild reaching Renowned on days 5–15 without spiralling.
- 60 hands-off days never soft-locking.
- Every interface string resolving.

**Unattended runs of the real game** (Xvfb is enough on Linux):

```sh
guildmasters_seat --seed 42 --autoplay --screenshot hud.png --after-minutes 3000 --scene hud
#   scenes: hud, sheet, staff, quest, candidate, build, chronicle, outfitter, menu, options, howto, main
guildmasters_seat --seed 9 --autoplay --selftest-save --after-minutes 1500 --speed 16
```
