# waos-dev-protocol

What `waos` and a running WaOS say to each other, over a Unix socket or a Windows named pipe, one command per connection. Each side says hello first, so a version mismatch is refused before anything else is sent.
