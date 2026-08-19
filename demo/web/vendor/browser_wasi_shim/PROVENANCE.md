# `@bjorn3/browser_wasi_shim` — vendored

A pure-JavaScript WASI preview1 host. The demo page runs the slither
runner's `.wasm` through it inside a Web Worker.

- **Package:** `@bjorn3/browser_wasi_shim`
- **Version:** `0.4.2`
- **Upstream:** <https://github.com/bjorn3/browser_wasi_shim>
- **Licence:** `MIT OR Apache-2.0` — both texts are beside this file,
  copied from the published tarball unmodified.

## What was taken

The published `dist/` — eight ESM modules, ~43 KB total — plus both
licence files. Nothing else: no `typings/`, no build config, no tests.
The files are **byte-for-byte as published**; nothing here has been
edited, reformatted or minified further.

```
sha256(bjorn3-browser_wasi_shim-0.4.2.tgz)
  = 9c0281520d0e99f027ec7c1c79b4036c0f8168ed9bf98aba19db4737a1333782

a91848ee180529e2a60c05dfb9584cad19cd4e1c6f391fdb76a938bcae4c0328  debug.js
9e82e1fc1bfd3e3573f64349dc42b4b624ed61d24e5c553f2bb4d041444f166c  fd.js
85dbc9e0ee784d9ff8b55452644e00bf7058e32355aab974f8b71d7d85772324  fs_mem.js
4b96aaeb5ac5986cf802cbf22b975c656682d22a38248160c96fc2ded5644869  fs_opfs.js
7e2fd52ee3f728bb0b1d6e449724e0f13e3d586bb25bde6e02a66366175b5605  index.js
ece435d3784d928d02bff4d015b7cb686f8c06de8536ff9f8ebc38a8f403a3be  strace.js
0db0f42ba330749a7b05095ea1fd0ff63fd2b30e84cead30fe4c28359d15f194  wasi_defs.js
168eb977a826f75ab0c39f9322f78cc58dbd5b233019ad1d6a7e940af8a7c4aa  wasi.js
```

## Why vendored rather than fetched from a CDN

GitHub Pages serves the site; a page that reaches a third-party host at
runtime is one outage — or one supply-chain event — away from lying about
what it is running. The whole site is self-contained, so the bytes a
visitor executes are the bytes in this repository.

## The one implementation detail the demo depends on

`wasi.js`'s `poll_oneoff` is a **busy-wait spin**:

```js
poll_oneoff(in_ptr, out_ptr, nsubscriptions) { …
  const endTime = …;
  while (endTime > getNow()) {}
  …
}
```

If the runner's tokio runtime ever parked on a real timer, the Worker
would spin one core for the whole wall duration — a 25-second protocol
timeout would be a 25-second frozen spin. It never does: the runtime is
built with `start_paused(true)`, so tokio auto-advances virtual time
whenever every task is idle and never asks the host to wait.

`demo/verify/run-vendored-shim.mjs` asserts this rather than trusting it:
it counts calls into `poll_oneoff` and fails if the count is not zero.

## Updating

Re-run `npm pack @bjorn3/browser_wasi_shim@<version>`, copy `dist/*.js`
and both licences here, update the hashes above, and re-run
`demo/verify/run-vendored-shim.mjs`. There is no `package.json` and no
`node_modules` in this repository on purpose.
