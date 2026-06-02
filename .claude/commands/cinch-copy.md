Save the answer you just grabbed with Claude Code's built-in `/copy` into a
cinch clip — synced across your devices and searchable — without leaving the
session.

Flow:
  1. Run `/copy` first. Claude Code puts the last answer on your system clipboard.
  2. Run `/cinch-copy [label]`. This ingests whatever `/copy` placed on the
     clipboard into cinch (the optional argument sets the clip label;
     default: "Claude Code /copy").

The answer is already on your clipboard from `/copy`; this command is what makes
it persistent — synced to your other devices and searchable in clip history.

For cross-session use — picking a specific older answer, or copying a whole
session — use the full `cinch session copy` command instead (interactive
skim picker with preview, `--last N`, `--all`).

!`L="$ARGUMENTS"; pbpaste | cinch push -l "${L:-Claude Code /copy}"`
