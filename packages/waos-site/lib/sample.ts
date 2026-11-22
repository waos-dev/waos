/** The commands that install the tools, make an app and run it on macOS and Linux. */
export const helloCommands = 'curl -fsSL https://waos.app/install.sh | sh\nwaos new hello\ncd hello && waos run';

/** The same commands in PowerShell on Windows. */
export const windowsHelloCommands = 'irm https://waos.app/install.ps1 | iex\nwaos new hello\ncd hello; waos run';
