# waos-activity

What a program did over the days WaOS keeps of it, its audit summary, summed across the days and said in plain words: one section per capability, as the inspector's Activity shows it, and a line per capability in short, as a program's settings page in WaOS shows it (decision 2 of the inspector's first panels in `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`). Plain Rust with no dependency, so a program and the app say the same of the same days.

- **Takes:** an `Activity`, filled day by day by its user from its own types of a day: `wasdk:inspect`'s `audit.day` in the inspector, `waos-capabilities`' `AuditDay` in the app.
- **Provides:** `Activity`, with what it sums (`Captures`, `Counts`, `Named`, `CommandRuns`, `Folder`, `Asked`); its `sections`, each a `Section` of plain lines; its `short`, a line per capability; its `marks`, what it used, by name; and `when`, a time of day said against now.
- **Used by:** `builtin-apps/inspector` and `apps/waos`'s settings page.
