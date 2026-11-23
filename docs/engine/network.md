# Network

- **The user controls what an app reaches on the network.** ([network](/crates/waos/src/network/mod.rs))
  - *Today:* two switches, the internet (with a domain rule: any, only these, all but these) and this computer with the local network; `localhost` is answered by WaOS, under the local-network switch; each connection is checked by the names its address was looked up by. Naive, and to be reworked. ([TODO](/TODO.md))
- **A certificate is trusted only as the system trusts it:** the system's roots, those the user or an organization added, and its revocation rules. WaOS adds no root of its own. ([trust](/crates/waos/src/trust/mod.rs))
