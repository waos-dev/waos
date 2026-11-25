# waos-dev-protocol

What `waos` and a running WaOS say to each other, one command per connection: over a Unix socket or a Windows named pipe on the same machine, or across the network as a WebSocket, `wss://` to the device's own certificate, which a paired computer pins by its fingerprint, and `ws://` from the device itself alone. Each side says hello first, so a version mismatch is refused before anything else is sent; across the network the computer's hello carries the token the device gave it.

It takes tokio for the streams, tungstenite and rustls for the WebSocket and its TLS, and rcgen for the device's certificate. The WaOS app serves it (`apps/waos`'s dev server); `waos-cli` speaks it.
