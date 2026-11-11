---
status: proposed
---

# The reader's screen is one value, and everything else is derived from it

The reader used to track what it shows in seven separate variables (`current`, `currentView`, `currentQuizDraft`, `currentPdfState`, `lastChapterId`, `latestOpen` and `data-mode`), set by hand on each of six paths. Tickets 44, 51 and 57 were each one variable forgotten on one path. We decided that `mountReader` holds exactly one `Screen` value, a discriminated union with one variant per screen, and changes it only through one `enter(screen)` function. The saved tab position, `data-mode`, `data-state`, the footer menu and the sidebar mark are pure functions of `Screen` in `ui/src/reader-screen.ts`, each an exhaustive `switch`, so a new screen fails the typecheck until every derivation handles it.

A load that finishes late checks that the screen it started from is still the current object. That identity check replaces the `latestOpen` counter.

## Considered options

- **A reactive store or a UI framework.** Rejected. The app is plain TypeScript over DOM by design, and one closure with one value needs no subscription machinery.
- **A router with one module per screen.** Rejected for now. It is the larger version of the same idea, and it would move the study panel, annotations and PDF mounts at once. The union is the step that removes the bug class, and a split by screen can follow it without undoing it.
- **Keep the variables and add an invariant check in tests.** Rejected. It finds a forgotten update after the fact. The union makes the forgotten update not typecheck.

## Consequences

- Reader code must not write `shell.dataset.mode`, `page.dataset.state` or call `tools.updateActions` outside `enter`. A view that needs a new attribute adds a derivation, not an assignment.
- `TabPosition` stays the persisted shape. `Screen` is richer, because it also covers loading, writing and error screens that are never saved, and `positionOf(screen)` is the only place the two meet.
- An internal architecture review, finding 1, recorded the evidence and the ranking that led here. It is not published. Ticket 60 owns the refactor.
