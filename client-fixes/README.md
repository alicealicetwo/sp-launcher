# Client fixes

- **Local class selection:** Unlocks all current classes in standalone local
  games by temporarily raising the local player's class eligibility level to
  5. Online matches are excluded.
- **White and Gold Super Capsules:** Corrects their merged item table buff IDs
  so both capsules can be used. The DLL verifies the original IDs and the
  replacement buff rows before changing either value, then reapplies the fix
  if the table is rebuilt.
- **First Blood audio:** Lets the first cue play in a local match, suppresses
  later bot first-kill cues, and re-arms it for the next match. It checks the
  loaded perk widget script and sound assets before changing the audio
  reference. The 25 ms widget poll can miss exceptionally close kills.
