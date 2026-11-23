# Machines

- **An app works across machines, not just the one it runs on.** Once its user picks a machine, the app reads its files and runs its commands there, as the user, outside the app's sandbox. The app never sees machines it wasn't given. ([machines](/crates/waos/src/machines/mod.rs))
  - *Why:* an editor or a terminal needs real machines (git, compilers, language servers), wherever they are; the user's pick is their consent.
- **A picked machine stays the app's until the app ends,** as an open file does, and the pick is kept in its settings for next time. ([machines](/crates/waos/src/machines/mod.rs))
- **On a machine, paths are the machine's:** a file read or a command run there sees it as any program on it does, symlinks and all. ([target.rs](/crates/waos/src/files/target.rs))
- **A command ends with the app that started it,** along with everything it started; dropping its handle leaves it running until then. ([machines](/crates/waos/src/machines/mod.rs))
- **Any kind of machine:** this computer, a server over SSH, and others. ([local-processes](/capabilities/local-processes))
  - *Today:* only This computer; SSH comes next. ([TODO](/TODO.md))
