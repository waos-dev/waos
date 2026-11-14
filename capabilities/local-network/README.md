# waos-local-network

This computer's the network as a capability: the provider of the network trait of `waos-capabilities` for the machine in front of the user, with each OS's code inside. Tauri's plugins are the reference for this cut. It depends on `waos-capabilities` only, and is chosen per program, in its environment.

- **Takes:** the network trait of `waos-capabilities`; this computer's network.
- **Provides:** the network provider for this computer.
- **Used by:** `waos-local`, which assembles this computer's defaults; any embedder that picks it on its own.

Nothing is built yet: step 7 of the design's work order fills it.
