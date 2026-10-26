# Quests and jobs

Agreed with Chippy on 25 October 2026 (#70). Names, lines and numbers are provisional.

- **Quests** are the big, handmade stories: a handful per area, RuneScape-style, given by someone
  in town. A quest is a list of **steps** that follow one another. Each step has one or more
  **objectives**, and it's done when they all are:
  - **Talk** to someone.
  - **Kill** some of a monster. A kill counts for everyone with a fair share of the fight, as
    for its XP (docs/PROGRESSION.md).
  - **Gather** some of an item from nodes or carcasses, while the step is under way. What's
    already in the pack doesn't count.
  - **Deliver** some of an item to someone. They take it all at once.
  - **Reach** a place.
  - **Use** an item from the pack at a place. It's used up. A step can hand the item over as it
    begins.
- **No branching.** There is one way through a quest, and it doesn't change the world. Finishing
  one can open others (`needs`).
- **Rewards:** coins, quest XP for the player to give to any specialty, and items. Items that
  don't fit in the pack are left in a pile at the player's feet that only they can take.
- **Jobs** are the small tasks between quests. A notice board offers a few at a time, rolled from
  templates with the board's own dice, and rolls new ones every 15 minutes. A job asks for a
  number of kills, of something gathered after taking it, or of something brought to the board,
  and pays coins and quest XP for each. Everyone sees the same offers and may take each once. A
  player has up to three jobs at once and hands each in at its board.

## In the code

- The tables are `crates/content/quests/*.ron` (`content::quests`): people, boards, quests and
  job templates, checked on load.
- `sim::quests` plays them: a player's `Quests` component holds the quests under way (each at a
  step, with a count for each objective), the quests done and the jobs taken. Progress is counted
  when things happen (a kill, a swing, a talk, an item used), so a step costs nothing for players
  with no quest. Places to reach are checked every 10 steps, for players with one to reach.
- Commands: `Talk`, `StartQuest`, `TakeJob`, `HandIn`, `DropJob`; `UseItem` uses a quest's item
  where a quest wants it. Events: `QuestStarted`, `QuestStep`, `QuestDone`, `JobDone`.
- `PlayerState` carries the quest log (`quests`, `quests_done`), the people within 40 m with what
  they offer (`people`), and the board the player stands at (`board`). On the wire they are one
  part, sent only when it changes.
- People stand by the shops and stations until the townsfolk and their routines exist (#128).
  Quest progress isn't saved yet (#17). The quest log and givers in the client are #297.
