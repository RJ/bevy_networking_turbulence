# bevy_networking_turbulence

Networking plugin for [Bevy engine][1] running on [naia-socket][2] - crossplatform UDP, including the web (with WebRTC).

This is a stripped down version of [bevy_networking_turbulence][3], with the turbulence bits removed.

No delivery guarantees! Use [3] if you want something higher level.

[1]: https://github.com/bevyengine/bevy
[2]: https://github.com/amethyst/naia-socket
[3]: https://github.com/smokku/bevy_networking_turbulence
[4]: https://github.com/smokku/bevy_networking_turbulence/milestones

## Testing

### Native

On one terminal run:

    $ cargo run --example simple -- --server

On other terminal run:

    $ cargo run --example simple -- --client

Observe `PING`/`PONG` exchange between server and client. You can run more clients in more terminals.

### WASM

Server:

    $ cargo run --example simple --features use-webrtc --no-default-features -- --server

Change IP address in `examples/simple.rs` / `startup()` function to point to your local machine, and run:

    $ cargo build --example simple --target wasm32-unknown-unknown --no-default-features --features use-webrtc
    $ wasm-bindgen --out-dir target --target web target/wasm32-unknown-unknown/debug/examples/simple.wasm

Serve project directory over HTTP. For example (`cargo install basic-http-server`):

    $ basic-http-server .

Open <http://127.0.0.1:4000> and watch Browser's console in Developer Tools.
You will see the same `PING`/`PONG` exchange as in the Native mode.
