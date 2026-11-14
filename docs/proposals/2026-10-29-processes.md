# Processes

**Decisions taken (2026-10-29).** The lead took these, one question at a time, starting step 7's processes. Already settled: the usual process model, a command (program, arguments, working folder, environment) started with pipes or in a terminal (v1 interface decision 9, `2026-10-17-waos-v1-interface.md`); a switch, off by default, with a rule of which commands, any or only some, as Deno's `--allow-run` (permissions decisions 4 and 8). Starting programs is no UI framework's job, so processes are WaOS's alone, served by the `local-processes` capability, the machine's side, with an SSH machine's provider later.

1. **What a program starts runs outside its sandbox**, as Deno documents that `--allow-run` lets a script escape its permissions: an allowed command runs as the user, with everything the user can reach, and the switch's page in the program's settings says so. Confining started processes to the program's mounts and network rules, as AI coding tools confine the commands they run (Seatbelt on macOS, Landlock and seccomp on Linux, an AppContainer on Windows), is subtle and runs into trouble in practice: a tool needing a folder or a host outside what was mounted fails in ways that are hard to see.
2. **A program acts on machines, not on processes** (2026-10-29): a machine the user lets a program use, this computer or an SSH machine, gives its files from its root and runs commands on it, with pipes or in a terminal, as one SSH login gives files (SFTP), commands (`exec`) and terminals (`pty`) and as VS Code's Remote Development and Zed's SSH projects hold a machine; a program may hold several. It names what running a command already grants (decision 1): acting on the machine as its user. It replaces the processes switch and its rule of commands (permissions decisions 4 and 8) with a grant per machine, all or nothing as a login is, shown on the program's settings page as a permission of its own with a warning, and it stays apart from mounts, the everyday way a program sees the user's folders. `local-processes` becomes this computer's machine provider; an SSH machine's comes with the same interface. The experiment's remote design ran a whole program against one machine; here a program keeps its own sandbox and acts on the machines it holds.
3. **A program gets a machine the way an app gets a photo through the system's picker** (2026-10-29): it asks, WaOS shows the machines (this computer, and SSH machines once their provider exists), the user picks one, and the pick is the grant, kept until the user takes it back, as iOS's photo picker gives an app only the photos picked, without the app seeing the library. A program lists only the machines granted to it, never all the user's, which would tell it the user's hosts.
4. **A command runs in the machine user's login environment**, as an SSH login gives and as VS Code reads it on this computer, running the user's login shell once (`$SHELL -i -l -c env`, with a timeout and a fallback) so that a WaOS started from the Dock still finds Homebrew's and rustup's tools; the program's own variables go on top.
5. **Commands run by `exec`**, SSH's name for running a command on a channel, and `exec-in-terminal` for one given a terminal, as SSH's `exec` after a `pty-req`.
6. **The WIT** (2026-10-29): a machine gives its files from its root as `wasi:filesystem`'s own `descriptor`, served by WaOS, and runs a command with `exec` (pipes) or `exec-in-terminal`; a command's variables go on top of the login environment; `machines.granted` lists the machines the user picked for the program, and `pick` asks for one, none when the user cancels. The picker offers this computer for now, and SSH machines once their provider is built.

```wit
interface machines {
  use wasi:filesystem/types@0.2.6.{descriptor};

  resource command {
    constructor(program: string);
    add-argument: func(argument: string);
    working-directory: func(path: string);
    variable: func(name: string, value: string);
  }

  resource child {
    signal: func(signal: signal);
    exit-status: async func() -> exit-status;
  }
  resource pipes {
    write: async func(data: list<u8>) -> result<_, exec-error>;
    close-input: func();
    output: func() -> option<stream<u8>>;
    errors: func() -> option<stream<u8>>;
  }
  resource terminal {
    write: async func(data: list<u8>) -> result<_, exec-error>;
    output: func() -> option<stream<u8>>;
    resize: func(size: terminal-size);
  }

  resource machine {
    name: func() -> string;
    root: func() -> descriptor;
    exec: async func(command: borrow<command>) -> result<tuple<child, pipes>, exec-error>;
    exec-in-terminal: async func(command: borrow<command>, size: terminal-size) -> result<tuple<child, terminal>, exec-error>;
  }

  resource machines {
    granted: func() -> list<machine>;
    pick: async func() -> option<machine>;
  }

  get-machines: func() -> option<machines>;
}
```
7. **Machines are the one way; the processes switch and its rule of commands go** (2026-10-29): `permissions.processes`, `ProcessGrant`, `CommandRule`, the store's `[processes]` and the settings page's commands are removed, the package still at 0.1.0. Keeping `processes` to mean "holds a machine" would mix a switch with picks, and a getter answering denied for ever would be a dead call. *This replaces permissions decisions 4 and 8 on processes.*
8. **A machine's root resolves paths with the system's ordinary calls** (2026-10-29): the grant is everything the machine's user can reach, and the program can already read any file by running a command, so confining `root` beneath `/` as a mount is confined (`O_RESOLVE_BENEATH`, permissions decision 10) protects nothing and only breaks ordinary files, an absolute symlink such as a dotfile linked into a dotfiles folder, or `..` above `/`. Mounts keep their confinement.
9. **The WIT as built is kept, with an `interrupt` signal** (2026-10-29): the types decision 6 named but did not write come from the experiment's `processes.wit` (`terminal-size`, `signal`, `exit-status`, `exec-error`), and the root is `wasi:filesystem@0.2.12`'s descriptor, the version WaOS serves. `interrupt` is what Ctrl-C sends, for a command run with pipes, which has no terminal to send it through: `SIGINT` on Unix, `CTRL_C_EVENT` to a console process group on Windows, and SSH's `signal` request `INT`.
